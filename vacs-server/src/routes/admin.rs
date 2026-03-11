use crate::state::AppState;
use axum::Router;
use axum::routing::post;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/dataset/reload", post(post::reload_dataset))
}

mod post {
    use crate::http::StatusCodeResult;
    use crate::http::error::AppError;
    use crate::state::AppState;
    use axum::Json;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use jsonwebtoken::{DecodingKey, Validation, decode, jwk::JwkSet};
    use serde::Deserialize;
    use std::sync::Arc;
    use std::time::Duration;
    use tracing::instrument;

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
    /// 4. Subject matches the configured allowed subject (repo + environment)
    async fn verify_github_oidc(
        config: &crate::config::AdminConfig,
        headers: &HeaderMap,
    ) -> Result<(), AppError> {
        // Extract bearer token
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| {
                tracing::warn!("Missing or invalid Authorization header");
                AppError::Unauthorized("Authentication failed".to_string())
            })?;

        // Decode the JWT header to get the key ID
        let header = jsonwebtoken::decode_header(token).map_err(|err| {
            tracing::warn!(?err, "Failed to decode JWT header");
            AppError::Unauthorized("Authentication failed".to_string())
        })?;

        let kid = header.kid.ok_or_else(|| {
            tracing::warn!("JWT header missing key ID");
            AppError::Unauthorized("Authentication failed".to_string())
        })?;

        // Fetch GitHub's JWKS
        let client = reqwest::Client::builder()
            .user_agent(crate::APP_USER_AGENT)
            .timeout(JWKS_REQUEST_TIMEOUT)
            .build()
            .map_err(|err| {
                tracing::error!(?err, "Failed to build HTTP client for JWKS fetch");
                AppError::InternalServerError(anyhow::anyhow!("Failed to build HTTP client"))
            })?;

        let jwks: JwkSet = client
            .get(GITHUB_OIDC_JWKS_URL)
            .send()
            .await
            .map_err(|err| {
                tracing::error!(?err, "Failed to fetch GitHub OIDC JWKS");
                AppError::InternalServerError(anyhow::anyhow!("Failed to fetch OIDC JWKS"))
            })?
            .json()
            .await
            .map_err(|err| {
                tracing::error!(?err, "Failed to parse GitHub OIDC JWKS");
                AppError::InternalServerError(anyhow::anyhow!("Failed to parse OIDC JWKS"))
            })?;

        // Find the matching key
        let jwk = jwks.find(&kid).ok_or_else(|| {
            tracing::warn!(%kid, "No matching key found in GitHub OIDC JWKS");
            AppError::Unauthorized("Authentication failed".to_string())
        })?;

        let decoding_key = DecodingKey::from_jwk(jwk).map_err(|err| {
            tracing::error!(?err, "Failed to construct decoding key from JWK");
            AppError::InternalServerError(anyhow::anyhow!("Failed to construct decoding key"))
        })?;

        // Validate the token - always require RS256;
        // never trust the algorithm from the JWT header to prevent algorithm
        // confusion attacks (e.g. "none").
        let mut validation = Validation::new(GITHUB_OIDC_JWT_ALGORITHM);
        validation.set_issuer(&[GITHUB_OIDC_ISSUER]);
        validation.set_audience(&[&config.oidc_audience]);

        let token_data =
            decode::<GitHubOidcClaims>(token, &decoding_key, &validation).map_err(|err| {
                tracing::warn!(?err, "JWT validation failed");
                AppError::Unauthorized("Authentication failed".to_string())
            })?;

        let claims = token_data.claims;

        // Verify subject matches allowed pattern
        if claims.sub != config.oidc_allowed_sub {
            tracing::warn!(
                expected = %config.oidc_allowed_sub,
                actual = %claims.sub,
                "JWT subject does not match allowed subject"
            );
            return Err(AppError::Unauthorized("Authentication failed".to_string()));
        }

        tracing::info!(
            sub = %claims.sub,
            iss = %claims.iss,
            aud = %claims.aud,
            "Authenticated via GitHub Actions OIDC token"
        );
        Ok(())
    }

    #[instrument(level = "info", skip(state, headers, body))]
    pub async fn reload_dataset(
        State(state): State<Arc<AppState>>,
        headers: HeaderMap,
        Json(body): Json<ReloadRequest>,
    ) -> StatusCodeResult {
        verify_github_oidc(&state.config.admin, &headers).await?;

        let git_ref = &body.git_ref;
        let commit_sha = body.sha.as_deref().unwrap_or(git_ref);
        tracing::info!(%git_ref, %commit_sha, "Dataset reload triggered");

        let previous_sha = state.dataset.as_ref().and_then(|d| d.local_sha());

        let dataset = state.dataset.as_ref().ok_or_else(|| {
            tracing::warn!("Dataset reload requested but no dataset repository is configured");
            AppError::NotFound
        })?;

        let network = dataset
            .fetch_and_install(git_ref, commit_sha)
            .await
            .map_err(|err| {
                tracing::error!(?err, %git_ref, %commit_sha, "Failed to fetch and install dataset");
                AppError::InternalServerError(anyhow::anyhow!(
                    "Failed to fetch and install dataset"
                ))
            })?;

        state.replace_network(network).await;

        tracing::info!(
            from = ?previous_sha,
            to = %commit_sha,
            "Dataset reload completed successfully"
        );

        Ok(StatusCode::OK)
    }
}
