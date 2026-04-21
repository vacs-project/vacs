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
const VATSIM_OAUTH_PKCE_VERIFIER_KEY: &str = "vatsim.oauth.pkce_verifier";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/vatsim", get(get::vatsim))
        .route("/vatsim/callback", post(post::vatsim_callback))
        .route("/vatsim/redirect", get(get::vatsim_redirect))
        .route("/vatsim/token", post(post::vatsim_token))
        .route("/user", get(get::user_info))
        .route("/logout", post(post::logout))
}

const AUTH_REDIRECT_TEMPLATE: &str = include_str!("../../static/auth_redirect.html");
const AUTH_REDIRECT_ERROR_TEMPLATE: &str = include_str!("../../static/auth_redirect_error.html");

mod get {
    use super::*;
    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::{Html, IntoResponse, Response};
    use serde::Deserialize;
    use vacs_protocol::http::auth::InitVatsimLogin;

    #[derive(Deserialize)]
    pub struct VatsimRedirectParams {
        code: Option<String>,
        state: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    pub async fn vatsim_redirect(
        State(state): State<Arc<AppState>>,
        Query(VatsimRedirectParams {
            code,
            state: oauth_state,
            error,
            error_description,
        }): Query<VatsimRedirectParams>,
    ) -> Response {
        if let Some(error) = error {
            let description = error_description.unwrap_or_else(|| error.clone());
            tracing::warn!(error, "OAuth redirect received error from VATSIM");
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    AUTH_REDIRECT_ERROR_TEMPLATE
                        .replace("__ERROR_DESCRIPTION__", &html_escape(&description)),
                ),
            )
                .into_response();
        }

        let (Some(code), Some(oauth_state)) = (code, oauth_state) else {
            tracing::warn!("OAuth redirect received request without code or state");
            return (
                StatusCode::BAD_REQUEST,
                Html(AUTH_REDIRECT_ERROR_TEMPLATE.replace(
                    "__ERROR_DESCRIPTION__",
                    "Missing authorization code or state parameter.",
                )),
            )
                .into_response();
        };

        let mut deep_link = url::Url::parse(&state.config.auth.oauth.deep_link_url)
            .expect("deep link URL is valid");
        deep_link
            .query_pairs_mut()
            .append_pair("code", &code)
            .append_pair("state", &oauth_state);
        let deep_link = deep_link.as_str();

        Html(
            AUTH_REDIRECT_TEMPLATE
                .replace(
                    "__DEEP_LINK_JSON__",
                    &serde_json::to_string(deep_link).unwrap_or_default(),
                )
                .replace("__DEEP_LINK_HREF__", &html_escape(deep_link)),
        )
        .into_response()
    }

    pub async fn vatsim(auth_session: AuthSession, session: Session) -> ApiResult<InitVatsimLogin> {
        let (url, csrf_token, pkce_verifier) = auth_session.backend().authorize_url();

        session
            .insert(VATSIM_OAUTH_CSRF_TOKEN_KEY, csrf_token)
            .await
            .context("Failed to store CSRF token in session")?;

        session
            .insert(
                VATSIM_OAUTH_PKCE_VERIFIER_KEY,
                pkce_verifier.secret().to_string(),
            )
            .await
            .context("Failed to store PKCE verifier in session")?;

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
        auth_session: AuthSession,
        session: Session,
        Json(AuthExchangeToken { code, state }): Json<AuthExchangeToken>,
    ) -> ApiResult<UserInfo> {
        let stored_state = session
            .remove::<String>(VATSIM_OAUTH_CSRF_TOKEN_KEY)
            .await
            .context("Failed to remove CSRF token from session")?
            .ok_or(AppError::Unauthorized("Missing CSRF token".to_string()))?;

        let pkce_verifier = session
            .remove::<String>(VATSIM_OAUTH_PKCE_VERIFIER_KEY)
            .await
            .context("Failed to remove PKCE verifier from session")?
            .ok_or(AppError::Unauthorized("Missing PKCE verifier".to_string()))?;

        let creds = Credentials::OAuthCode {
            code,
            received_state: state,
            stored_state,
            pkce_verifier,
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
            let auth_session = AuthSession::from_request_parts(&mut parts, &state)
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
