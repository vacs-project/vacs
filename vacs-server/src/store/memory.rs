use crate::store::StoreBackend;
use anyhow::Context;
use bytes::Bytes;
use dashmap::DashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::time::{Duration, Instant};
use tracing::instrument;
use uuid::Uuid;

#[derive(Debug)]
struct StoredValue {
    value: Bytes,
    expires_at: Option<Instant>,
}

#[derive(Debug)]
pub struct MemoryStore {
    map: DashMap<String, StoredValue>,
}

impl MemoryStore {
    /// Returns a deterministic test API token (valid UUID) for the given index.
    pub fn test_api_token(index: u128) -> String {
        Uuid::from_u128(index).to_string()
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        let map = DashMap::new();
        for i in 0..=5u128 {
            map.insert(
                format!("ws.token.token{i}"),
                StoredValue {
                    value: Bytes::from(format!("\"client{i}\"")),
                    expires_at: None,
                },
            );
            map.insert(
                format!("api.token.{}", Uuid::from_u128(i)),
                StoredValue {
                    value: Bytes::from(format!("\"cid{i}\"")),
                    expires_at: None,
                },
            );
        }

        Self { map }
    }
}

#[async_trait::async_trait]
impl StoreBackend for MemoryStore {
    #[instrument(level = "trace", skip(self), err)]
    async fn get<V: DeserializeOwned + Send>(&self, key: &str) -> anyhow::Result<Option<V>> {
        tracing::trace!("Getting value from memory store");
        if let Some(stored_value) = self.map.get(key) {
            if let Some(expires_at) = stored_value.expires_at
                && Instant::now() > expires_at
            {
                tracing::trace!("Value expired, removing from memory store and returning None");
                self.map.remove(key);
                return Ok(None);
            }

            tracing::trace!("Deserializing value from memory store");
            let value = serde_json::from_slice(&stored_value.value)
                .context("Failed to deserialize value from memory store")?;

            tracing::trace!("Successfully retrieved value from memory store");
            Ok(Some(value))
        } else {
            tracing::trace!("Value not found in memory store");
            Ok(None)
        }
    }

    #[instrument(level = "trace", skip(self, value), err)]
    async fn set<V: Serialize + Send>(
        &self,
        key: &str,
        value: V,
        expiry: Option<Duration>,
    ) -> anyhow::Result<()> {
        tracing::trace!("Serializing value for memory store");
        let serialized = serde_json::to_vec(&value).context("Failed to serialize value")?;

        tracing::trace!("Storing value in memory store");
        self.map.insert(
            key.to_string(),
            StoredValue {
                value: Bytes::from(serialized),
                expires_at: expiry.map(|expiry| Instant::now() + expiry),
            },
        );

        tracing::trace!("Successfully stored value in memory store");
        Ok(())
    }

    #[instrument(level = "trace", skip(self), err)]
    async fn remove(&self, key: &str) -> anyhow::Result<()> {
        tracing::trace!("Removing value from memory store");
        self.map.remove(key);

        tracing::trace!("Successfully removed value from memory store");
        Ok(())
    }

    #[instrument(level = "trace", skip(self), err)]
    async fn expire(&self, key: &str, duration: Duration) -> anyhow::Result<()> {
        tracing::trace!("Setting expiry on memory store key");
        if let Some(mut entry) = self.map.get_mut(key) {
            entry.expires_at = Some(Instant::now() + duration);
        }
        Ok(())
    }

    async fn is_healthy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
