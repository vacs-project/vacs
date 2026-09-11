use crate::data_feed::{DataFeed, DataFeedError};
use crate::{ControllerInfo, FacilityType, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::Deserialize;
use std::fmt::{Debug, Formatter};
use std::time::{Duration, Instant};
use tracing::instrument;
use vacs_protocol::vatsim::ClientId;

const DATA_FEED_DEFAULT_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct VatsimDataFeed {
    url: String,
    client: reqwest::Client,
    cache_ttl: Duration,
    cache: RwLock<Option<Cache>>,
}

impl VatsimDataFeed {
    pub fn new(url: &str, timeout: Duration) -> Result<Self> {
        let client = reqwest::ClientBuilder::new()
            .user_agent(crate::APP_USER_AGENT)
            .timeout(timeout)
            .build()
            .map_err(DataFeedError::from)?;

        Ok(Self {
            url: url.to_string(),
            client,
            cache_ttl: DATA_FEED_DEFAULT_CACHE_TTL,
            cache: Default::default(),
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.client = reqwest::ClientBuilder::new()
            .user_agent(crate::APP_USER_AGENT)
            .timeout(timeout)
            .build()
            .map_err(DataFeedError::from)?;
        Ok(self)
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self.cache = Default::default();
        self
    }

    #[instrument(level = "trace", skip(self), err)]
    async fn fetch_data_feed(&self) -> Result<VatsimDataFeedResponse> {
        tracing::trace!("Fetching VATSIM data feed");
        let response = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(DataFeedError::from)?;

        tracing::trace!(content_length = ?response.headers().get(reqwest::header::CONTENT_LENGTH), "Parsing VATSIM data feed response body");
        let body = response.json().await.map_err(DataFeedError::from)?;

        Ok(body)
    }
}

#[async_trait]
impl DataFeed for VatsimDataFeed {
    #[instrument(level = "debug", skip(self), err)]
    async fn fetch_controller_info(&self) -> Result<Vec<ControllerInfo>> {
        tracing::debug!("Fetching controller info");

        if let Some(cache) = self.cache.read().as_ref()
            && cache.updated_at.elapsed() < self.cache_ttl
        {
            tracing::debug!(?cache, "Returning cached controller info");
            return Ok(cache.data.clone());
        }

        let data_feed = self.fetch_data_feed().await?;
        let controllers: Vec<ControllerInfo> =
            data_feed.controllers.into_iter().map(Into::into).collect();

        let cache = Cache {
            data: controllers.clone(),
            updated_at: Instant::now(),
        };
        *self.cache.write() = Some(cache);

        tracing::debug!(controllers = ?controllers.len(), "Returning controller info");
        Ok(controllers)
    }
}

struct Cache {
    data: Vec<ControllerInfo>,
    updated_at: Instant,
}

impl Debug for Cache {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("controllers", &self.data.len())
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct VatsimDataFeedResponse {
    pub controllers: Vec<VatsimDataFeedController>,
}

#[derive(Debug, Deserialize)]
struct VatsimDataFeedController {
    cid: i32,
    callsign: String,
    frequency: String,
    #[serde(default, deserialize_with = "lenient_visual_range")]
    visual_range: Option<u32>,
}

// A malformed value must degrade to None instead of failing the whole feed.
fn lenient_visual_range<'de, D>(deserializer: D) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let range = value.as_u64().and_then(|v| u32::try_from(v).ok());
    if range.is_none() && !value.is_null() {
        tracing::warn!(%value, "Unexpected visual_range in data feed, ignoring");
    }
    Ok(range)
}

impl From<VatsimDataFeedController> for ControllerInfo {
    fn from(value: VatsimDataFeedController) -> Self {
        Self {
            cid: ClientId::from(value.cid),
            frequency: value.frequency,
            facility_type: FacilityType::from(value.callsign.as_str()),
            callsign: value.callsign,
            visual_range: value.visual_range,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_controller_info_parses_visual_range_leniently() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{
                    "controllers": [
                        {"cid": 1000001, "callsign": "LOWW_TWR", "frequency": "119.400", "visual_range": 50},
                        {"cid": 1000001, "callsign": "LOWW_ATIS", "frequency": "121.725", "visual_range": 0},
                        {"cid": 1000000, "callsign": "LOVV_CTR", "frequency": "132.600"},
                        {"cid": 1000002, "callsign": "LOVV_FSS", "frequency": "128.950", "visual_range": -1}
                    ]
                }"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let feed = VatsimDataFeed::new(&server.uri(), Duration::from_secs(1))?;
        let controllers = feed.fetch_controller_info().await?;

        let ranges: Vec<(&str, Option<u32>)> = controllers
            .iter()
            .map(|c| (c.callsign.as_str(), c.visual_range))
            .collect();
        assert_eq!(
            ranges,
            vec![
                ("LOWW_TWR", Some(50)),
                ("LOWW_ATIS", Some(0)),
                ("LOVV_CTR", None),
                ("LOVV_FSS", None),
            ]
        );
        Ok(())
    }
}
