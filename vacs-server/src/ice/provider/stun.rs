use crate::ice::IceError;
use crate::ice::provider::IceConfigProvider;
use vacs_protocol::http::webrtc::IceConfig;
use vacs_protocol::vatsim::ClientId;

#[derive(Debug, Clone)]
pub struct StunOnlyProvider {
    stun_servers: Vec<String>,
}

impl StunOnlyProvider {
    pub fn new(stun_servers: Vec<String>) -> Self {
        if stun_servers.is_empty() {
            Self::default()
        } else {
            Self { stun_servers }
        }
    }
}

impl Default for StunOnlyProvider {
    fn default() -> Self {
        Self::new(vec![
            "stun:stun.cloudflare.com:3478".to_string(),
            "stun:stun.cloudflare.com:53".to_string(),
        ])
    }
}

#[async_trait::async_trait]
impl IceConfigProvider for StunOnlyProvider {
    async fn get_ice_config(&self, _user_id: &ClientId) -> Result<IceConfig, IceError> {
        tracing::trace!("Providing STUN-only ICE config");
        Ok(IceConfig::from(self.stun_servers.clone()))
    }
}
