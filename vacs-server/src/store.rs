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
