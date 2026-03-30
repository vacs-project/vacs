use crate::ice::IceError;
use crate::ice::provider::IceConfigProvider;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};
use std::time::UNIX_EPOCH;
use tracing::instrument;
use vacs_protocol::http::webrtc::{IceConfig, IceServer};
use vacs_protocol::vatsim::ClientId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoturnIceProviderConfig {
    pub auth_secret: String,
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    pub turns_urls: Vec<String>,
    pub ttl: u64,
}
#[derive(Clone)]
pub struct CoturnIceProvider {
    config: CoturnIceProviderConfig,
}

impl Debug for CoturnIceProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoturnIceProvider")
            .field("stun_urls", &self.config.stun_urls)
            .field("turn_urls", &self.config.turn_urls)
            .field("turns_urls", &self.config.turns_urls)
            .field("ttl", &self.config.ttl)
            .finish_non_exhaustive()
    }
}

impl CoturnIceProvider {
    pub fn new(config: CoturnIceProviderConfig) -> Self {
        Self { config }
    }

    fn calculate_expiry(&self) -> u64 {
        UNIX_EPOCH.elapsed().unwrap_or_default().as_secs() + self.config.ttl
    }
}

#[async_trait::async_trait]
impl IceConfigProvider for CoturnIceProvider {
    #[instrument(level = "debug", err)]
    async fn get_ice_config(&self, user_id: &ClientId) -> Result<IceConfig, IceError> {
        tracing::debug!("Providing coturn ICE config");

        let expiry = self.calculate_expiry();
        let username = format!("{expiry}:{user_id}");

        let mut mac = Hmac::<sha1::Sha1>::new_from_slice(self.config.auth_secret.as_bytes())
            .map_err(|err| IceError::Provider(format!("Failed to create HMAC instance: {err}")))?;
        mac.update(username.as_bytes());

        let credential =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        let ice_servers: Vec<IceServer> = vec![
            IceServer::new(self.config.stun_urls.clone()),
            IceServer::new(self.config.turn_urls.clone())
                .with_auth(username.clone(), credential.clone()),
            IceServer::new(self.config.turns_urls.clone()).with_auth(username, credential),
        ];

        tracing::trace!(?expiry, "Successfully generated TURN credentials");
        Ok(IceConfig {
            ice_servers,
            expires_at: Some(expiry),
        })
    }
}
