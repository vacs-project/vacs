use crate::app::{ClientConfig, ClientPageSettings};
use crate::audio::AudioConfig;
use anyhow::Context;
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;
use vacs_signaling::protocol::http::webrtc::IceConfig;
use vacs_signaling::protocol::vatsim::PositionId;

/// User-Agent string used for all HTTP requests.
pub static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
#[cfg(not(feature = "e2e"))]
pub const DEFAULT_SETTINGS_FILE_NAME: &str = "config.toml";
pub const AUDIO_SETTINGS_FILE_NAME: &str = "audio.toml";
pub const CLIENT_SETTINGS_FILE_NAME: &str = "client.toml";
#[cfg(not(feature = "e2e"))]
pub const CLIENT_PAGE_SETTINGS_FILE_NAME: &str = "client_page.toml";

/// The client configuration.
///
/// It is read from `config.toml`, `audio.toml`, `client_page.toml` and `client.toml` in that
/// order, and any file may hold any part of it. vacs itself writes `[audio]` to `audio.toml` and
/// `[client]` to `client.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct AppConfig {
    pub backend: BackendConfig,
    pub audio: AudioConfig,
    #[serde(alias = "webrtc")] // support for old naming scheme
    pub ice: IceConfig,
    pub client: ClientConfig,
    #[serde(default)]
    pub client_page: ClientPageSettings,
}

impl AppConfig {
    #[cfg_attr(feature = "e2e", allow(unused_variables))]
    pub fn parse(config_dir: &Path) -> anyhow::Result<Self> {
        #[cfg_attr(feature = "e2e", allow(unused_mut))]
        let mut builder = Config::builder().add_source(Config::try_from(&AppConfig::default())?);

        // In E2E mode, skip all config files and rely on compile-time defaults
        // plus environment variable overrides only.
        #[cfg(not(feature = "e2e"))]
        {
            builder = builder
                .add_source(
                    File::with_name(
                        config_dir
                            .join(DEFAULT_SETTINGS_FILE_NAME)
                            .to_str()
                            .expect("Failed to get local config path"),
                    )
                    .required(false),
                )
                .add_source(File::with_name(DEFAULT_SETTINGS_FILE_NAME).required(false))
                .add_source(
                    File::with_name(
                        config_dir
                            .join(AUDIO_SETTINGS_FILE_NAME)
                            .to_str()
                            .expect("Failed to get local config path"),
                    )
                    .required(false),
                )
                .add_source(File::with_name(AUDIO_SETTINGS_FILE_NAME).required(false))
                .add_source(
                    File::with_name(
                        config_dir
                            .join(CLIENT_PAGE_SETTINGS_FILE_NAME)
                            .to_str()
                            .expect("Failed to get local config path"),
                    )
                    .required(false),
                )
                .add_source(File::with_name(CLIENT_PAGE_SETTINGS_FILE_NAME).required(false))
                .add_source(
                    File::with_name(
                        config_dir
                            .join(CLIENT_SETTINGS_FILE_NAME)
                            .to_str()
                            .expect("Failed to get local config path"),
                    )
                    .required(false),
                )
                .add_source(File::with_name(CLIENT_SETTINGS_FILE_NAME).required(false));
        }

        let mut builder = builder.add_source(Environment::with_prefix("vacs_client"));

        let preliminary_config: AppConfig = builder
            .build_cloned()
            .context("Failed to build preliminary config")?
            .try_deserialize()
            .context("Failed to deserialize preliminary config")?;

        if let Some(extra_client_page_config) = preliminary_config.client.extra_client_page_config {
            log::info!("Loading extra client page config from {extra_client_page_config}");
            builder = builder
                .add_source(File::with_name(&extra_client_page_config).required(false))
                .add_source(Environment::with_prefix("vacs_client"));
        }

        let mut config: AppConfig = builder
            .build()
            .context("Failed to build config")?
            .try_deserialize()
            .context("Failed to deserialize config")?;

        // Migrate old transmit config to new radio config, if mode was not RadioIntegration
        if let Some(was_radio_integration) = config.client.transmit_config.was_radio_integration
            && !was_radio_integration
        {
            config.client.radio.integration = None;
        }

        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct BackendConfig {
    /// Base URL of the vacs server. Defaults to `https://vacs.network`.
    pub base_url: String,
    /// WebSocket URL of the vacs server. Defaults to `wss://vacs.network/ws`.
    pub ws_url: String,
    /// HTTP endpoint paths, relative to `base_url`.
    pub endpoints: BackendEndpointsConfigs,
    /// Timeout for HTTP requests to the server, in milliseconds.
    pub timeout_ms: u64,
    /// Position to connect as. Only honored by development and release candidate builds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_position_id: Option<PositionId>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            base_url: if cfg!(feature = "e2e") {
                "http://127.0.0.1:4568"
            } else if cfg!(debug_assertions) || cfg!(feature = "rc") {
                "https://dev.vacs.network"
            } else {
                "https://vacs.network"
            }
            .to_string(),
            ws_url: if cfg!(feature = "e2e") {
                "ws://127.0.0.1:4568/ws"
            } else if cfg!(debug_assertions) || cfg!(feature = "rc") {
                "wss://dev.vacs.network/ws"
            } else {
                "wss://vacs.network/ws"
            }
            .to_string(),
            endpoints: BackendEndpointsConfigs::default(),
            timeout_ms: 2000,
            dev_position_id: None,
        }
    }
}

impl BackendConfig {
    pub fn endpoint_url(&self, endpoint: &BackendEndpoint) -> String {
        let path = match endpoint {
            BackendEndpoint::InitAuth => &self.endpoints.init_auth,
            BackendEndpoint::ExchangeCode => &self.endpoints.exchange_code,
            BackendEndpoint::UserInfo => &self.endpoints.user_info,
            BackendEndpoint::Logout => &self.endpoints.logout,
            BackendEndpoint::WsToken => &self.endpoints.ws_token,
            BackendEndpoint::TerminateWsSession => &self.endpoints.terminate_ws_session,
            BackendEndpoint::VersionUpdateCheck => &self.endpoints.version_update_check,
            BackendEndpoint::IceConfig => &self.endpoints.ice_config,
        };
        format!("{}{}", self.base_url, path)
    }
}

pub enum BackendEndpoint {
    InitAuth,
    ExchangeCode,
    UserInfo,
    Logout,
    WsToken,
    TerminateWsSession,
    VersionUpdateCheck,
    IceConfig,
}

impl BackendEndpoint {
    pub const fn timeout(&self) -> Option<Duration> {
        match self {
            Self::ExchangeCode => Some(Duration::from_secs(2)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct BackendEndpointsConfigs {
    /// Starts the VATSIM Connect login.
    pub init_auth: String,
    /// Exchanges the VATSIM Connect authorization code for a session.
    pub exchange_code: String,
    /// Returns the logged in user.
    pub user_info: String,
    /// Ends the session.
    pub logout: String,
    /// Issues the token for the WebSocket connection.
    pub ws_token: String,
    /// Terminates the WebSocket session.
    pub terminate_ws_session: String,
    /// Checks for client updates. `{{...}}` placeholders are filled in by the client.
    pub version_update_check: String,
    /// Returns the STUN and TURN servers for calls.
    pub ice_config: String,
}

impl Default for BackendEndpointsConfigs {
    fn default() -> Self {
        Self {
            init_auth: "/auth/vatsim".to_string(),
            exchange_code: "/auth/vatsim/callback".to_string(),
            user_info: "/auth/user".to_string(),
            logout: "/auth/logout".to_string(),
            ws_token: "/ws/token".to_string(),
            terminate_ws_session: "/ws".to_string(),
            version_update_check: "/version/update?version={{current_version}}&target={{target}}&arch={{arch}}&bundle_type={{bundle_type}}&channel={{channel}}".to_string(),
            ice_config: "/webrtc/ice-config".to_string(),
        }
    }
}

pub trait Persistable {
    fn persist(&self, config_dir: &Path, file_name: &str) -> anyhow::Result<()>;
}

impl<T: Serialize> Persistable for T {
    fn persist(&self, config_dir: &Path, file_name: &str) -> anyhow::Result<()> {
        let serialized = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::create_dir_all(config_dir).context("Failed to create config directory")?;
        fs::write(config_dir.join(file_name), serialized)
            .context("Failed to write config to file")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::generate::SchemaSettings;
    use schemars::transform::RecursiveTransform;

    const SCHEMA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/config.schema.json");

    fn config_schema() -> String {
        // Each file holds only part of the config and the rest falls back to the defaults, so
        // nothing is required in any one file. Unknown keys are ignored when parsing, which
        // hides typos; the schema rejects them so an editor can point them out.
        let generator = SchemaSettings::draft07()
            .with_transform(RecursiveTransform(|schema: &mut schemars::Schema| {
                schema.remove("required");
                if schema.get("properties").is_some()
                    && schema.get("additionalProperties").is_none()
                {
                    schema.insert("additionalProperties".into(), false.into());
                }
            }))
            .into_generator();
        let schema = generator.into_root_schema_for::<AppConfig>();
        serde_json::to_string_pretty(&schema).expect("Failed to serialize config schema") + "\n"
    }

    #[test]
    fn config_schema_is_current() {
        let schema = config_schema();
        if std::env::var_os("VACS_UPDATE_CONFIG_SCHEMA").is_some() {
            fs::write(SCHEMA_PATH, &schema).expect("Failed to write config schema");
            return;
        }

        // Compared as JSON values, because the pre-commit prettier hook reformats the file.
        let committed: Option<serde_json::Value> = fs::read_to_string(SCHEMA_PATH)
            .ok()
            .and_then(|committed| serde_json::from_str(&committed).ok());
        let generated: serde_json::Value =
            serde_json::from_str(&schema).expect("Failed to parse generated config schema");
        assert!(
            committed.as_ref() == Some(&generated),
            "vacs-client/config.schema.json is out of date, regenerate it with \
             `VACS_UPDATE_CONFIG_SCHEMA=1 cargo test -p vacs-client config_schema_is_current`"
        );
    }
}
