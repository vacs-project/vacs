use crate::auth::extractor::AuthenticatedUser;
use crate::auth::users::{AuthSession, Credentials};
use crate::http::ApiResult;
use crate::http::error::AppError;
use crate::state::AppState;
use anyhow::Context;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::Arc;
use tower_sessions::Session;
use vacs_protocol::http::auth::UserInfo;

const VATSIM_OAUTH_CSRF_TOKEN_KEY: &str = "vatsim.oauth.csrf_token";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/vatsim", get(get::vatsim))
        .route("/vatsim/callback", post(post::vatsim_callback))
        .route("/vatsim/token", post(post::vatsim_token))
        .route("/user", get(get::user_info))
        .route("/logout", post(post::logout))
}

mod get {
    use super::*;
    use vacs_protocol::http::auth::InitVatsimLogin;

    pub async fn vatsim(auth_session: AuthSession, session: Session) -> ApiResult<InitVatsimLogin> {
        let (url, csrf_token) = auth_session.backend.authorize_url();

        session
            .insert(VATSIM_OAUTH_CSRF_TOKEN_KEY, csrf_token)
            .await
            .context("Failed to store CSRF token in session")?;

        Ok(Json(InitVatsimLogin {
            url: url.to_string(),
        }))
    }

    pub async fn user_info(auth: AuthenticatedUser) -> ApiResult<UserInfo> {
        Ok(Json(UserInfo { cid: auth.user.cid }))
    }
}

mod post {
    use super::*;
    use crate::http::StatusCodeResult;
    use axum::extract::{FromRequestParts, State};
    use axum::http::StatusCode;
    use axum_client_ip::ClientIp;
    use vacs_protocol::http::auth::AuthExchangeToken;

    pub async fn vatsim_callback(
        mut auth_session: AuthSession,
        session: Session,
        Json(AuthExchangeToken { code, state }): Json<AuthExchangeToken>,
    ) -> ApiResult<UserInfo> {
        let stored_state = session
            .remove::<String>(VATSIM_OAUTH_CSRF_TOKEN_KEY)
            .await
            .context("Failed to remove CSRF token from session")?
            .ok_or(AppError::Unauthorized("Missing CSRF token".to_string()))?;

        let creds = Credentials::OAuthCode {
            code,
            received_state: state,
            stored_state,
        };

        tracing::debug!("Authenticating with VATSIM");
        let user = match auth_session.authenticate(creds).await {
            Ok(Some(user)) => user,
            Ok(None) => return Err(AppError::Unauthorized("Invalid credentials".to_string())),
            Err(err) => return Err(err.into()),
        };

        auth_session
            .login(&user)
            .await
            .context("Failed to login user")?;

        Ok(Json(UserInfo { cid: user.cid }))
    }

    pub async fn logout(
        auth: AuthenticatedUser,
        State(state): State<Arc<AppState>>,
        mut parts: http::request::Parts,
    ) -> StatusCodeResult {
        tracing::debug!("Logging user out");

        if let Some(token) = &auth.api_token {
            state
                .revoke_api_token(token)
                .await
                .context("Failed to revoke API token")?;
        } else {
            let mut auth_session = AuthSession::from_request_parts(&mut parts, &state)
                .await
                .map_err(|_| AppError::Unauthorized("Not authenticated".to_string()))?;
            let session = Session::from_request_parts(&mut parts, &state)
                .await
                .map_err(|err| AppError::InternalServerError(anyhow::anyhow!("{err:?}")))?;
            auth_session.logout().await.context("Failed to logout")?;
            session
                .delete()
                .await
                .context("Failed to destroy session")?;
        }

        Ok(StatusCode::NO_CONTENT)
    }

    pub async fn vatsim_token(
        auth_session: AuthSession,
        State(state): State<Arc<AppState>>,
        ClientIp(client_ip): ClientIp,
        headers: http::HeaderMap,
    ) -> ApiResult<vacs_protocol::http::auth::AuthTokenResponse> {
        if let Err(until) = state.rate_limiters().check_vatsim_token(client_ip) {
            tracing::debug!(
                ?client_ip,
                ?until,
                "Rate limit exceeded, rejecting token request"
            );
            return Err(AppError::TooManyRequests(until.as_secs()));
        }

        let access_token = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?
            .to_string();

        let creds = Credentials::AccessToken { access_token };

        tracing::debug!("Authenticating with VATSIM access token");
        let user = match auth_session.authenticate(creds).await {
            Ok(Some(user)) => user,
            Ok(None) => return Err(AppError::Unauthorized("Invalid credentials".to_string())),
            Err(err) => return Err(err.into()),
        };

        let token = state
            .generate_api_token(user.cid.as_str())
            .await
            .context("Failed to generate API token")?;

        Ok(Json(vacs_protocol::http::auth::AuthTokenResponse {
            cid: user.cid,
            token,
        }))
    }
}
