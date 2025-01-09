use bollard::models::HostConfig;
use bollard::container::Config;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub container_id: String,
    pub name: String,
    pub image: String,
    pub host_config: Option<HostConfig>,
    pub config: Option<Config<String>>,
    pub ip_address: Option<String>,
}

impl ContainerConfig {
    pub fn new(
        container_id: String,
        name: String,
        image: String,
        host_config: Option<HostConfig>,
        config: Option<Config<String>>,
    ) -> Self {
        Self {
            container_id,
            name,
            image,
            host_config,
            config,
            ip_address: None,
        }
    }
} 