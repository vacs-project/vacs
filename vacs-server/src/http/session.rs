use crate::config::AppConfig;
use tower_sessions::cookie::{Key, SameSite, time};
use tower_sessions::service::SignedCookie;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer, SessionStore};
use tower_sessions_redis_store::{RedisStore, fred::prelude::*};

pub async fn setup_redis_session_manager(
    config: &AppConfig,
    redis_pool: Pool,
) -> anyhow::Result<SessionManagerLayer<RedisStore<Pool>, SignedCookie>> {
    let session_store = RedisStore::new(redis_pool);
    Ok(configure_session_manager(config, session_store))
}

pub async fn setup_memory_session_manager(
    config: &AppConfig,
) -> anyhow::Result<SessionManagerLayer<MemoryStore, SignedCookie>> {
    let session_store = MemoryStore::default();
    Ok(configure_session_manager(config, session_store))
}

fn configure_session_manager<S>(
    config: &AppConfig,
    session_store: S,
) -> SessionManagerLayer<S, SignedCookie>
where
    S: SessionStore + Send + Sync + 'static + Clone,
{
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(config.session.secure)
        .with_http_only(config.session.http_only)
        .with_expiry(Expiry::OnInactivity(time::Duration::seconds(
            config.session.expiry_secs,
        )))
        .with_same_site(SameSite::Lax) // Required for session cookie during OAuth redirect
        .with_signed(if config.session.signing_key.is_empty() {
            Key::generate()
        } else {
            Key::from(config.session.signing_key.as_bytes())
        });

    tracing::debug!("Session manager configured");
    session_layer
}
