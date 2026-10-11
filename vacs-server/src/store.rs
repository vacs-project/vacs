pub mod memory;
pub mod redis;

use crate::store::memory::MemoryStore;
use crate::store::redis::RedisStore;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

#[async_trait::async_trait]
pub trait StoreBackend {
    async fn get<V: DeserializeOwned + Send>(&self, key: &str) -> anyhow::Result<Option<V>>;
    async fn set<V: Serialize + Send>(
        &self,
        key: &str,
        value: V,
        expiry: Option<Duration>,
    ) -> anyhow::Result<()>;
    async fn remove(&self, key: &str) -> anyhow::Result<()>;
    async fn expire(&self, key: &str, duration: Duration) -> anyhow::Result<()>;
    async fn is_healthy(&self) -> anyhow::Result<()>;
}

/// The part of a storage key that is safe to log. Keys embed tokens after their second dot
/// (`ws.token.<token>`), so anything without that shape is logged as empty.
pub(crate) fn key_prefix(key: &str) -> &str {
    key.match_indices('.')
        .nth(1)
        .map_or("", |(idx, _)| &key[..idx])
}

pub enum Store {
    Redis(RedisStore),
    Memory(MemoryStore),
}

#[async_trait::async_trait]
impl StoreBackend for Store {
    async fn get<V: DeserializeOwned + Send>(&self, key: &str) -> anyhow::Result<Option<V>> {
        match self {
            Store::Redis(store) => store.get(key).await,
            Store::Memory(store) => store.get(key).await,
        }
    }

    async fn set<V: Serialize + Send>(
        &self,
        key: &str,
        value: V,
        expiry: Option<Duration>,
    ) -> anyhow::Result<()> {
        match self {
            Store::Redis(store) => store.set(key, value, expiry).await,
            Store::Memory(store) => store.set(key, value, expiry).await,
        }
    }

    async fn remove(&self, key: &str) -> anyhow::Result<()> {
        match self {
            Store::Redis(store) => store.remove(key).await,
            Store::Memory(store) => store.remove(key).await,
        }
    }

    async fn expire(&self, key: &str, duration: Duration) -> anyhow::Result<()> {
        match self {
            Store::Redis(store) => store.expire(key, duration).await,
            Store::Memory(store) => store.expire(key, duration).await,
        }
    }

    async fn is_healthy(&self) -> anyhow::Result<()> {
        match self {
            Store::Redis(store) => store.is_healthy().await,
            Store::Memory(store) => store.is_healthy().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::key_prefix;

    #[test]
    fn key_prefix_drops_the_token() {
        assert_eq!(key_prefix("ws.token.abc-def"), "ws.token");
        assert_eq!(key_prefix("api.token.0000.1111"), "api.token");
        assert_eq!(key_prefix("secret"), "");
        assert_eq!(key_prefix("one.dot"), "");
    }
}
