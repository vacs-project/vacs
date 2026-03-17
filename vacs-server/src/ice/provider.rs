pub mod cloudflare;
pub mod coturn;
pub mod stun;

use crate::ice::IceError;
use vacs_protocol::http::webrtc::IceConfig;
use vacs_protocol::vatsim::ClientId;

#[async_trait::async_trait]
pub trait IceConfigProvider: Send + Sync {
    async fn get_ice_config(&self, user_id: &ClientId) -> Result<IceConfig, IceError>;
}
