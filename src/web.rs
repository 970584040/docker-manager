use axum::{
    Router,
    routing::get,
    extract::State,
    response::{Json, Html},
    http::StatusCode,
};
use serde::Serialize;
use std::sync::Arc;
use crate::monitor::ContainerMonitor;

#[derive(Serialize)]
struct ContainerInfo {
    id: String,
    name: String,
    image: String,
    status: String,
    ports: Vec<String>,
    mounts: Vec<String>,
    env: Vec<String>,
}

// 添加首页处理函数
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

pub async fn start_web_server(monitor: Arc<ContainerMonitor>) {
    // 创建静态文件服务
    let app = Router::new()
        .route("/", get(index))
        .route("/api/containers", get(list_containers))
        .route("/api/container/:id", get(get_container))
        .with_state(monitor);

    // 尝试不同的端口
    for port in 3000..3010 {
        let addr = format!("127.0.0.1:{}", port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                println!("Web 服务器启动在 http://{}", addr);
                axum::serve(listener, app).await.unwrap();
                break;
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    println!("端口 {} 已被占用，尝试下一个端口", port);
                    continue;
                }
                panic!("启动服务器失败: {}", e);
            }
        }
    }
}

#[axum::debug_handler]
async fn list_containers(
    State(monitor): State<Arc<ContainerMonitor>>,
) -> Result<Json<Vec<ContainerInfo>>, StatusCode> {
    // 收集所有需要的数据，然后释放锁
    let container_data: Vec<_> = {
        let configs = monitor.get_container_configs().lock().unwrap();
        configs.values().map(|config| {
            (
                config.container_id.clone(),
                config.name.clone(),
                config.image.clone(),
                config.host_config.clone(),
                config.config.clone(),
            )
        }).collect()
    };

    let mut containers = Vec::new();
    
    // 使用收集的数据处理每个容器
    for (id, name, image, host_config, container_config) in container_data {
        let mut ports = Vec::new();
        let mut mounts = Vec::new();
        let mut env = Vec::new();

        // 处理环境变量
        if let Some(config) = container_config {
            if let Some(env_vars) = config.env {
                env = env_vars;
            }
        }

        if let Some(host_config) = host_config {
            if let Some(port_bindings) = &host_config.port_bindings {
                for (container_port, host_ports) in port_bindings {
                    if let Some(host_ports) = host_ports {
                        for host_port in host_ports {
                            ports.push(format!("{}:{} -> {}",
                                host_port.host_ip.as_deref().unwrap_or("0.0.0.0"),
                                host_port.host_port.as_deref().unwrap_or(""),
                                container_port
                            ));
                        }
                    }
                }
            }

            if let Some(binds) = &host_config.binds {
                mounts.extend(binds.iter().cloned());
            }
            
            if let Some(mounts_vec) = &host_config.mounts {
                for mount in mounts_vec {
                    if let (Some(source), Some(target)) = (&mount.source, &mount.target) {
                        mounts.push(format!("{} -> {}", source, target));
                    }
                }
            }
        }

        let status = monitor.get_container_status(&id).await
            .unwrap_or_else(|_| "unknown".to_string());

        containers.push(ContainerInfo {
            id,
            name,
            image,
            status,
            ports,
            mounts,
            env,
        });
    }

    Ok(Json(containers))
}

#[axum::debug_handler]
async fn get_container(
    State(monitor): State<Arc<ContainerMonitor>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ContainerInfo>, StatusCode> {
    let (container_id, name, image, host_config, container_config) = {
        let configs = monitor.get_container_configs().lock().unwrap();
        let config = configs.get(&id).ok_or(StatusCode::NOT_FOUND)?;
        (
            config.container_id.clone(),
            config.name.clone(),
            config.image.clone(),
            config.host_config.clone(),
            config.config.clone(),
        )
    };

    let mut ports = Vec::new();
    let mut mounts = Vec::new();
    let mut env = Vec::new();

    // 处理环境变量
    if let Some(config) = container_config {
        if let Some(env_vars) = config.env {
            env = env_vars;
        }
    }

    if let Some(host_config) = host_config {
        if let Some(port_bindings) = &host_config.port_bindings {
            for (container_port, host_ports) in port_bindings {
                if let Some(host_ports) = host_ports {
                    for host_port in host_ports {
                        ports.push(format!("{}:{} -> {}",
                            host_port.host_ip.as_deref().unwrap_or("0.0.0.0"),
                            host_port.host_port.as_deref().unwrap_or(""),
                            container_port
                        ));
                    }
                }
            }
        }

        if let Some(mounts_vec) = &host_config.mounts {
            for mount in mounts_vec {
                mounts.push(format!("{} -> {}",
                    mount.source.as_deref().unwrap_or(""),
                    mount.target.as_deref().unwrap_or("")
                ));
            }
        }
    }

    let status = monitor.get_container_status(&container_id).await
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(Json(ContainerInfo {
        id: container_id,
        name,
        image,
        status,
        ports,
        mounts,
        env,
    }))
} 