use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use vacs_protocol::vatsim::ClientId;
use vatsim_api::mock::MockServer;
use vatsim_api::mock::state::SharedState as MockVatsimState;
use vatsim_api::types::CertificateId;
use vatsim_api::types::connect::{
    ConnectRatingInfo, ConnectUser, NamedInfo, OAuthInfo, PersonalDetails, VatsimDetails,
};

use super::CertificateIdExt;
use super::TestClient;

use crate::auth::layer::setup_test_auth_layer;
use crate::config::{AppConfig, AuthConfig, OAuthConfig, VatsimConfig};
use crate::ice::provider::stun::StunOnlyProvider;
use crate::ratelimit::RateLimiters;
use crate::release::UpdateChecker;
use crate::routes::create_app;
use crate::state::AppState;
use crate::store::Store;
use crate::store::memory::MemoryStore;
use vacs_vatsim::coverage::network::Network;
use vacs_vatsim::data_feed::VatsimDataFeed;
use vacs_vatsim::slurper::SlurperClient;

/// Base CID for default test users. User N gets CID `DEFAULT_CID_BASE + N`
/// (i.e. 1000001, 1000002, ...).
const DEFAULT_CID_BASE: u32 = 1_000_000;

/// A self-contained test environment that spins up a mock VATSIM server and a
/// real vacs-server instance wired together.
///
/// The mock VATSIM server provides real HTTP endpoints for OAuth, datafeed,
/// and slurper. The vacs-server uses the real `Backend` auth layer pointed
/// at these URLs, with `MemoryStore` for sessions (no Redis required).
///
/// The environment owns all resources and tears them down on drop.
pub struct TestEnv {
    state: Arc<AppState>,
    mock_vatsim: vatsim_api::mock::MockServerHandle,
    ws_url: String,
    http_base_url: String,
    shutdown_tx: watch::Sender<()>,
    handle: JoinHandle<()>,
}

/// Builder for configuring a [`TestEnv`].
pub struct TestEnvBuilder {
    users: Vec<ConnectUser>,
    controllers: Vec<vatsim_api::types::datafeed::Controller>,
    network: Network,
    require_active_connection: bool,
}

impl TestEnv {
    /// Returns a new [`TestEnvBuilder`] with sensible defaults.
    #[must_use]
    pub fn builder() -> TestEnvBuilder {
        TestEnvBuilder {
            users: Vec::new(),
            controllers: Vec::new(),
            network: Network::default(),
            require_active_connection: false,
        }
    }

    /// Returns the WebSocket URL for connecting to the vacs-server.
    #[must_use]
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Returns the HTTP base URL of the vacs-server (e.g. `http://127.0.0.1:PORT`).
    #[must_use]
    pub fn http_base_url(&self) -> &str {
        &self.http_base_url
    }

    /// Returns a shared reference to the vacs-server application state.
    #[must_use]
    pub fn state(&self) -> &Arc<AppState> {
        &self.state
    }

    /// Returns a shared reference to the mock VATSIM server state, allowing
    /// runtime mutations (add controllers, users, etc.) without going
    /// through HTTP.
    ///
    /// This controls both the datafeed (`/v3/vatsim-data.json`) and the
    /// OAuth/slurper endpoints. Use
    /// [`MockState::upsert_controller`](vatsim_api::mock::state::MockState::upsert_controller)
    /// to add online controllers mid-test.
    #[must_use]
    pub fn vatsim_api(&self) -> &MockVatsimState {
        self.mock_vatsim.state()
    }

    /// Returns the base URL of the mock VATSIM server.
    #[must_use]
    pub fn vatsim_api_base_url(&self) -> &str {
        self.mock_vatsim.base_url()
    }

    /// Adds or replaces a controller in the mock VATSIM datafeed.
    pub async fn upsert_controller(&self, controller: vatsim_api::types::datafeed::Controller) {
        self.mock_vatsim
            .state()
            .write()
            .await
            .upsert_controller(controller);
    }

    /// Removes a controller from the mock VATSIM datafeed by CID.
    ///
    /// Returns `true` if a controller with that CID was present.
    pub async fn remove_controller(&self, cid: impl Into<ClientId>) -> bool {
        let cert_id = CertificateId::from_client_id(&cid.into());
        self.mock_vatsim
            .state()
            .write()
            .await
            .remove_controller(cert_id)
    }

    /// Creates an HTTP client with a cookie jar that is pre-authenticated
    /// for the given user CID. This walks the full OAuth flow:
    ///
    /// 1. `GET /auth/vatsim` to initiate login (stores CSRF state in session)
    /// 2. Follows the redirect to the mock VATSIM OAuth authorize endpoint
    ///    (with `login_hint` set to the target CID)
    /// 3. `POST /auth/vatsim/callback` to exchange the code
    ///
    /// The returned client has a valid session cookie and can call any
    /// authenticated endpoint.
    pub async fn authenticated_http_client(
        &self,
        cid: impl Into<ClientId>,
    ) -> anyhow::Result<reqwest::Client> {
        let cid = cid.into();
        let jar = Arc::new(reqwest::cookie::Jar::default());
        let client = reqwest::Client::builder()
            .cookie_provider(jar)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        // Step 1: Initiate login
        let body: serde_json::Value = client
            .get(format!("{}/auth/vatsim", self.http_base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let auth_url_str = body["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing `url` in /auth/vatsim response"))?;

        // Append login_hint so the mock OAuth server authenticates as the
        // correct user instead of always picking the first one.
        let mut auth_url = Url::parse(auth_url_str)?;
        auth_url
            .query_pairs_mut()
            .append_pair("login_hint", cid.as_str());

        // Step 2: Follow redirect to mock OAuth (which auto-approves and
        // redirects back with code + state)
        let redirect_resp = client
            .get(auth_url.as_str())
            .send()
            .await?
            .error_for_status()?;
        let redirect_location = redirect_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or_else(|| anyhow::anyhow!("No redirect from mock OAuth authorize"))?
            .to_str()?;

        // Parse code and state from the redirect URL
        let redirect_url = Url::parse(redirect_location)?;
        let params: HashMap<String, String> = redirect_url.query_pairs().into_owned().collect();
        let code = params
            .get("code")
            .ok_or_else(|| anyhow::anyhow!("No `code` in OAuth redirect"))?;
        let state = params
            .get("state")
            .ok_or_else(|| anyhow::anyhow!("No `state` in OAuth redirect"))?;

        // Step 3: Exchange code for session
        let resp = client
            .post(format!("{}/auth/vatsim/callback", self.http_base_url))
            .json(&serde_json::json!({ "code": code, "state": state }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OAuth callback failed with {status}: {body}");
        }

        Ok(client)
    }

    /// Obtains a WebSocket auth token for the given user CID by walking
    /// the full OAuth flow and then calling `GET /ws/token`.
    pub async fn ws_token_for(&self, cid: impl Into<ClientId>) -> anyhow::Result<String> {
        let client = self.authenticated_http_client(cid).await?;

        let resp = client
            .get(format!("{}/ws/token", self.http_base_url))
            .send()
            .await?
            .error_for_status()?;
        let body: serde_json::Value = resp.json().await?;
        let token = body["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing `token` in /ws/token response"))?
            .to_string();

        Ok(token)
    }

    /// Authenticates and connects `n` users via WebSocket, returning them as
    /// a `Vec<TestClient>`.
    ///
    /// The users are drawn from the seeded user list (see
    /// [`TestEnvBuilder::default_users`]). Each client walks the full OAuth
    /// flow to obtain a WS token and then performs a WS login.
    ///
    /// Client CIDs will be `"1000001"`, `"1000002"`, etc. (matching
    /// [`default_users`]).
    pub async fn setup_clients(&self, n: usize) -> Vec<TestClient> {
        let mut clients = Vec::with_capacity(n);
        for i in 1..=n {
            let cid = format!("{}", DEFAULT_CID_BASE + i as u32);
            let token = self
                .ws_token_for(cid.as_str())
                .await
                .expect("ws_token_for failed");
            let client = TestClient::new_with_login(
                self.ws_url(),
                cid.as_str(),
                &token,
                |_, _| Ok(()),
                |_| Ok(()),
                |_| Ok(()),
            )
            .await
            .unwrap_or_else(|e| panic!("Failed to connect client {cid}: {e}"));
            clients.push(client);
        }
        clients
    }

    /// Like [`setup_clients`](Self::setup_clients) but returns a
    /// `HashMap<ClientId, TestClient>` for named access.
    pub async fn setup_clients_map(&self, n: usize) -> HashMap<ClientId, TestClient> {
        self.setup_clients(n)
            .await
            .into_iter()
            .map(|c| (c.id().clone(), c))
            .collect()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        self.handle.abort();
    }
}

impl TestEnvBuilder {
    /// Adds Connect users for OAuth authentication in the mock VATSIM server.
    #[must_use]
    pub fn users(mut self, users: Vec<ConnectUser>) -> Self {
        self.users = users;
        self
    }

    /// Convenience: seeds `n` default test users with CIDs 1000001 through
    /// 1000000+n. Use with [`TestEnv::setup_clients`] to connect them.
    #[must_use]
    pub fn default_users(mut self, n: usize) -> Self {
        self.users = (1..=n)
            .map(|i| {
                let cid = DEFAULT_CID_BASE + i as u32;
                test_user(cid.to_string(), &format!("User{i}"), &format!("Test{i}"))
            })
            .collect();
        self
    }

    /// Sets the initial online controllers in the mock VATSIM datafeed.
    /// These can be mutated at runtime via [`TestEnv::vatsim_api()`].
    #[must_use]
    pub fn controllers(
        mut self,
        controllers: Vec<vatsim_api::types::datafeed::Controller>,
    ) -> Self {
        self.controllers = controllers;
        self
    }

    /// Sets the coverage network configuration for vacs-server.
    #[must_use]
    pub fn network(mut self, network: Network) -> Self {
        self.network = network;
        self
    }

    /// Sets whether vacs-server should require an active VATSIM connection
    /// for WebSocket login. Default: `false`.
    #[must_use]
    pub fn require_active_connection(mut self, require: bool) -> Self {
        self.require_active_connection = require;
        self
    }

    /// Builds the [`TestEnv`], starting both the mock VATSIM server and
    /// the vacs-server.
    pub async fn build(self) -> TestEnv {
        // Start mock VATSIM server
        let mock_vatsim = MockServer::builder()
            .users(self.users)
            .controllers(self.controllers)
            .spawn()
            .await;

        let mock_base = mock_vatsim.base_url().to_owned();

        let config = AppConfig {
            auth: AuthConfig {
                login_flow_timeout_millis: 100,
                oauth: OAuthConfig {
                    auth_url: format!("{mock_base}/oauth/authorize"),
                    token_url: format!("{mock_base}/oauth/token"),
                    redirect_url: "vacs://auth/vatsim/callback".to_string(),
                    client_id: "test-client-id".to_string(),
                    client_secret: "test-client-secret".to_string(),
                },
                ..Default::default()
            },
            vatsim: VatsimConfig {
                user_service: crate::config::VatsimUserServiceConfig {
                    user_details_endpoint_url: format!("{mock_base}/api/user"),
                },
                require_active_connection: self.require_active_connection,
                slurper_base_url: mock_base.clone(),
                data_feed_url: format!("{mock_base}/v3/vatsim-data.json"),
                ..Default::default()
            },
            session: crate::config::SessionConfig {
                secure: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let data_feed = Arc::new(
            VatsimDataFeed::new(
                &config.vatsim.data_feed_url,
                config.vatsim.data_feed_timeout,
            )
            .expect("Failed to create VatsimDataFeed")
            .with_cache_ttl(Duration::ZERO),
        );

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let state = Arc::new(AppState::new(
            config.clone(),
            UpdateChecker::default(),
            Store::Memory(MemoryStore::default()),
            SlurperClient::new(&mock_base).unwrap(),
            data_feed,
            self.network,
            RateLimiters::default(),
            shutdown_rx,
            Arc::new(StunOnlyProvider::default()),
            None,
        ));

        let auth_layer = setup_test_auth_layer(&config)
            .await
            .expect("Failed to set up test auth layer");
        let app = create_app(
            auth_layer,
            None,
            config.server.client_ip_source.clone(),
            false,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind test server");
        let addr = listener.local_addr().expect("Failed to get local address");

        let state_clone = state.clone();
        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.with_state(state_clone)
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .ok();
        });

        TestEnv {
            state,
            mock_vatsim,
            http_base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            shutdown_tx,
            handle,
        }
    }
}

/// Returns the CID string for default test user `n` (1-based).
///
/// E.g., `cid(1)` returns `"1000001"`, `cid(2)` returns `"1000002"`.
#[must_use]
pub fn cid(n: usize) -> String {
    format!("{}", DEFAULT_CID_BASE + n as u32)
}

/// Creates a minimal [`ConnectUser`] for test seeding.
///
/// The user will have Controller rating C1, no pilot rating, and belong to
/// the VATSIM Europe region.
#[must_use]
pub fn test_user(cid: impl Into<ClientId>, first_name: &str, last_name: &str) -> ConnectUser {
    let client_id = cid.into();
    ConnectUser {
        cid: CertificateId::from_client_id(&client_id),
        personal: PersonalDetails {
            name_first: first_name.to_owned(),
            name_last: last_name.to_owned(),
            name_full: format!("{first_name} {last_name}"),
            email: None,
            country: None,
        },
        vatsim: VatsimDetails {
            rating: ConnectRatingInfo {
                id: 3,
                short: "C1".to_owned(),
                long: "Controller".to_owned(),
            },
            pilotrating: ConnectRatingInfo {
                id: 0,
                short: "NEW".to_owned(),
                long: "Basic Member".to_owned(),
            },
            region: NamedInfo {
                id: Some("EMEA".to_owned()),
                name: Some("Europe, Middle East and Africa".to_owned()),
            },
            division: NamedInfo {
                id: Some("EUD".to_owned()),
                name: Some("Europe (except UK)".to_owned()),
            },
            subdivision: None,
        },
        oauth: OAuthInfo {
            token_valid: "true".to_owned(),
        },
    }
}

/// Creates a minimal [`Controller`](vatsim_api::types::datafeed::Controller)
/// for seeding the mock VATSIM datafeed.
#[must_use]
pub fn test_controller(
    cid: impl Into<ClientId>,
    callsign: &str,
    frequency: &str,
    facility: vatsim_api::types::Facility,
) -> vatsim_api::types::datafeed::Controller {
    let client_id = cid.into();
    vatsim_api::types::datafeed::Controller {
        cid: CertificateId::from_client_id(&client_id),
        name: format!("Test Controller {}", client_id.as_str()),
        callsign: callsign.to_owned(),
        frequency: frequency.to_owned(),
        facility,
        rating: vatsim_api::types::ControllerRating::EnrouteController,
        server: "TEST".to_owned(),
        visual_range: 300,
        text_atis: None,
        last_updated: vatsim_api::chrono::Utc::now(),
        logon_time: vatsim_api::chrono::Utc::now(),
    }
}
