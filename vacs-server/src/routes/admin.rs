use crate::state::AppState;
use axum::Router;
use axum::routing::post;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dataset/reload", post(post::reload_dataset))
        .route("/releases/reload", post(post::reload_releases))
}

mod post {
    use crate::http::StatusCodeResult;
    use crate::http::error::AppError;
    use crate::state::AppState;
    use axum::Json;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use octocrab::jsonwebtoken::{self, DecodingKey, Validation, decode, jwk::JwkSet};
    use serde::Deserialize;
    use std::sync::Arc;
    use std::time::Duration;

    /// GitHub Actions OIDC issuer.
    const GITHUB_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
    /// GitHub Actions OIDC JWKS endpoint.
    const GITHUB_OIDC_JWKS_URL: &str =
        "https://token.actions.githubusercontent.com/.well-known/jwks";
    /// Expected algorithm for GitHub Actions OIDC tokens.
    const GITHUB_OIDC_JWT_ALGORITHM: jsonwebtoken::Algorithm = jsonwebtoken::Algorithm::RS256;
    /// Timeout for fetching the JWKS from GitHub.
    const JWKS_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

    /// Claims from a GitHub Actions OIDC token.
    #[derive(Debug, Deserialize)]
    struct GitHubOidcClaims {
        sub: String,
        iss: String,
        aud: String,
    }

    /// Request body for the dataset reload endpoint.
    #[derive(Debug, Deserialize)]
    pub struct ReloadRequest {
        /// The git ref to download (tag, branch, or commit SHA).
        #[serde(rename = "ref")]
        pub git_ref: String,
        /// The resolved commit SHA. Used as the authoritative version marker
        /// stored on disk. If omitted, `git_ref` is used as-is.
        #[serde(default)]
        pub sha: Option<String>,
    }

    /// Verify that the request carries a valid GitHub Actions OIDC token.
    ///
    /// The token is expected as a Bearer token in the Authorization header.
    /// Verification checks:
    /// 1. JWT signature against GitHub's JWKS (fetched on each request - this
    ///    endpoint is called infrequently so caching is unnecessary)
    /// 2. Issuer matches GitHub's OIDC issuer
    /// 3. Audience matches the configured expected audience
    /// 4. Subject matches `allowed_sub`, supplied per endpoint so neither can call the other
    async fn verify_github_oidc(
        config: &crate::config::AdminConfig,
        allowed_sub: &str,
        headers: &HeaderMap,
    ) -> Result<(), AppError> {
        // Extract bearer token
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| {
                tracing::info!(reason = "missing_bearer_token", "Admin request rejected");
                AppError::Unauthorized("Authentication failed".to_string())
            })?;

        // Decode the JWT header to get the key ID
        let header = jsonwebtoken::decode_header(token).map_err(|err| {
            tracing::info!(reason = "invalid_jwt_header", %err, "Admin request rejected");
            AppError::Unauthorized("Authentication failed".to_string())
        })?;

        let kid = header.kid.ok_or_else(|| {
            tracing::info!(reason = "missing_key_id", "Admin request rejected");
            AppError::Unauthorized("Authentication failed".to_string())
        })?;

        // Fetch GitHub's JWKS
        let client = reqwest::Client::builder()
            .user_agent(crate::APP_USER_AGENT)
            .timeout(JWKS_REQUEST_TIMEOUT)
            .build()
            .map_err(|err| {
                AppError::InternalServerError(
                    anyhow::Error::new(err).context("Failed to build HTTP client for JWKS fetch"),
                )
            })?;

        let jwks: JwkSet = client
            .get(GITHUB_OIDC_JWKS_URL)
            .send()
            .await
            .map_err(|err| {
                AppError::InternalServerError(
                    anyhow::Error::new(err).context("Failed to fetch GitHub OIDC JWKS"),
                )
            })?
            .json()
            .await
            .map_err(|err| {
                AppError::InternalServerError(
                    anyhow::Error::new(err).context("Failed to parse GitHub OIDC JWKS"),
                )
            })?;

        // Find the matching key
        let jwk = jwks.find(&kid).ok_or_else(|| {
            tracing::info!(
                reason = "unknown_key_id",
                kid = crate::logging::bounded(&kid),
                "Admin request rejected"
            );
            AppError::Unauthorized("Authentication failed".to_string())
        })?;

        let decoding_key = DecodingKey::from_jwk(jwk).map_err(|err| {
            AppError::InternalServerError(
                anyhow::Error::new(err).context("Failed to construct decoding key from JWK"),
            )
        })?;

        // Validate the token - always require RS256;
        // never trust the algorithm from the JWT header to prevent algorithm
        // confusion attacks (e.g. "none").
        let mut validation = Validation::new(GITHUB_OIDC_JWT_ALGORITHM);
        validation.set_issuer(&[GITHUB_OIDC_ISSUER]);
        validation.set_audience(&[&config.oidc_audience]);

        let token_data =
            decode::<GitHubOidcClaims>(token, &decoding_key, &validation).map_err(|err| {
                tracing::info!(reason = "jwt_validation_failed", %err, "Admin request rejected");
                AppError::Unauthorized("Authentication failed".to_string())
            })?;

        let claims = token_data.claims;

        // Verify subject matches allowed pattern
        if claims.sub != allowed_sub {
            tracing::info!(
                reason = "subject_mismatch",
                expected_sub = %allowed_sub,
                actual_sub = %claims.sub,
                "Admin request rejected"
            );
            return Err(AppError::Unauthorized("Authentication failed".to_string()));
        }

        tracing::debug!(
            sub = %claims.sub,
            iss = %claims.iss,
            aud = %claims.aud,
            "Authenticated via GitHub Actions OIDC token"
        );
        Ok(())
    }

    pub async fn reload_dataset(
        State(state): State<Arc<AppState>>,
        headers: HeaderMap,
        Json(body): Json<ReloadRequest>,
    ) -> StatusCodeResult {
        let allowed_sub = state.config.admin.oidc_allowed_sub.clone();
        verify_github_oidc(&state.config.admin, &allowed_sub, &headers).await?;

        let git_ref = &body.git_ref;
        let commit_sha = body.sha.as_deref().unwrap_or(git_ref);
        tracing::info!(%git_ref, %commit_sha, "Dataset reload triggered");

        let dataset = state.dataset.as_ref().ok_or_else(|| {
            tracing::warn!("Dataset reload requested but no dataset repository is configured");
            AppError::NotFound
        })?;

        let network = dataset
            .fetch_and_install(git_ref, commit_sha)
            .await
            .map_err(|err| {
                AppError::InternalServerError(err.context("Failed to fetch and install dataset"))
            })?;

        state.replace_network(network).await;

        Ok(StatusCode::OK)
    }

    /// Repopulate the release catalog so the updater offers a new release before the cache TTL
    /// expires.
    pub async fn reload_releases(
        State(state): State<Arc<AppState>>,
        headers: HeaderMap,
    ) -> StatusCodeResult {
        let allowed_sub = state
            .config
            .admin
            .oidc_allowed_sub_releases
            .clone()
            .ok_or_else(|| {
                tracing::warn!(
                    "Release catalog reload requested but no allowed subject is configured"
                );
                AppError::NotFound
            })?;

        verify_github_oidc(&state.config.admin, &allowed_sub, &headers).await?;

        tracing::info!("Release catalog reload triggered");

        state.updates.refresh_catalog().await?;

        Ok(StatusCode::OK)
    }
}
