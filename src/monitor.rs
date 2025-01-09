use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{SystemTime, Duration};
use bollard::Docker;
use bollard::container::{ListContainersOptions, RemoveContainerOptions};
use bollard::models::{HostConfig, EventMessageTypeEnum};
use bollard::system::EventsOptions;
use futures::StreamExt;
use futures::TryStreamExt;
use crate::models::ContainerConfig;
use crate::restart::ContainerRestarter;
use crate::docker::DockerClient;

pub struct ContainerMonitor {
    docker: Docker,
    restarter: Arc<ContainerRestarter>,
    pub container_configs: Mutex<HashMap<String, ContainerConfig>>,
}

impl ContainerMonitor {
    pub async fn new() -> anyhow::Result<Self> {
        let docker = DockerClient::get().await?;
        let restarter = Arc::new(ContainerRestarter::new(docker.clone()).await?);
        let monitor = Self { 
            docker: docker.clone(), 
            restarter,
            container_configs: Mutex::new(HashMap::new()),
        };
        
        monitor.init_containers().await?;
        
        Ok(monitor)
    }

    async fn init_containers(&self) -> anyhow::Result<()> {
        let mut filters = HashMap::new();
        filters.insert("status", vec!["running", "created", "exited", "paused"]);
        
        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        println!("发现 {} 个容器", containers.len());
        
        for container in containers {
            if let Some(id) = &container.id {
                println!("正在加载容器配置: {}", id);
                if let Ok(inspect) = self.docker.inspect_container(id, None).await {

                    let image = container.image.clone().unwrap_or_default();
                    let name = inspect.name.unwrap_or_default().trim_start_matches('/').to_string();
                    println!("容器名称: {}, 镜像: {}", name, image);
                    
                    let config = ContainerConfig {
                        container_id: id.clone(),
                        name,
                        image: image.clone(),
                        host_config: inspect.host_config.clone(),
                        config: inspect.config.map(|c| bollard::container::Config {
                            hostname: c.hostname.clone(),
                            domainname: c.domainname.clone(),
                            user: c.user.clone(),
                            attach_stdin: c.attach_stdin,
                            attach_stdout: c.attach_stdout,
                            attach_stderr: c.attach_stderr,
                            exposed_ports: c.exposed_ports.clone(),
                            tty: c.tty,
                            open_stdin: c.open_stdin,
                            stdin_once: c.stdin_once,
                            env: c.env.clone(),
                            cmd: c.cmd.clone(),
                            image: Some(image),
                            volumes: c.volumes.clone(),
                            working_dir: c.working_dir.clone(),
                            entrypoint: c.entrypoint.clone(),
                            network_disabled: c.network_disabled,
                            mac_address: c.mac_address.clone(),
                            labels: c.labels.clone(),
                            ..Default::default()
                        }),
                        ip_address: self.get_container_ip(id).await,
                    };

                    let mut configs = self.container_configs.lock().unwrap();
                    configs.insert(id.clone(), config);
                    println!("已保存容器 {} 的配置", id);
                } else {
                    eprintln!("无法获取容器 {} 的详细信息", id);
                }
            }
        }

        let config_count = self.container_configs.lock().unwrap().len();
        println!("已加载 {} 个容器配置", config_count);
        
        // 打印所有已加载的容器ID
        let configs = self.container_configs.lock().unwrap();
        println!("已加载的容器ID列表:");
        for id in configs.keys() {
            println!("- {}", id);
        }
        
        Ok(())
    }

    async fn ensure_docker_connection(&self) -> anyhow::Result<()> {
        if !DockerClient::check_health(&self.docker).await {
            return Err(anyhow::anyhow!("Docker连接已断开"));
        }
        Ok(())
    }

    pub async fn start_monitoring(&self) -> anyhow::Result<()> {
        let mut events = self.docker.events(None::<EventsOptions<String>>);
        let mut restart_records: HashMap<String, RestartRecord> = HashMap::new();

        println!("开始监控容器状态...");

        // 先检查现有的已停止容器
        self.check_stopped_containers().await?;

        while let Some(event) = events.next().await {
            match event {
                Ok(event) => {
                    if let Some(EventMessageTypeEnum::CONTAINER) = event.typ {
                        if let Some(status) = event.action {
                            if let Some(id) = event.actor.and_then(|a| a.id) {
                                match status.as_str() {
                                    "die" | "stop" | "kill" => {
                                        println!("检测到容器停止: {}", id);
                                        if let Err(e) = self.handle_container_stop(&id, &mut restart_records).await {
                                            eprintln!("处理容器停止事件失败: {}", e);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("监控事件错误: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn check_stopped_containers(&self) -> anyhow::Result<()> {
        println!("检查已停止的容器...");
        let mut filters = HashMap::new();
        filters.insert("status", vec!["exited", "dead"]);
        
        let options = ListContainersOptions{
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        println!("发现 {} 个已停止的容器", containers.len());
        
        for container in containers {
            if let Some(id) = container.id {
                println!("准备重启容器: {}", id);
                
                // 保存镜像名称
                let image = container.image.clone().unwrap_or_default();
                
                // 尝试重启容器
                match self.docker.start_container::<String>(&id, None).await {
                    Ok(_) => {
                        println!("✅ 容器 {} 重启成功", id);
                        
                        // 更新配置
                        if let Ok(inspect) = self.docker.inspect_container(&id, None).await {
                            let config = ContainerConfig {
                                container_id: id.clone(),
                                name: inspect.name.unwrap_or_default().trim_start_matches('/').to_string(),
                                image: image.clone(),
                                host_config: inspect.host_config,
                                config: inspect.config.as_ref().map(|c| bollard::container::Config {
                                    hostname: c.hostname.clone(),
                                    domainname: c.domainname.clone(),
                                    user: c.user.clone(),
                                    attach_stdin: c.attach_stdin,
                                    attach_stdout: c.attach_stdout,
                                    attach_stderr: c.attach_stderr,
                                    exposed_ports: c.exposed_ports.clone(),
                                    tty: c.tty,
                                    open_stdin: c.open_stdin,
                                    stdin_once: c.stdin_once,
                                    env: c.env.clone(),
                                    cmd: c.cmd.clone(),
                                    image: Some(image),
                                    volumes: c.volumes.clone(),
                                    working_dir: c.working_dir.clone(),
                                    entrypoint: c.entrypoint.clone(),
                                    network_disabled: c.network_disabled,
                                    mac_address: c.mac_address.clone(),
                                    labels: c.labels.clone(),
                                    ..Default::default()
                                }),
                                ip_address: self.get_container_ip(&id).await,
                            };

                            let mut configs = self.container_configs.lock().unwrap();
                            configs.insert(id.clone(), config);
                            println!("已更新容器 {} 的配置", id);
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ 容器 {} 重启失败: {}", id, e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_container_stop(
        &self,
        container_id: &str,
        restart_records: &mut HashMap<String, RestartRecord>,
    ) -> anyhow::Result<()> {
        println!("处理容器停止事件: {}", container_id);

        let now = SystemTime::now();
        let record = restart_records
            .entry(container_id.to_string())
            .or_insert(RestartRecord {
                last_restart: now,
                restart_count: 0,
            });

        if now.duration_since(record.last_restart)
            .unwrap_or(Duration::from_secs(0)) > Duration::from_secs(600)
        {
            record.restart_count = 0;
        }

        record.restart_count += 1;
        record.last_restart = now;

        println!(
            "尝试重启容器 {} (第 {} 次尝试)",
            container_id,
            record.restart_count
        );

        match self.docker.start_container::<String>(container_id, None).await {
            Ok(_) => {
                println!("✅ 容器 {} 重启成功", container_id);
                if let Ok(inspect) = self.docker.inspect_container(container_id, None).await {
                    let image = inspect.config.as_ref()
                        .and_then(|c| c.image.clone())
                        .unwrap_or_default();
                    
                    self.update_container_config(container_id, &inspect, image).await?;
                }
            }
            Err(e) => {
                eprintln!("❌ 容器 {} 重启失败: {}", container_id, e);
                return Err(anyhow::anyhow!("重启容器失败: {}", e));
            }
        }
        
        Ok(())
    }

    pub fn get_container_configs(&self) -> &Mutex<HashMap<String, ContainerConfig>> {
        &self.container_configs
    }

    pub async fn get_container_status(&self, container_id: &str) -> anyhow::Result<String> {
        let inspect = self.docker.inspect_container(container_id, None).await?;
        if let Some(state) = inspect.state {
            if let Some(status) = state.status {
                return Ok(status.to_string());
            }
        }
        Ok("unknown".to_string())
    }

    // 删除容器的方法
    pub async fn remove_container(&self, id: &str) -> anyhow::Result<()> {
        let docker = &self.docker;
        // 停止容器
        let _ = docker.stop_container(id, None).await;
        
        // 删除容器
        docker.remove_container(
            id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ).await?;
        
        // 从配置中移除容器
        let mut configs = self.get_container_configs().lock().unwrap();
        configs.remove(id);
        
        Ok(())
    }

    // 创建容器的方法
    pub async fn create_container(
        &self,
        name: &str,
        image: &str,
        ports: &[String],
        mounts: &[String],
        env: &[String],
    ) -> anyhow::Result<()> {
        let docker = &self.docker;
        
        // 确保镜像名称包含标签
        let image = if !image.contains(":") {
            format!("{}:latest", image)
        } else {
            image.to_string()
        };
        
        println!("开始拉取镜像: {}", image);
        let pull_opts = bollard::image::CreateImageOptions::<String> {
            from_image: image.clone(),
            ..Default::default()
        };
        
        println!("开始下载镜像层...");
        let mut stream = docker.create_image(Some(pull_opts), None, None);
        let mut last_progress = std::collections::HashMap::new();
        
        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    match (info.id, info.status) {
                        (Some(id), Some(status)) => {
                            let progress = info.progress.as_ref().map(|p| p.as_str()).unwrap_or_default();
                            let current_progress = format!("{} - {}", status, progress);
                            if last_progress.get(&id) != Some(&current_progress) {
                                if status.contains("Pull complete") {
                                    println!("✅ 层 [{}] 下载完成", id);
                                } else if status.contains("Downloading") {
                                    println!("⏳ 层 [{}] {}", id, progress);
                                } else if status.contains("Extracting") {
                                    println!("📦 层 [{}] 正在解压 {}", id, progress);
                                } else {
                                    println!("层 [{}] {}", id, status);
                                }
                                last_progress.insert(id, current_progress);
                            }
                        }
                        (None, Some(status)) => {
                            println!("状态更新: {}", status);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    eprintln!("❌ 拉取镜像出错: {}", e);
                    return Err(anyhow::anyhow!("拉取镜像失败: {}", e));
                }
            }
        }

        println!("✨ 镜像拉取完成，开始创建容器");

        // 创建端口绑定配置
        let mut port_bindings = HashMap::new();
        let mut exposed_ports = HashMap::new();
        for port_mapping in ports {
            if let Some((host_port, container_port)) = port_mapping.split_once(':') {
                let container_port_key = format!("{}/tcp", container_port);
                let host_binding = vec![bollard::models::PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(host_port.to_string()),
                }];
                port_bindings.insert(container_port_key.clone(), Some(host_binding));
                exposed_ports.insert(container_port_key, HashMap::new());
            }
        }

        println!("端口映射: {:?}", port_bindings);
        println!("暴露端口: {:?}", exposed_ports);

        // 创建挂载点配置
        let binds = Some(mounts.to_vec());
        println!("挂载点: {:?}", binds);

        // 创建容器配置
        let config = bollard::container::Config {
            image: Some(image.to_string()),
            env: Some(env.to_vec()),
            exposed_ports: Some(exposed_ports),
            host_config: Some(bollard::models::HostConfig {
                port_bindings: Some(port_bindings),
                binds,
                ..Default::default()
            }),
            ..Default::default()
        };

        // 创建容器
        println!("正在创建容器: {}", name);
        let container = match docker.create_container(
            Some(bollard::container::CreateContainerOptions {
                name,
                ..Default::default()
            }),
            config.clone(),
        ).await {
            Ok(container) => container,
            Err(e) => {
                eprintln!("创建容器失败: {}", e);
                return Err(anyhow::anyhow!("创建容器失败: {}", e));
            }
        };

        // 启动容器
        println!("正在启动容器: {}", container.id);
        if let Err(e) = docker.start_container::<String>(&container.id, None).await {
            eprintln!("启动容器失败: {}", e);
            return Err(anyhow::anyhow!("启动容器失败: {}", e));
        }

        // 获取容器详细信息并保存配置
        if let Ok(inspect) = docker.inspect_container(&container.id, None).await {
            let config = ContainerConfig {
                container_id: container.id.clone(),
                name: name.to_string(),
                image: image.to_string(),
                host_config: inspect.host_config,
                config: Some(config),
                ip_address: self.get_container_ip(&container.id).await,
            };

            let mut configs = self.container_configs.lock().unwrap();
            configs.insert(container.id, config);
            println!("容器配置已保存");
        }

        println!("✅ 容器创建完成");
        Ok(())
    }

    // 更新容器的方法
    pub async fn update_container(
        &self,
        id: &str,
        name: &str,
        image: &str,
        ports: &[String],
        mounts: &[String],
        env: &[String],
    ) -> anyhow::Result<()> {
        // 先停止并删除旧容器
        self.remove_container(id).await?;
        // 创建新容器
        self.create_container(name, image, ports, mounts, env).await?;
        Ok(())
    }

    async fn get_container_ip(&self, container_id: &str) -> Option<String> {
        if let Ok(inspect) = self.docker.inspect_container(container_id, None).await {
            if let Some(network_settings) = inspect.network_settings {
                if let Some(networks) = network_settings.networks {
                    // 优先获取 bridge 网络的 IP
                    if let Some(bridge) = networks.get("bridge") {
                        return bridge.ip_address.clone();
                    }
                    // 如果没有 bridge 网络，返回第一个找到的 IP
                    for network in networks.values() {
                        if let Some(ip) = &network.ip_address {
                            if !ip.is_empty() {
                                return Some(ip.clone());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    async fn update_container_config(&self, container_id: &str, inspect: &bollard::models::ContainerInspectResponse, image: String) -> anyhow::Result<()> {
        let ip_address = self.get_container_ip(container_id).await;
        let image_clone = image.clone(); // 克隆 image 以在多处使用
        
        let config = ContainerConfig {
            container_id: container_id.to_string(),
            name: inspect.name.as_ref().unwrap_or(&String::new()).trim_start_matches('/').to_string(),
            image,
            host_config: inspect.host_config.clone(),
            config: inspect.config.as_ref().map(|c| bollard::container::Config {
                hostname: c.hostname.clone(),
                domainname: c.domainname.clone(),
                user: c.user.clone(),
                attach_stdin: c.attach_stdin,
                attach_stdout: c.attach_stdout,
                attach_stderr: c.attach_stderr,
                exposed_ports: c.exposed_ports.clone(),
                tty: c.tty,
                open_stdin: c.open_stdin,
                stdin_once: c.stdin_once,
                env: c.env.clone(),
                cmd: c.cmd.clone(),
                image: Some(image_clone), // 使用克隆的 image
                volumes: c.volumes.clone(),
                working_dir: c.working_dir.clone(),
                entrypoint: c.entrypoint.clone(),
                network_disabled: c.network_disabled,
                mac_address: c.mac_address.clone(),
                labels: c.labels.clone(),
                ..Default::default()
            }),
            ip_address,
        };

        let mut configs = self.container_configs.lock().unwrap();
        configs.insert(container_id.to_string(), config);
        println!("已更新容器 {} 的配置", container_id);
        
        Ok(())
    }
}

#[derive(Clone)]
struct RestartRecord {
    last_restart: SystemTime,
    restart_count: u32,
} 