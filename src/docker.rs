use bollard::Docker;
use bollard::API_DEFAULT_VERSION;
use std::path::Path;
use std::process::Command;
use anyhow::Result;
use lazy_static::lazy_static;
use tokio::sync::OnceCell;
use tokio::time::{Duration, sleep};

lazy_static! {
    static ref DOCKER: OnceCell<Docker> = OnceCell::new();
}

pub struct DockerClient;

impl DockerClient {
    fn get_docker_socket_path() -> String {
        if Path::new("/var/run/docker.sock").exists() {
            return "/var/run/docker.sock".to_string();
        }

        let output = Command::new("docker")
            .args(["context", "inspect"])
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
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

        "/var/run/docker.sock".to_string()
    }

    pub async fn get() -> Result<&'static Docker> {
        Ok(DOCKER.get_or_init(|| async {
            Self::connect_with_retry().await
                .unwrap_or_else(|e| panic!("Failed to connect to Docker after retries: {}", e))
        }).await)
    }

    async fn connect_with_retry() -> Result<Docker> {
        let max_retries = 3;
        let mut retry_count = 0;
        
        while retry_count < max_retries {
            let socket_path = Self::get_docker_socket_path();
            match Docker::connect_with_socket(&socket_path, 120, API_DEFAULT_VERSION) {
                Ok(docker) => {
                    // 验证连接是否真的可用
                    if docker.ping().await.is_ok() {
                        return Ok(docker);
                    }
                }
                Err(e) => {
                    eprintln!("Docker连接失败 (尝试 {}/{}): {}", retry_count + 1, max_retries, e);
                }
            }
            retry_count += 1;
            sleep(Duration::from_secs(1)).await;
        }
        
        Err(anyhow::anyhow!("无法连接到Docker服务"))
    }

    // 添加健康检查方法
    pub async fn check_health(docker: &Docker) -> bool {
        docker.ping().await.is_ok()
    }
} 