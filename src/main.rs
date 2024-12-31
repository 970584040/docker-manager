mod monitor;
mod restart;
mod web;

use anyhow::Result;
use std::sync::Arc;
use monitor::ContainerMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    let monitor = ContainerMonitor::new().await?;
    let monitor = Arc::new(monitor);
    
    // 启动 Web 服务
    let monitor_clone = monitor.clone();
    tokio::spawn(async move {
        web::start_web_server(monitor_clone).await;
    });

    // 启动容器监控
    monitor.start_monitoring().await?;

    Ok(())
} 