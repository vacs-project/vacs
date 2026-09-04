use crate::auth::users::Backend;
use crate::config::AppConfig;
use crate::http::session::setup_redis_session_manager;
use anyhow::Context;
use axum_login::{AuthManagerLayer, AuthManagerLayerBuilder};
use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
use tower_sessions::service::SignedCookie;
use tower_sessions_redis_store::RedisStore;
use tower_sessions_redis_store::fred::prelude::Pool;
use tracing::instrument;

fn create_oauth_backend(config: &AppConfig) -> anyhow::Result<Backend> {
    let client = BasicClient::new(ClientId::new(config.auth.oauth.client_id.clone()))
        .set_client_secret(ClientSecret::new(config.auth.oauth.client_secret.clone()))
        .set_auth_uri(AuthUrl::new(config.auth.oauth.auth_url.clone()).context("Invalid auth URL")?)
        .set_token_uri(
            TokenUrl::new(config.auth.oauth.token_url.clone()).context("Invalid token URL")?,
        )
        .set_redirect_uri(
            RedirectUrl::new(config.auth.oauth.redirect_url.clone())
                .context("Invalid redirect URL")?,
        );
    Backend::new(
        client,
        config.vatsim.user_service.user_details_endpoint_url.clone(),
    )
}

#[instrument(level = "debug", skip_all, err)]
pub async fn setup_auth_layer(
    config: &AppConfig,
    redis_pool: Pool,
) -> anyhow::Result<AuthManagerLayer<Backend, RedisStore<Pool>, SignedCookie>> {
    tracing::debug!("Setting up authentication layer");

    let backend = create_oauth_backend(config)?;
    let session_layer = setup_redis_session_manager(config, redis_pool).await?;

    tracing::debug!("Authentication layer setup complete");
    Ok(AuthManagerLayerBuilder::new(backend, session_layer).build())
}

/// Sets up a real [`Backend`] auth layer backed by in-memory sessions.
///
/// This is intended for integration tests that run a real mock VATSIM HTTP
/// server (e.g. `vatsim-api` mock) so the full OAuth code -> token -> user
/// flow is exercised over HTTP, without requiring Redis.
#[cfg(feature = "test-utils")]
#[instrument(level = "debug", skip_all, err)]
pub async fn setup_test_auth_layer(
    config: &AppConfig,
) -> anyhow::Result<AuthManagerLayer<Backend, tower_sessions::MemoryStore, SignedCookie>> {
    tracing::debug!("Setting up test authentication layer");

    let backend = create_oauth_backend(config)?;
    let session_layer = crate::http::session::setup_memory_session_manager(config).await?;

    tracing::debug!("Test authentication layer setup complete");
    Ok(AuthManagerLayerBuilder::new(backend, session_layer).build())
}
