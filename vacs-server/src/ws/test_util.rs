use crate::config::{AppConfig, VatsimConfig};
use crate::ice::provider::stun::StunOnlyProvider;
use crate::metrics::guards::ClientConnectionGuard;
use crate::ratelimit::RateLimiters;
use crate::release::UpdateChecker;
use crate::state::AppState;
use crate::state::clients::session::ClientSession;
use crate::store::Store;
use crate::store::memory::MemoryStore;
use axum::extract::ws;
use futures_util::{Sink, Stream};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc, watch};
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{ClientId, PositionId};
use vacs_protocol::ws::server::{ClientInfo, ServerMessage};
use vacs_vatsim::coverage::network::Network;
use vacs_vatsim::data_feed::mock::MockDataFeed;
use vacs_vatsim::slurper::SlurperClient;

pub struct MockSink {
    tx: mpsc::Sender<ws::Message>,
}

impl MockSink {
    pub fn new(tx: mpsc::Sender<ws::Message>) -> Self {
        Self { tx }
    }
}

impl Sink<ws::Message> for MockSink {
    type Error = axum::Error;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: ws::Message) -> Result<(), Self::Error> {
        self.tx.try_send(item).map_err(axum::Error::new)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

pub struct MockStream {
    messages: Vec<Result<ws::Message, axum::Error>>,
}

impl MockStream {
    pub fn new(messages: Vec<Result<ws::Message, axum::Error>>) -> Self {
        Self { messages }
    }
}

impl Stream for MockStream {
    type Item = Result<ws::Message, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.messages.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Ready(Some(self.messages.remove(0)))
        }
    }
}

pub struct TestSetup {
    pub app_state: Arc<AppState>,
    pub session: ClientSession,
    pub mock_stream: MockStream,
    pub mock_sink: MockSink,
    pub websocket_tx: Arc<Mutex<mpsc::Sender<ws::Message>>>,
    pub websocket_rx: Arc<Mutex<mpsc::Receiver<ws::Message>>>,
    pub rx: mpsc::Receiver<ServerMessage>,
    pub broadcast_rx: broadcast::Receiver<ServerMessage>,
    pub shutdown_tx: watch::Sender<()>,
    pub coverage_dir: tempfile::TempDir,
}

impl Default for TestSetup {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSetup {
    pub fn new() -> Self {
        let coverage_dir = tempfile::tempdir().unwrap();
        let mut vatsim_users = HashMap::new();
        for i in 0..=5 {
            vatsim_users.insert(format!("token{i}"), format!("client{i}"));
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let config = AppConfig {
            vatsim: VatsimConfig {
                user_service: Default::default(),
                require_active_connection: false,
                slurper_base_url: Default::default(),
                controller_update_interval: Default::default(),
                data_feed_url: Default::default(),
                data_feed_timeout: Default::default(),
                coverage_dir: coverage_dir.path().to_str().unwrap().to_string(),
                data_feed_position_grace_period: Duration::from_secs(90),
            },
            ..Default::default()
        };
        let mock_data_feed = Arc::new(MockDataFeed::default());
        let app_state = Arc::new(AppState::new(
            config,
            UpdateChecker::default(),
            Store::Memory(MemoryStore::default()),
            SlurperClient::new("http://localhost:12345").unwrap(),
            mock_data_feed.clone(),
            Network::load_from_dir(coverage_dir.path()).unwrap(),
            RateLimiters::default(),
            shutdown_rx,
            Arc::new(StunOnlyProvider::default()),
            None,
        ));
        let client_info = ClientInfo {
            id: ClientId::from("client1"),
            position_id: Some(PositionId::from("position1")),
            display_name: "Client 1".to_string(),
            frequency: "100.000".to_string(),
        };
        let (tx, rx) = mpsc::channel(10);
        let session = ClientSession::new(
            client_info,
            ActiveProfile::Specific(ProfileId::from("profile1")),
            tx,
            ClientConnectionGuard::default(),
        );
        let (websocket_tx, websocket_rx) = mpsc::channel(100);
        let mock_stream = MockStream::new(vec![]);
        let mock_sink = MockSink::new(websocket_tx.clone());
        let (_broadcast_tx, broadcast_rx) = broadcast::channel(10);

        Self {
            app_state,
            session,
            mock_stream,
            mock_sink,
            websocket_tx: Arc::new(Mutex::new(websocket_tx)),
            websocket_rx: Arc::new(Mutex::new(websocket_rx)),
            rx,
            broadcast_rx,
            shutdown_tx,
            coverage_dir,
        }
    }

    pub fn with_messages(mut self, messages: Vec<Result<ws::Message, axum::Error>>) -> Self {
        self.mock_stream = MockStream::new(messages);
        self
    }

    pub async fn register_client(
        &self,
        client_info: ClientInfo,
    ) -> (ClientSession, mpsc::Receiver<ServerMessage>) {
        self.app_state
            .register_client(
                client_info,
                ActiveProfile::Specific(ProfileId::from("profile1")),
                ClientConnectionGuard::default(),
            )
            .await
            .expect("Failed to register client")
    }

    pub async fn register_client_with_profile(
        &self,
        client_info: ClientInfo,
        active_profile: ActiveProfile<ProfileId>,
    ) -> (ClientSession, mpsc::Receiver<ServerMessage>) {
        self.app_state
            .register_client(
                client_info,
                active_profile,
                ClientConnectionGuard::default(),
            )
            .await
            .expect("Failed to register client")
    }

    pub async fn register_clients(
        &self,
        client_ids: Vec<ClientInfo>,
    ) -> HashMap<String, (ClientSession, mpsc::Receiver<ServerMessage>)> {
        futures_util::future::join_all(client_ids.into_iter().map(|client_id| async move {
            (
                client_id.id.to_string(),
                self.register_client(client_id).await,
            )
        }))
        .await
        .into_iter()
        .collect()
    }

    pub async fn take_last_websocket_message(&mut self) -> Option<ws::Message> {
        self.websocket_rx.lock().await.recv().await
    }

    pub fn spawn_session_handle_interaction(
        self,
    ) -> (tokio::task::JoinHandle<()>, watch::Sender<()>) {
        let TestSetup {
            app_state,
            mut session,
            mock_stream,
            mock_sink,
            mut broadcast_rx,
            mut rx,
            shutdown_tx,
            ..
        } = self;

        let mut shutdown_rx = shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            session
                .handle_interaction(
                    &app_state,
                    mock_stream,
                    mock_sink,
                    &mut broadcast_rx,
                    &mut rx,
                    &mut shutdown_rx,
                )
                .await;
        });

        (handle, shutdown_tx)
    }
}

pub fn create_client_info(id: u8) -> ClientInfo {
    ClientInfo {
        id: ClientId::from(format!("client{id}")),
        position_id: Some(PositionId::from(format!("position{id}"))),
        display_name: format!("Client {id}"),
        frequency: format!("{id}00.000"),
    }
}
