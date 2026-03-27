use crate::auth::users::User;
use crate::http::error::AppError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use std::sync::Arc;
use vacs_protocol::vatsim::ClientId;

/// An authenticated user, resolved from either:
/// 1. An `Authorization: Bearer <api_token>` header (API token flow), or
/// 2. A session cookie (standard OAuth flow via `axum_login`).
pub struct AuthenticatedUser {
    pub user: User,
    /// The API token used to authenticate, if any. `None` for session-based auth.
    pub api_token: Option<String>,
}

impl AuthenticatedUser {
    pub fn cid(&self) -> &ClientId {
        &self.user.cid
    }
}

impl FromRequestParts<Arc<AppState>> for AuthenticatedUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = extract_bearer_token(parts) {
            if let Some(cid) = state.verify_api_token(&token).await.map_err(|err| {
                tracing::warn!(?err, "Failed to verify API token");
                AppError::Unauthorized("Invalid token".to_string())
            })? {
                return Ok(Self {
                    user: User { cid },
                    api_token: Some(token),
                });
            }

            return Err(AppError::Unauthorized("Invalid token".to_string()));
        }

        let auth_session =
            axum_login::AuthSession::<crate::auth::users::Backend>::from_request_parts(
                parts, state,
            )
            .await
            .map_err(|err| {
                tracing::debug!(?err, "Failed to extract auth session");
                AppError::Unauthorized("Not authenticated".to_string())
            })?;

        match auth_session.user().await {
            Some(user) => Ok(Self {
                user,
                api_token: None,
            }),
            None => Err(AppError::Unauthorized("Not authenticated".to_string())),
        }
    }
}

fn extract_bearer_token(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, header};

    fn parts_with_header(name: header::HeaderName, value: &str) -> Parts {
        let (parts, _) = Request::builder()
            .header(name, value)
            .body(())
            .unwrap()
            .into_parts();
        parts
    }

    fn parts_without_auth() -> Parts {
        let (parts, _) = Request::builder().body(()).unwrap().into_parts();
        parts
    }

    #[test]
    fn extracts_bearer_token() {
        let parts = parts_with_header(AUTHORIZATION, "Bearer my-secret-token");
        assert_eq!(
            extract_bearer_token(&parts),
            Some("my-secret-token".to_string())
        );
    }

    #[test]
    fn returns_none_without_auth_header() {
        let parts = parts_without_auth();
        assert_eq!(extract_bearer_token(&parts), None);
    }

    #[test]
    fn returns_none_for_non_bearer_scheme() {
        let parts = parts_with_header(AUTHORIZATION, "Basic dXNlcjpwYXNz");
        assert_eq!(extract_bearer_token(&parts), None);
    }

    #[test]
    fn returns_none_for_bearer_without_space() {
        let parts = parts_with_header(AUTHORIZATION, "Bearertoken");
        assert_eq!(extract_bearer_token(&parts), None);
    }

    #[test]
    fn returns_none_for_bearer_with_trailing_space() {
        let parts = parts_with_header(AUTHORIZATION, "Bearer ");
        assert_eq!(extract_bearer_token(&parts), None);
    }

    mod authenticated_user {
        use super::*;
        use crate::config::AppConfig;
        use crate::ice::provider::stun::StunOnlyProvider;
        use crate::ratelimit::RateLimiters;
        use crate::release::UpdateChecker;
        use crate::state::AppState;
        use crate::store::Store;
        use crate::store::memory::MemoryStore;
        use tokio::sync::watch;
        use vacs_vatsim::coverage::network::Network;
        use vacs_vatsim::data_feed::mock::MockDataFeed;
        use vacs_vatsim::slurper::SlurperClient;

        fn test_state() -> Arc<AppState> {
            let (_, shutdown_rx) = watch::channel(());
            Arc::new(AppState::new(
                AppConfig::default(),
                UpdateChecker::default(),
                Store::Memory(MemoryStore::default()),
                SlurperClient::new("http://localhost:12345").unwrap(),
                Arc::new(MockDataFeed::default()),
                Network::default(),
                RateLimiters::default(),
                shutdown_rx,
                Arc::new(StunOnlyProvider::default()),
                None,
            ))
        }

        #[tokio::test]
        async fn valid_bearer_token_returns_user() {
            let state = test_state();
            let token = MemoryStore::test_api_token(0);
            let mut parts = parts_with_header(AUTHORIZATION, &format!("Bearer {token}"));

            let result = AuthenticatedUser::from_request_parts(&mut parts, &state).await;

            let auth = result.unwrap();
            assert_eq!(auth.cid().as_str(), "cid0");
            assert_eq!(auth.api_token, Some(token));
        }

        #[tokio::test]
        async fn invalid_bearer_token_returns_unauthorized() {
            let state = test_state();
            let mut parts = parts_with_header(AUTHORIZATION, "Bearer nonexistent");

            let result = AuthenticatedUser::from_request_parts(&mut parts, &state).await;

            assert!(matches!(result, Err(AppError::Unauthorized(_))));
        }

        #[tokio::test]
        async fn no_auth_header_returns_unauthorized() {
            let state = test_state();
            let mut parts = parts_without_auth();

            let result = AuthenticatedUser::from_request_parts(&mut parts, &state).await;

            assert!(matches!(result, Err(AppError::Unauthorized(_))));
        }
    }
}
