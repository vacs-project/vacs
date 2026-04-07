use crate::APP_USER_AGENT;
use crate::http::error::AppError;
use anyhow::Context;
use axum_login::{AuthUser, AuthnBackend, UserId};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthorizationCode, CsrfToken, EndpointNotSet, EndpointSet, HttpRequest, TokenResponse,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use vacs_protocol::vatsim::ClientId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub cid: ClientId,
}

impl AuthUser for User {
    type Id = ClientId;

    fn id(&self) -> Self::Id {
        self.cid.clone()
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.cid.as_bytes()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub enum Credentials {
    OAuthCode {
        code: String,
        stored_state: String,
        received_state: String,
    },
    AccessToken {
        access_token: String,
    },
}

pub type VatsimOAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Debug, Clone)]
pub struct Backend {
    client: VatsimOAuthClient,
    http_client: reqwest::Client,
    vatsim_user_details_endpoint_url: String,
}

impl Backend {
    pub fn new(
        client: VatsimOAuthClient,
        vatsim_user_details_endpoint_url: String,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client,
            http_client: reqwest::ClientBuilder::new()
                .user_agent(APP_USER_AGENT)
                .build()
                .context("Failed to build HTTP client")?,
            vatsim_user_details_endpoint_url,
        })
    }

    pub fn authorize_url(&self) -> (Url, CsrfToken) {
        self.client.authorize_url(CsrfToken::new_random).url()
    }

    async fn fetch_user_details(&self, access_token: &str) -> Result<User, AppError> {
        tracing::trace!(?access_token, "Fetching user details");
        let response = self
            .http_client
            .get(self.vatsim_user_details_endpoint_url.clone())
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to get user details")?
            .error_for_status()
            .context("Received non-200 HTTP status code")?;

        tracing::trace!(content_length = ?response.content_length(), "Parsing response body");
        let user_details = response
            .json::<ConnectUserDetails>()
            .await
            .context("Failed to parse response body")?;

        Ok(User {
            cid: user_details.data.cid,
        })
    }
}

impl AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = AppError;

    #[instrument(level = "debug", skip_all, err)]
    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        tracing::debug!("Authenticating user");

        let access_token = match creds {
            Credentials::OAuthCode {
                code,
                stored_state,
                received_state,
            } => {
                if stored_state != received_state {
                    tracing::debug!("CSRF token mismatch");
                    return Ok(None);
                }

                tracing::trace!("Exchanging code for VATSIM access token");
                let token = self
                    .client
                    .exchange_code(AuthorizationCode::new(code))
                    .request_async(&ReqwestClient(&self.http_client))
                    .await
                    .context("Failed to exchange code")
                    .map_err(|err| {
                        tracing::warn!(?err, "Failed to exchange code for VATSIM access token");
                        AppError::Unauthorized("Invalid code".to_string())
                    })?;

                token.access_token().secret().to_string()
            }
            Credentials::AccessToken { access_token } => access_token,
        };

        let user = self.fetch_user_details(&access_token).await?;
        tracing::debug!(?user, "User authenticated");
        Ok(Some(user))
    }

    #[instrument(level = "trace", skip(self), err)]
    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        tracing::trace!(?user_id, "Getting user");
        Ok(Some(User {
            cid: user_id.clone(),
        }))
    }
}

pub type AuthSession = axum_login::AuthSession<Backend>;

#[derive(Deserialize, Debug, Clone)]
struct ConnectUserDetails {
    data: ConnectUserDetailsData,
}

#[derive(Deserialize, Debug, Clone)]
struct ConnectUserDetailsData {
    cid: ClientId,
}

// Wrapper for reqwest::Client to implement oauth2::AsyncHttpClient.
// Required until oauth2 compatibility with reqwest >= 0.13.0 is fixed
// See: https://github.com/ramosbugs/oauth2-rs/issues/333, https://github.com/ramosbugs/oauth2-rs/pull/334
struct ReqwestClient<'a>(&'a reqwest::Client);

impl<'a, 'c> oauth2::AsyncHttpClient<'c> for ReqwestClient<'a> {
    type Error = oauth2::HttpClientError<reqwest::Error>;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<oauth2::HttpResponse, Self::Error>> + Send + Sync + 'c>,
    >;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let response = self
                .0
                .execute(request.try_into().map_err(Box::new)?)
                .await
                .map_err(Box::new)?;
            let mut response_builder = http::Response::builder()
                .status(response.status())
                .version(response.version());
            for (header_name, header_value) in response.headers() {
                response_builder = response_builder.header(header_name, header_value);
            }
            response_builder
                .body(response.bytes().await.map_err(Box::new)?.to_vec())
                .map_err(oauth2::HttpClientError::Http)
        })
    }
}
