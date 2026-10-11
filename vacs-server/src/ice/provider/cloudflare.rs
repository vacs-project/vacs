use crate::ice::IceError;
use crate::ice::provider::IceConfigProvider;
use reqwest::{Client, header};
use serde::Deserialize;
use std::fmt::{Debug, Formatter};
use std::time::{Duration, UNIX_EPOCH};
use vacs_protocol::http::webrtc::{IceConfig, IceServer};
use vacs_protocol::vatsim::ClientId;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudflareIceConfig {
    pub ice_servers: Vec<IceServer>,
}

impl CloudflareIceConfig {
    pub fn into_ice_config(self, expiry: u64) -> IceConfig {
        IceConfig {
            ice_servers: self.ice_servers,
            expires_at: Some(expiry),
        }
    }
}

#[derive(Clone)]
pub struct CloudflareIceProvider {
    client: Client,
    url: String,
    api_token: String,
    ttl: u64,
}

impl Debug for CloudflareIceProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareIceProvider")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl CloudflareIceProvider {
    const CLOUDFLARE_TURN_HTTP_TIMEOUT: Duration = Duration::from_secs(1);
    const CLOUDFLARE_TURN_API_URL: &'static str = "https://rtc.live.cloudflare.com/v1/turn/keys/{TURN_KEY_ID}/credentials/generate-ice-servers";

    pub fn new(
        turn_key_id: impl AsRef<str>,
        turn_key_api_token: impl Into<String>,
        ttl: u64,
    ) -> Result<Self, IceError> {
        Ok(Self {
            client: reqwest::ClientBuilder::new()
                .user_agent(crate::APP_USER_AGENT)
                .timeout(Self::CLOUDFLARE_TURN_HTTP_TIMEOUT)
                .build()
                .map_err(|e| IceError::Provider(format!("Failed to create HTTP client: {e}")))?,
            url: Self::CLOUDFLARE_TURN_API_URL.replace("{TURN_KEY_ID}", turn_key_id.as_ref()),
            api_token: turn_key_api_token.into(),
            ttl,
        })
    }

    fn calculate_expiry(&self) -> u64 {
        UNIX_EPOCH.elapsed().unwrap_or_default().as_secs() + self.ttl
    }
}

#[async_trait::async_trait]
impl IceConfigProvider for CloudflareIceProvider {
    async fn get_ice_config(&self, user_id: &ClientId) -> Result<IceConfig, IceError> {
        let expiry = self.calculate_expiry();
        let res = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_token)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({ "ttl": self.ttl, "customIdentifier": user_id}))
            .send()
            .await;

        let response = match res {
            Ok(response) => response,
            Err(err) => {
                return Err(IceError::Provider(format!(
                    "Failed to generate TURN credentials: {}",
                    err.without_url()
                )));
            }
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(IceError::Provider(format!(
                "Received error while generating TURN credentials: {status}"
            )));
        }

        match serde_json::from_str::<CloudflareIceConfig>(&body) {
            Ok(config) => {
                tracing::trace!(expiry, "Generated TURN credentials");
                Ok(config.into_ice_config(expiry))
            }
            Err(err) => Err(IceError::Provider(format!(
                "Failed to parse TURN credentials response: {:?} error at line {} column {}",
                err.classify(),
                err.line(),
                err.column()
            ))),
        }
    }
}
