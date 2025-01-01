use bollard::Docker;
use bollard::API_DEFAULT_VERSION;
use bollard::models::EventMessageTypeEnum;
use bollard::system::EventsOptions;
use bollard::container::ListContainersOptions;
use futures::StreamExt;
use std::collections::HashMap;
use anyhow::Result;
use std::time::{Duration, SystemTime};
use crate::restart::{ContainerConfig, ContainerRestarter};
use std::sync::Arc;
use std::sync::Mutex;
use std::path::Path;
use std::process::Command;

pub struct ContainerMonitor {
    docker: Docker,
    restarter: Arc<ContainerRestarter>,
}

impl ContainerMonitor {
    fn get_docker_socket_path() -> String {
        // 首先检查默认路径
        if Path::new("/var/run/docker.sock").exists() {
            return "/var/run/docker.sock".to_string();
        }

        // 如果默认路径不存在，尝试获取 Docker context 中的路径
        let output = Command::new("docker")
            .args(["context", "inspect"])
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    // 解析输出找到 socket 路径
                    if stdout.contains("\"Host\":") {
                        if let Some(socket_path) = stdout
                            .lines()
                            .find(|line| line.contains("\"Host\":"))
                            .and_then(|line| line.split("unix://").nth(1))
                            .map(|path| path.trim_matches(|c| c == '"' || c == ',' || c == ' '))
                        {
                            return socket_path.to_string();
                        }
                    }
                }
            }
        }

        // 如果都失败了，返回默认路径
        "/var/run/docker.sock".to_string()
    }

    pub async fn new() -> Result<Self> {
        let socket_path = Self::get_docker_socket_path();
        let docker = Docker::connect_with_socket(&socket_path, 120, API_DEFAULT_VERSION)?;
        let restarter = Arc::new(ContainerRestarter::new().await?);
        let monitor = Self { docker, restarter };
        
        monitor.init_containers().await?;
        
        Ok(monitor)
    }

    // 添加初始化方法
    async fn init_containers(&self) -> Result<()> {
        let mut filters = HashMap::new();
        filters.insert("status", vec!["running", "created", "exited", "paused"]);
        
        let options = ListContainersOptions{
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        
        for container in containers {
            if let Some(id) = &container.id {
                if let Ok(inspect) = self.docker.inspect_container(id, None).await {
                    self.restarter.save_container_config(inspect).await?;
                }
            }
        }

        println!("已加载 {} 个容器配置", 
            self.restarter.container_configs.lock().unwrap().len());
        
        Ok(())
    }

    pub async fn start_monitoring(&self) -> Result<()> {
        let mut events = self.docker.events(None::<EventsOptions<String>>);
        let mut restart_records: HashMap<String, RestartRecord> = HashMap::new();

        println!("开始监控容器状态...");

        // 先检查现有的已停止容器
        self.check_stopped_containers().await?;

        while let Some(event) = events.next().await {
            match event {
                Ok(event) => {
                    if event.typ == Some(EventMessageTypeEnum::CONTAINER) {
                        if let Some(id) = event.actor.and_then(|a| a.id) {
                            println!("收到容器事件: {} - {:?}", id, event.action);
                            match event.action.as_deref() {
                                Some("die") | Some("stop") | Some("kill") | Some("exited") => {
                                    println!("检测到容器停止: {}", id);
                                    // 立即尝试重启
                                    if let Err(e) = self.handle_container_stop(&id, &mut restart_records).await {
                                        eprintln!("处理容器停止失败: {}", e);
                                    }
                                }
                                Some("start") => {
                                    println!("容器已启动: {}", id);
                                    restart_records.remove(&id);
                                    if let Ok(inspect) = self.docker.inspect_container(&id, None).await {
                                        self.restarter.save_container_config(inspect).await?;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => eprintln!("监控事件错误: {}", e),
            }
        }
        Ok(())
    }

    // 添加检查已停止容器的方法
    async fn check_stopped_containers(&self) -> Result<()> {
        let mut filters = HashMap::new();
        filters.insert("status", vec!["exited", "dead"]);
        
        let options = ListContainersOptions{
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;
        let mut restart_records = HashMap::new();
        
        for container in containers {
            if let Some(id) = container.id {
                println!("发现已停止的容器: {}", id);
                if let Err(e) = self.handle_container_stop(&id, &mut restart_records).await {
                    eprintln!("重启已停止的容器失败: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn handle_container_stop(
        &self,
        container_id: &str,
        restart_records: &mut HashMap<String, RestartRecord>,
    ) -> Result<()> {

        let now = SystemTime::now();
        let record = restart_records
            .entry(container_id.to_string())
            .or_insert(RestartRecord {
                last_restart: now,
                restart_count: 0,
            });

        // 检查重启间隔
        if now.duration_since(record.last_restart)
            .unwrap_or(Duration::from_secs(0)) > Duration::from_secs(600)
        {
            record.restart_count = 0;
        }

        record.restart_count += 1;
        record.last_restart = now;

        println!(
            "正在尝试重启容器 {} (第 {} 次尝试)",
            container_id,
            record.restart_count
        );

        // 执行重启
        self.restarter.clone().async_restart_container(container_id.to_string());
        
        Ok(())
    }

    pub fn get_container_configs(&self) -> &Mutex<HashMap<String, ContainerConfig>> {
        &self.restarter.container_configs
    }

    pub async fn get_container_status(&self, container_id: &str) -> Result<String> {
        self.restarter.get_container_status(container_id).await
    }
}

#[derive(Clone)]
struct RestartRecord {
    last_restart: SystemTime,
    restart_count: u32,
} 