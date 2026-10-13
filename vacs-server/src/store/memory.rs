use crate::store::{StoreBackend, key_prefix};
use anyhow::Context;
use bytes::Bytes;
use dashmap::DashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::time::{Duration, Instant};
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
    async fn get<V: DeserializeOwned + Send>(&self, key: &str) -> anyhow::Result<Option<V>> {
        if let Some(stored_value) = self.map.get(key) {
            if let Some(expires_at) = stored_value.expires_at
                && Instant::now() > expires_at
            {
                drop(stored_value);
                self.map.remove_if(key, |_, stored| {
                    stored
                        .expires_at
                        .is_some_and(|expires_at| Instant::now() > expires_at)
                });
                tracing::trace!(
                    key_prefix = key_prefix(key),
                    "Removed expired value from memory store"
                );
                return Ok(None);
            }

            let value = serde_json::from_slice(&stored_value.value)
                .context("Failed to deserialize value from memory store")?;
            tracing::trace!(
                key_prefix = key_prefix(key),
                found = true,
                "Read value from memory store"
            );
            Ok(Some(value))
        } else {
            tracing::trace!(
                key_prefix = key_prefix(key),
                found = false,
                "Read value from memory store"
            );
            Ok(None)
        }
    }

    async fn set<V: Serialize + Send>(
        &self,
        key: &str,
        value: V,
        expiry: Option<Duration>,
    ) -> anyhow::Result<()> {
        let serialized = serde_json::to_vec(&value).context("Failed to serialize value")?;

        self.map.insert(
            key.to_string(),
            StoredValue {
                value: Bytes::from(serialized),
                expires_at: expiry.map(|expiry| Instant::now() + expiry),
            },
        );

        tracing::trace!(key_prefix = key_prefix(key), "Stored value in memory store");
        Ok(())
    }

    async fn remove(&self, key: &str) -> anyhow::Result<()> {
        let removed = self.map.remove(key).is_some();
        tracing::trace!(
            key_prefix = key_prefix(key),
            removed,
            "Removed value from memory store"
        );
        Ok(())
    }

    async fn expire(&self, key: &str, duration: Duration) -> anyhow::Result<()> {
        if let Some(mut entry) = self.map.get_mut(key) {
            entry.expires_at = Some(Instant::now() + duration);
        }
        tracing::trace!(
            key_prefix = key_prefix(key),
            "Set expiry on memory store key"
        );
        Ok(())
    }

    async fn is_healthy(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn reading_an_expired_key_returns_none_without_hanging() {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            runtime.block_on(async {
                let store = MemoryStore::default();
                store
                    .set("key", 1, Some(Duration::from_millis(1)))
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
                let expired = store.get::<i32>("key").await.unwrap();

                store.set("key", 2, None).await.unwrap();
                let fresh = store.get::<i32>("key").await.unwrap();
                tx.send((expired, fresh)).unwrap();
            });
        });

        let (expired, fresh) = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("reading an expired key hung");
        assert_eq!(expired, None);
        assert_eq!(fresh, Some(2));
    }
}
