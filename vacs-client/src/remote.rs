pub mod commands;
pub mod protocol;
pub mod server;

use serde::{Deserialize, Serialize};
pub use server::{RemoteServer, RemoteServerHandle};
use std::net::SocketAddr;

/// Configuration for the embedded remote-control server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Whether the remote control server is enabled.
    pub enabled: bool,
    /// Socket address to bind the remote control server to.
    /// Must be in the format "IP:PORT", e.g. "0.0.0.0:9600".
    pub listen_addr: SocketAddr,
    /// Whether to serve the web frontend. When disabled, only the WebSocket
    /// API is exposed (useful for custom or third-party clients).
    pub serve_frontend: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_addr: "0.0.0.0:9600".parse().unwrap(),
            serve_frontend: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendRemoteConfig {
    pub enabled: bool,
    pub listen_addr: SocketAddr,
    pub serve_frontend: bool,
}

impl Default for FrontendRemoteConfig {
    fn default() -> Self {
        Self::from(RemoteConfig::default())
    }
}

impl From<RemoteConfig> for FrontendRemoteConfig {
    fn from(config: RemoteConfig) -> Self {
        Self {
            enabled: config.enabled,
            listen_addr: config.listen_addr,
            serve_frontend: config.serve_frontend,
        }
    }
}

impl From<FrontendRemoteConfig> for RemoteConfig {
    fn from(config: FrontendRemoteConfig) -> Self {
        Self {
            enabled: config.enabled,
            listen_addr: config.listen_addr,
            serve_frontend: config.serve_frontend,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub listening: bool,
    pub connected_clients: usize,
}
