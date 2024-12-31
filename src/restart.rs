use bollard::Docker;
use bollard::container::{CreateContainerOptions, Config, StartContainerOptions, KillContainerOptions};
use bollard::service::ContainerInspectResponse;
use bollard::models::{HostConfig, ContainerConfig as BollardContainerConfig};
use std::collections::HashMap;
use std::sync::Mutex;
use anyhow::Result;
use tokio::time::timeout;
use std::time::Duration;
use std::sync::Arc;

#[derive(Clone)]
pub struct ContainerConfig {
    pub container_id: String,
    pub name: String,
    pub image: String,
    pub config: Option<BollardContainerConfig>,
    pub host_config: Option<HostConfig>,
}

pub struct ContainerRestarter {
    docker: Docker,
    pub container_configs: Arc<Mutex<HashMap<String, ContainerConfig>>>,
}

impl ContainerRestarter {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self {
            docker,
            container_configs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn save_container_config(&self, inspect: ContainerInspectResponse) -> Result<()> {
        let container_id = inspect.id.clone().unwrap_or_default();
        let name = inspect.name.clone().unwrap_or_default();
        
        // 检查容器是否已经存在
        {
            let configs = self.container_configs.lock().unwrap();
            if configs.contains_key(&container_id) {
                // 如果容器已存在，不需要重新保存配置
                return Ok(());
            }
        }

        let image = inspect.config.as_ref().and_then(|c| c.image.clone()).unwrap_or_default();
        
        let config = ContainerConfig {
            container_id: container_id.clone(),
            name,
            image,
            config: inspect.config,
            host_config: inspect.host_config,
        };

        let mut configs = self.container_configs.lock().unwrap();
        configs.insert(container_id, config);
        Ok(())
    }

    pub async fn get_container_status(&self, container_id: &str) -> Result<String> {
        let inspect = self.docker.inspect_container(container_id, None).await?;
        if let Some(state) = inspect.state {
            return Ok(match state.status {
                Some(status) => status.to_string(),
                None => "unknown".to_string()
            });
        }
        Ok("unknown".to_string())
    }

    pub async fn restart_container(&self, container_id: &str) -> Result<()> {
        // 在作用域内获取所需数据，然后释放锁
        let (container_name, container_config, old_config) = {
            let configs = self.container_configs.lock().unwrap();
            let config = configs.get(container_id)
                .ok_or_else(|| anyhow::anyhow!("容器配置未找到"))?;
            
            let image = config.image.clone();
            (
                config.name.trim_start_matches('/').to_string(),
                if let Some(conf) = &config.config {
                    Config {
                        hostname: conf.hostname.clone(),
                        domainname: conf.domainname.clone(),
                        user: conf.user.clone(),
                        attach_stdin: conf.attach_stdin,
                        attach_stdout: conf.attach_stdout,
                        attach_stderr: conf.attach_stderr,
                        exposed_ports: conf.exposed_ports.clone(),
                        tty: conf.tty,
                        open_stdin: conf.open_stdin,
                        stdin_once: conf.stdin_once,
                        env: conf.env.clone(),
                        cmd: conf.cmd.clone(),
                        image: Some(image.clone()),
                        volumes: conf.volumes.clone(),
                        working_dir: conf.working_dir.clone(),
                        entrypoint: conf.entrypoint.clone(),
                        network_disabled: conf.network_disabled,
                        mac_address: conf.mac_address.clone(),
                        labels: conf.labels.clone(),
                        host_config: config.host_config.clone(),
                        ..Default::default()
                    }
                } else {
                    Config {
                        image: Some(image),
                        host_config: config.host_config.clone(),
                        ..Default::default()
                    }
                },
                config.clone(),
            )
        };

        println!("正在重启容器: {}", container_name);

        // 停止容器
        println!("停止容器: {}", container_id);
        match timeout(Duration::from_secs(10), self.docker.stop_container(container_id, None)).await {
            Ok(result) => {
                if let Err(e) = result {
                    println!("停止容器出错: {}", e);
                }
            }
            Err(_) => {
                println!("停止容器超时，尝试强制停止");
                if let Err(e) = self.docker.kill_container(container_id, None::<KillContainerOptions<String>>).await {
                    println!("强制停止容器出错: {}", e);
                }
            }
        }

        // 等待容器完全停止
        println!("等待容器停止");
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 删除旧容器
        println!("删除容器: {}", container_id);
        if let Err(e) = self.docker.remove_container(container_id, None).await {
            println!("删除容器出错: {}", e);
            return Err(anyhow::anyhow!("删除容器失败: {}", e));
        }

        // 创建新容器
        println!("创建新容器: {}", container_name);
        let create_opts = CreateContainerOptions {
            name: container_name,
            ..Default::default()
        };

        let new_container = match self.docker.create_container(Some(create_opts), container_config).await {
            Ok(response) => response,
            Err(e) => {
                println!("创建容器失败: {}", e);
                return Err(anyhow::anyhow!("创建容器失败: {}", e));
            }
        };

        // 启动新容器
        println!("启动新容器: {}", new_container.id);
        if let Err(e) = self.docker.start_container(&new_container.id, None::<StartContainerOptions<String>>).await {
            println!("启动容器失败: {}", e);
            return Err(anyhow::anyhow!("启动容器失败: {}", e));
        }

        // 验证容器是否成功启动
        tokio::time::sleep(Duration::from_secs(2)).await;
        match self.docker.inspect_container(&new_container.id, None).await {
            Ok(inspect) => {
                if let Some(state) = inspect.state {
                    if let Some(running) = state.running {
                        if !running {
                            return Err(anyhow::anyhow!("容器启动后未处于运行状态"));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("无法验证容器状态: {}", e));
            }
        }

        println!("容器已成功重启: {}", new_container.id);
        
        // 更新容器配置时，移除旧配置并添加新配置
        {
            let mut configs = self.container_configs.lock().unwrap();
            // 移除旧配置
            configs.remove(container_id);
            // 添加新配置
            let new_config = ContainerConfig {
                container_id: new_container.id.clone(),
                name: old_config.name,
                image: old_config.image,
                config: old_config.config,
                host_config: old_config.host_config,
            };
            configs.insert(new_container.id.clone(), new_config);
        }

        Ok(())
    }

    pub fn async_restart_container(self: Arc<Self>, container_id: String) {
        let restarter = self;
        tokio::spawn(async move {
            match restarter.restart_container(&container_id).await {
                Ok(_) => println!("容器 {} 重启完成", container_id),
                Err(e) => eprintln!("重启容器 {} 失败: {}", container_id, e),
            }
        });
    }
} 