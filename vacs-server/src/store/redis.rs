use crate::config::RedisConfig;
use crate::store::StoreBackend;
use anyhow::Context;
use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::time::Duration;
use tower_sessions_redis_store::fred::interfaces::ClientLike;
use tower_sessions_redis_store::fred::prelude::Expiration::EX;
use tower_sessions_redis_store::fred::prelude::{Config, KeysInterface, Pool};
use tower_sessions_redis_store::fred::types::Builder;
use tracing::instrument;

#[derive(Debug)]
pub struct RedisStore {
    pool: Pool,
}

impl RedisStore {
    #[instrument(level = "trace", err)]
    pub async fn new(redis_config: &RedisConfig) -> anyhow::Result<Self> {
        tracing::trace!("Creating Redis pool");
        let pool_config = Config::from_url_centralized(&redis_config.addr)
            .context("Failed to create redis pool config")?;
        let pool = Builder::from_config(pool_config)
            .with_performance_config(|config| {
                config.default_command_timeout = Duration::from_secs(2);
            })
            .build_pool(5)
            .context("Failed to create redis pool")?;

        tracing::trace!("Connecting to Redis");
        pool.connect();
        pool.wait_for_connect()
            .await
            .context("Failed to connect to redis")?;

        tracing::info!("Redis connection pool created");
        Ok(Self { pool })
    }

    pub fn get_pool(&self) -> &Pool {
        &self.pool
    }
}

#[async_trait::async_trait]
impl StoreBackend for RedisStore {
    #[instrument(level = "trace", skip(self), err)]
    async fn get<V: DeserializeOwned + Send>(&self, key: &str) -> anyhow::Result<Option<V>> {
        tracing::trace!("Getting value from redis store");
        match self
            .pool
            .get::<Option<Bytes>, _>(key)
            .await
            .context("Failed to get value from redis")?
        {
            Some(serialized) => {
                tracing::trace!("Deserializing value from redis store");
                let value: V = serde_json::from_slice(serialized.as_ref())
                    .context("Failed to deserialize value from redis store")?;

                tracing::trace!("Successfully retrieved value from redis store");
                Ok(Some(value))
            }
            None => {
                tracing::trace!("Value not found in redis store");
                Ok(None)
            }
        }
    }

    #[instrument(level = "trace", skip(self, value), err)]
    async fn set<V: Serialize + Send>(
        &self,
        key: &str,
        value: V,
        expiry: Option<Duration>,
    ) -> anyhow::Result<()> {
        tracing::trace!("Serializing value for redis store");
        let serialized = serde_json::to_vec(&value).context("Failed to serialize value")?;

        tracing::trace!("Storing value in redis store");
        self.pool
            .set::<Bytes, _, _>(
                key,
                serialized,
                expiry.map(|d| EX(d.as_secs() as i64)),
                None,
                false,
            )
            .await
            .context("Failed to store value in redis")?;

        tracing::trace!("Successfully stored value in redis store");
        Ok(())
    }

    #[instrument(level = "trace", skip(self), err)]
    async fn remove(&self, key: &str) -> anyhow::Result<()> {
        tracing::trace!("Removing value from redis store");
        let removed = self
            .pool
            .del::<i64, _>(key)
            .await
            .context("Failed to remove value from redis")?;

        tracing::trace!(?removed, "Successfully removed value from redis store");
        Ok(())
    }

    #[instrument(level = "trace", skip(self), err)]
    async fn expire(&self, key: &str, duration: Duration) -> anyhow::Result<()> {
        tracing::trace!("Setting expiry on redis key");
        self.pool
            .expire::<bool, _>(key, duration.as_secs() as i64, None)
            .await
            .context("Failed to set expiry on redis key")?;
        Ok(())
    }

    async fn is_healthy(&self) -> anyhow::Result<()> {
        self.pool.ping(None).await.context("Failed to ping redis")
    }
}
