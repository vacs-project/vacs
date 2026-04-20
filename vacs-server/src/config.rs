use crate::ice::IceConfig;
use crate::ratelimit::RateLimitersConfig;
use crate::release::catalog::CatalogConfig;
use anyhow::Context;
use axum_client_ip::ClientIpSource;
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub const BROADCAST_CHANNEL_CAPACITY: usize = 100;
pub const CLIENT_CHANNEL_CAPACITY: usize = 100;
pub const CLIENT_WEBSOCKET_TASK_CHANNEL_CAPACITY: usize = 100;
pub const CLIENT_WEBSOCKET_PING_INTERVAL: Duration = Duration::from_secs(10);
pub const CLIENT_WEBSOCKET_PONG_TIMEOUT: Duration = Duration::from_secs(30);
pub const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
/// After connecting, a client's position is frozen for this duration to allow the
/// VATSIM datafeed to catch up with the slurper-derived position assignment.
pub const POSITION_GRACE_PERIOD: Duration = Duration::from_secs(90);

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub redis: RedisConfig,
    pub session: SessionConfig,
    pub auth: AuthConfig,
    pub vatsim: VatsimConfig,
    pub updates: UpdatesConfig,
    pub rate_limiters: RateLimitersConfig,
    pub ice: IceConfig,
    pub admin: AdminConfig,
}

impl AppConfig {
    pub fn parse() -> anyhow::Result<Self> {
        let config = Config::builder()
            .add_source(Config::try_from(&AppConfig::default())?)
            .add_source(File::with_name(config_file_path("config.toml")?.as_str()).required(false))
            .add_source(File::with_name("config.toml").required(false))
            .add_source(
                Environment::with_prefix("vacs")
                    .separator("-")
                    .try_parsing(true),
            )
            .build()
            .context("Failed to build config")?
            .try_deserialize::<Self>()
            .context("Failed to deserialize config")?;

        if config.auth.oauth.client_id.is_empty() {
            anyhow::bail!("OAuth client ID is empty");
        } else if config.auth.oauth.client_secret.is_empty() {
            anyhow::bail!("OAuth client secret is empty");
        } else if config.session.signing_key.is_empty() {
            anyhow::bail!("Session signing key is empty");
        }

        url::Url::parse(&config.auth.oauth.deep_link_url).context("Invalid OAuth deep link URL")?;

        Ok(config)
    }
}

pub fn config_file_path(file_name: impl AsRef<Path>) -> anyhow::Result<String> {
    Ok(Path::new("/etc")
        .join(env!("CARGO_PKG_NAME").to_lowercase())
        .join(file_name)
        .to_str()
        .context("Failed to build config file path")?
        .to_string())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub metrics_bind_addr: String,
    pub client_ip_source: ClientIpSource,
    #[serde(default)]
    pub debug_endpoints: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:3000".to_string(),
            metrics_bind_addr: "0.0.0.0:9200".to_string(),
            client_ip_source: ClientIpSource::ConnectInfo,
            debug_endpoints: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RedisConfig {
    pub addr: String,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            addr: "redis://127.0.0.1:6379".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionConfig {
    pub secure: bool,
    pub http_only: bool,
    pub expiry_secs: i64,
    pub signing_key: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            secure: true,
            http_only: true,
            expiry_secs: 604800, // 7 days
            signing_key: "".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    pub login_flow_timeout_millis: u64,
    pub oauth: OAuthConfig,
    pub api_token: ApiTokenConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            login_flow_timeout_millis: 10000,
            oauth: OAuthConfig::default(),
            api_token: ApiTokenConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiTokenConfig {
    pub expiry_secs: u64,
}

impl Default for ApiTokenConfig {
    fn default() -> Self {
        Self {
            expiry_secs: 86400, // 24 hours
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OAuthConfig {
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    pub deep_link_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            auth_url: "https://auth-dev.vatsim.net/oauth/authorize".to_string(),
            token_url: "https://auth-dev.vatsim.net/oauth/token".to_string(),
            redirect_url: "http://localhost:3000/auth/vatsim/redirect".to_string(),
            deep_link_url: "vacs://auth/vatsim/callback".to_string(),
            client_id: "".to_string(),
            client_secret: "".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VatsimConfig {
    pub user_service: VatsimUserServiceConfig,
    pub require_active_connection: bool,
    pub slurper_base_url: String,
    pub data_feed_url: String,
    pub data_feed_timeout: Duration,
    pub controller_update_interval: Duration,
    /// Path to the dataset coverage directory. Must be a **subdirectory** of
    /// the volume mount - not the volume root itself - so that the dataset
    /// manager can create temporary and backup directories as siblings on the
    /// same filesystem for atomic renames.
    ///
    /// In production this should live on a named Docker volume
    /// (`/var/lib/vacs-server/data`), separate from the config bind mount.
    pub coverage_dir: String,
}

impl Default for VatsimConfig {
    fn default() -> Self {
        Self {
            user_service: Default::default(),
            require_active_connection: true,
            slurper_base_url: "https://slurper.vatsim.net".to_string(),
            data_feed_url: "https://data.vatsim.net/v3/vatsim-data.json".to_string(),
            data_feed_timeout: Duration::from_secs(2),
            controller_update_interval: Duration::from_secs(30),
            coverage_dir: "/var/lib/vacs-server/data/coverage".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VatsimUserServiceConfig {
    pub user_details_endpoint_url: String,
}

impl Default for VatsimUserServiceConfig {
    fn default() -> Self {
        Self {
            user_details_endpoint_url: "https://auth-dev.vatsim.net/api/user".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdatesConfig {
    pub policy_path: String,
    pub catalog: CatalogConfig,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            policy_path: config_file_path("release_policy.toml")
                .expect("Failed to build policy path"),
            catalog: CatalogConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AdminConfig {
    /// Expected audience for GitHub OIDC tokens.
    pub oidc_audience: String,
    /// Allowed subject claim for GitHub OIDC tokens.
    /// With GitHub Environments, the format is:
    /// `repo:<owner>/<repo>:environment:<environment_name>`
    /// e.g. `repo:vacs-project/vacs-data:environment:production`
    pub oidc_allowed_sub: String,
    /// Configuration for the dataset repository. If omitted, the server
    /// will only load the dataset from the local `coverage_dir` on disk
    /// and the admin reload endpoint will be unavailable.
    #[serde(default)]
    pub dataset: Option<DatasetRepoConfig>,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            oidc_audience: "https://vacs.network".to_string(),
            oidc_allowed_sub: String::new(),
            dataset: None,
        }
    }
}

/// Credentials for authenticating as a GitHub App.
///
/// Shared between the release catalog and the dataset manager.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GitHubCredentials {
    /// GitHub App ID.
    pub app_id: u64,
    /// GitHub App private key (PEM-encoded).
    pub app_private_key: String,
    /// GitHub App installation ID.
    pub installation_id: u64,
}

impl std::fmt::Debug for GitHubCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubCredentials")
            .field("app_id", &self.app_id)
            .field("installation_id", &self.installation_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DatasetRepoConfig {
    /// GitHub repository owner (e.g. `vacs-project`).
    pub owner: String,
    /// GitHub repository name (e.g. `vacs-data`).
    pub repo: String,
    /// GitHub App credentials for authenticated API access.
    /// If omitted, the client falls back to unauthenticated requests
    /// (only works for public repositories).
    #[serde(flatten)]
    pub credentials: Option<GitHubCredentials>,
    /// Lightweight tag that the GHA workflow force-pushes after every
    /// successful deploy (e.g. `deployed/production`). The server resolves
    /// this tag to a commit SHA on startup to decide whether the local
    /// dataset is up-to-date.
    pub deployed_tag: String,
}

impl Default for DatasetRepoConfig {
    fn default() -> Self {
        Self {
            owner: "vacs-project".to_string(),
            repo: "vacs-data".to_string(),
            credentials: None,
            deployed_tag: "deployed/production".to_string(),
        }
    }
}
