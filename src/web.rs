use axum::{
    Router,
    routing::{get, post, put, delete},
    extract::State,
    response::{Json, Html},
    http::StatusCode,
};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use bollard::container::Config;
use bollard::models::HostConfig;
use crate::monitor::ContainerMonitor;

#[derive(Serialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub ports: Vec<String>,
    pub mounts: Vec<String>,
    pub env: Vec<String>,
    pub ip_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_config: Option<HostConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Config<String>>,
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
        .route("/api/container/:id", delete(delete_container))
        .route("/api/containers", post(create_container))
        .route("/api/container/:id", put(update_container))
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
    let container_data: Vec<_> = {
        let configs = monitor.get_container_configs().lock().unwrap();
        configs.values().cloned().collect()
    };

    let mut containers = Vec::new();
    for config in container_data {
        // 获取容器状态
        let status = monitor.get_container_status(&config.container_id).await
            .unwrap_or_else(|_| "unknown".to_string());

        // 从配置中提取端口映射
        let ports = config.host_config
            .as_ref()
            .and_then(|host_config| host_config.port_bindings.as_ref())
            .map(|bindings| {
                bindings.iter()
                    .map(|(container_port, host_bindings)| {
                        if let Some(bindings) = host_bindings {
                            if let Some(first_binding) = bindings.first() {
                                if let Some(host_port) = &first_binding.host_port {
                                    return format!("{}:{}", host_port, container_port.split('/').next().unwrap_or(""));
                                }
                            }
                        }
                        container_port.to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // 从配置中提取挂载点
        let mounts = config.host_config
            .as_ref()
            .and_then(|host_config| host_config.binds.as_ref())
            .map(|binds| binds.clone())
            .unwrap_or_default();

        // 从配置中提取环境变量
        let env = config.config
            .as_ref()
            .and_then(|config| config.env.clone())
            .unwrap_or_default();

        containers.push(ContainerInfo {
            id: config.container_id,
            name: config.name,
            image: config.image,
            status,
            ports,
            mounts,
            env,
            ip_address: config.ip_address,
            host_config: config.host_config,
            config: config.config,
        });
    }

    Ok(Json(containers))
}

#[axum::debug_handler]
async fn get_container(
    State(monitor): State<Arc<ContainerMonitor>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ContainerInfo>, StatusCode> {
    let config = {
        let configs = monitor.get_container_configs().lock().unwrap();
        configs.get(&id)
            .ok_or(StatusCode::NOT_FOUND)?
            .clone()
    };

    let status = monitor.get_container_status(&config.container_id).await
        .unwrap_or_else(|_| "unknown".to_string());

    // 从配置中提取端口映射
    let ports = config.host_config
        .as_ref()
        .and_then(|host_config| host_config.port_bindings.as_ref())
        .map(|bindings| {
            bindings.iter()
                .map(|(container_port, host_bindings)| {
                    if let Some(bindings) = host_bindings {
                        if let Some(first_binding) = bindings.first() {
                            if let Some(host_port) = &first_binding.host_port {
                                return format!("{}:{}", host_port, container_port.split('/').next().unwrap_or(""));
                            }
                        }
                    }
                    container_port.to_string()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // 从配置中提取挂载点
    let mounts = config.host_config
        .as_ref()
        .and_then(|host_config| host_config.binds.as_ref())
        .map(|binds| binds.clone())
        .unwrap_or_default();

    // 从配置中提取环境变量
    let env = config.config
        .as_ref()
        .and_then(|config| config.env.clone())
        .unwrap_or_default();

    Ok(Json(ContainerInfo {
        id: config.container_id,
        name: config.name,
        image: config.image,
        status,
        ports,
        mounts,
        env,
        ip_address: config.ip_address,
        host_config: config.host_config,
        config: config.config,
    }))
}

#[axum::debug_handler]
async fn delete_container(
    State(monitor): State<Arc<ContainerMonitor>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    monitor.as_ref().remove_container(&id).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct CreateContainerRequest {
    name: String,
    image: String,
    ports: Vec<String>,
    mounts: Vec<String>,
    env: Vec<String>,
}

#[axum::debug_handler]
async fn create_container(
    State(monitor): State<Arc<ContainerMonitor>>,
    Json(payload): Json<CreateContainerRequest>,
) -> Result<StatusCode, StatusCode> {
    monitor.as_ref().create_container(
        &payload.name,
        &payload.image,
        &payload.ports,
        &payload.mounts,
        &payload.env
    ).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

#[axum::debug_handler]
async fn update_container(
    State(monitor): State<Arc<ContainerMonitor>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(payload): Json<CreateContainerRequest>,
) -> Result<StatusCode, StatusCode> {
    monitor.as_ref().update_container(
        &id,
        &payload.name,
        &payload.image,
        &payload.ports,
        &payload.mounts,
        &payload.env
    ).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
} 