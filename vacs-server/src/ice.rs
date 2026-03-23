use crate::ice::provider::IceConfigProvider;
use crate::ice::provider::cloudflare::CloudflareIceProvider;
use crate::ice::provider::coturn::{CoturnIceProvider, CoturnIceProviderConfig};
use crate::ice::provider::stun::StunOnlyProvider;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

pub mod provider;

#[derive(Debug, thiserror::Error)]
pub enum IceError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("ICE server provider error: {0}")]
    Provider(String),
    #[error("Timeout getting ICE config: {0}")]
    Timeout(String),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum IceConfigProviderType {
    #[default]
    StunOnly,
    Cloudflare,
    Coturn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceConfig {
    pub provider: IceConfigProviderType,
    pub stun_servers: Option<Vec<String>>,
    pub cloudflare_turn_key_id: Option<String>,
    pub cloudflare_turn_key_api_token: Option<String>,
    pub turn_credential_ttl: Option<Duration>,
    pub coturn: Option<CoturnIceProviderConfig>,
}

impl Default for IceConfig {
    fn default() -> Self {
        Self {
            provider: IceConfigProviderType::StunOnly,
            stun_servers: Some(vec![
                "stun:stun.cloudflare.com:3478".to_string(),
                "stun:stun.cloudflare.com:53".to_string(),
            ]),
            cloudflare_turn_key_api_token: None,
            cloudflare_turn_key_id: None,
            turn_credential_ttl: Some(Self::DEFAULT_TURN_CREDENTIAL_TTL),
            coturn: None,
        }
    }
}

impl IceConfig {
    const DEFAULT_TURN_CREDENTIAL_TTL: Duration = Duration::from_hours(6);

    pub fn create_provider(&self) -> Result<Arc<dyn IceConfigProvider>, IceError> {
        match self.provider {
            IceConfigProviderType::StunOnly => {
                if let Some(stun_servers) = self.stun_servers.clone() {
                    Ok(Arc::new(StunOnlyProvider::new(stun_servers)))
                } else {
                    Err(IceError::Config("Missing STUN servers".to_string()))
                }
            }
            IceConfigProviderType::Cloudflare => {
                match (
                    &self.cloudflare_turn_key_id,
                    &self.cloudflare_turn_key_api_token,
                ) {
                    (Some(turn_key_id), Some(turn_key_api_token)) => {
                        Ok(Arc::new(CloudflareIceProvider::new(
                            turn_key_id,
                            turn_key_api_token,
                            self.turn_credential_ttl
                                .unwrap_or(Self::DEFAULT_TURN_CREDENTIAL_TTL)
                                .as_secs(),
                        )?))
                    }
                    _ => Err(IceError::Config(
                        "Missing Cloudflare credentials".to_string(),
                    )),
                }
            }
            IceConfigProviderType::Coturn => {
                if let Some(coturn_config) = self.coturn.clone() {
                    Ok(Arc::new(CoturnIceProvider::new(coturn_config)))
                } else {
                    Err(IceError::Config("Missing Coturn configuration".to_string()))
                }
            }
        }
    }
}
