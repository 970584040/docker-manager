use bollard::Docker;
use bollard::container::{CreateContainerOptions, Config, StartContainerOptions, KillContainerOptions};
use bollard::service::ContainerInspectResponse;
use crate::models::ContainerConfig;
use std::collections::HashMap;
use std::sync::Mutex;
use anyhow::Result;
use tokio::time::timeout;
use std::time::Duration;
use std::sync::Arc;
use std::time::SystemTime;
use crate::docker::DockerClient;

pub struct ContainerRestarter {
    docker: Docker,
    pub container_configs: Mutex<HashMap<String, ContainerConfig>>,
}

impl ContainerRestarter {
    pub async fn new(docker: Docker) -> Result<Self> {
        Ok(Self {
            docker,
            container_configs: Mutex::new(HashMap::new())
        })
    }

    pub async fn save_container_config(&self, inspect: ContainerInspectResponse) -> Result<()> {
        let container_id = inspect.id.clone().unwrap_or_default();
        let name = inspect.name.clone().unwrap_or_default();
        
        // 检查容器是否已经存在
        {
            let configs = self.container_configs.lock().unwrap();
            if configs.contains_key(&container_id) {
                return Ok(());
            }
        }

        let image = inspect.config.as_ref().and_then(|c| c.image.clone()).unwrap_or_default();
        
        // 转换 ContainerConfig 类型
        let config = if let Some(conf) = inspect.config {
            Some(Config {
                hostname: conf.hostname,
                domainname: conf.domainname,
                user: conf.user,
                attach_stdin: conf.attach_stdin,
                attach_stdout: conf.attach_stdout,
                attach_stderr: conf.attach_stderr,
                exposed_ports: conf.exposed_ports,
                tty: conf.tty,
                open_stdin: conf.open_stdin,
                stdin_once: conf.stdin_once,
                env: conf.env,
                cmd: conf.cmd,
                image: Some(image.clone()),
                volumes: conf.volumes,
                working_dir: conf.working_dir,
                entrypoint: conf.entrypoint,
                network_disabled: conf.network_disabled,
                mac_address: conf.mac_address,
                labels: conf.labels,
                ..Default::default()
            })
        } else {
            None
        };

        let config = ContainerConfig {
            container_id: container_id.clone(),
            name,
            image,
            config,
            host_config: inspect.host_config,
            ip_address: None,
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
        let docker = &self.docker;
        
        // 获取容器配置
        if let Ok(inspect) = docker.inspect_container(container_id, None).await {
            let name = inspect.name.unwrap_or_default().trim_start_matches('/').to_string();
            let image = inspect.config.as_ref()
                .and_then(|c| c.image.clone())
                .unwrap_or_default();

            let _config = ContainerConfig {
                container_id: container_id.to_string(),
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
                ip_address: None,
            };
        }
        Ok(())
    }

    pub async fn async_restart_container(self: Arc<Self>, container_id: String) {
        let restarter = self;
        tokio::spawn(async move {
            match restarter.restart_container(&container_id).await {
                Ok(_) => println!("容器 {} 重启完成", container_id),
                Err(e) => eprintln!("重启容器 {} 失败: {}", container_id, e),
            }
        });
    }
} 