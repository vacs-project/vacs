use crate::auth::layer::setup_mock_auth_layer;
use crate::config::{AppConfig, AuthConfig, VatsimConfig};
use crate::ice::provider::stun::StunOnlyProvider;
use crate::ratelimit::RateLimiters;
use crate::release::UpdateChecker;
use crate::release::catalog::file::FileCatalog;
use crate::release::policy::Policy;
use crate::routes::create_app;
use crate::state::AppState;
use crate::store::Store;
use crate::store::memory::MemoryStore;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use vacs_vatsim::coverage::network::Network;
use vacs_vatsim::data_feed::mock::MockDataFeed;
use vacs_vatsim::slurper::SlurperClient;

pub struct TestApp {
    state: Arc<AppState>,
    pub mock_data_feed: Arc<MockDataFeed>,
    addr: String,
    http_base_url: String,
    shutdown_tx: watch::Sender<()>,
    handle: JoinHandle<()>,
}

impl TestApp {
    pub async fn new() -> Self {
        Self::new_with_network(Network::default()).await
    }

    pub async fn new_with_network(network: Network) -> Self {
        Self::build(
            network,
            Self::default_config(),
            RateLimiters::default(),
            UpdateChecker::default(),
        )
        .await
    }

    /// Runs the server with a tailored config, for tests that need a specific
    /// conference size limit or invite rate limit.
    pub async fn new_with_config(config: AppConfig, rate_limiters: RateLimiters) -> Self {
        Self::build(
            Network::default(),
            config,
            rate_limiters,
            UpdateChecker::default(),
        )
        .await
    }

    /// Runs the server behind a release policy that only accepts client
    /// protocol versions matching `compatible_protocol_range`.
    pub async fn new_with_protocol_range(compatible_protocol_range: &str) -> Self {
        let mut policy_file = tempfile::NamedTempFile::new().expect("Failed to create policy file");
        writeln!(
            policy_file,
            "compatible_protocol_range = \"{compatible_protocol_range}\""
        )
        .expect("Failed to write policy file");

        // Policy reads the file once, so the handle may go away afterwards.
        let policy = Policy::new(policy_file.path()).expect("Failed to load policy");
        let catalog = FileCatalog::new("releases.toml").expect("Failed to load release catalog");

        Self::build(
            Network::default(),
            Self::default_config(),
            RateLimiters::default(),
            UpdateChecker::new(Arc::new(catalog), policy),
        )
        .await
    }

    /// The config every test app starts from: no VATSIM connection required
    /// and a short login timeout.
    pub fn default_config() -> AppConfig {
        AppConfig {
            auth: AuthConfig {
                login_flow_timeout_millis: 100,
                ..Default::default()
            },
            vatsim: VatsimConfig {
                user_service: Default::default(),
                require_active_connection: false,
                slurper_base_url: Default::default(),
                controller_update_interval: Default::default(),
                data_feed_url: Default::default(),
                data_feed_timeout: Default::default(),
                coverage_dir: Default::default(),
                data_feed_position_grace_period: Duration::from_secs(90),
            },
            ..Default::default()
        }
    }

    async fn build(
        network: Network,
        config: AppConfig,
        rate_limiters: RateLimiters,
        updates: UpdateChecker,
    ) -> Self {
        let mock_data_feed = Arc::new(MockDataFeed::default());

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let state = Arc::new(AppState::new(
            config.clone(),
            updates,
            Store::Memory(MemoryStore::default()),
            SlurperClient::new("http://localhost:12345").unwrap(),
            mock_data_feed.clone(),
            network,
            rate_limiters,
            shutdown_rx,
            Arc::new(StunOnlyProvider::default()),
            None,
        ));

        let auth_layer = setup_mock_auth_layer(&config).await.unwrap();
        let app = create_app(
            auth_layer,
            None,
            config.server.client_ip_source.clone(),
            config.server.debug_endpoints,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let state_clone = state.clone();
        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.with_state(state_clone)
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        Self {
            state,
            mock_data_feed,
            http_base_url: format!("http://{addr}"),
            addr: format!("ws://{addr}/ws"),
            shutdown_tx,
            handle,
        }
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn http_base_url(&self) -> &str {
        &self.http_base_url
    }

    pub fn state(&self) -> Arc<AppState> {
        self.state.clone()
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.shutdown_tx.send(()).unwrap();
        self.handle.abort();
    }
}
