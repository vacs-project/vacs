use crate::auth::TokenProvider;
use crate::error::{SignalingError, SignalingRuntimeError, UntilInstant};
use crate::matcher::ResponseMatcher;
use crate::transport::{SignalingReceiver, SignalingSender, SignalingTransport};
use parking_lot::{Mutex, RwLock};
use rand::RngExt;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_tungstenite::tungstenite;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, instrument};
use vacs_protocol::VACS_PROTOCOL_VERSION;
use vacs_protocol::profile::{ActiveProfile, Profile};
use vacs_protocol::vatsim::{PositionId, StationId};
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{ClientInfo, ServerMessage, SessionProfile};
use vacs_protocol::ws::{client, server};

const BROADCAST_CHANNEL_SIZE: usize = 100;
const SEND_CHANNEL_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Default state, no connection to the server, messages cannot be sent or received.
    /// This state will also be set when a disconnect is requested, a websocket timeout/error is received,
    /// or one of the processing tasks encounters an error.
    Disconnected,
    /// Connected to the server but not logged in yet.
    /// This state is set after the [`SignalingClient`] has successfully established a websocket connection
    /// to the server but has not performed authentication yet.
    /// The only message that can be sent is [`SignalingMessage::Login`], as the server will reject all others.
    /// The [`SignalingClient`] will automatically perform a login using the TokenProvider's auth token.
    /// Depending on the result of the login, the [`State`] will either change to [`State::LoggedIn`] (on success)
    /// or [`State::Disconnected`] (on receiving a login failure).
    Connected,
    /// Connected to the server and successfully authenticated.
    /// This state is set after a successful login. Messages can be sent and received.
    LoggedIn,
}

#[derive(Debug, Clone)]
pub enum SignalingEvent {
    /// Emitted after the [`SignalingClient`] successfully connected to the server, including authentication.
    /// The client is ready to send and receive messages.
    Connected {
        /// Information about the connected client.
        client_info: ClientInfo,
        /// The profile associated with the current session.
        profile: ActiveProfile<Profile>,
        /// The ordered list of default call sources for the current position.
        default_call_sources: Vec<StationId>,
    },
    /// Emitted for every [`ServerMessage`] received by a connected and authenticated [`SignalingClient`].
    Message(ServerMessage),
    /// Emitted for every [`SignalingRuntimeError`] handled by the [`SignalingClient`].
    /// This includes issues during transmission or other errors received from the server.
    Error(SignalingRuntimeError),
}

type BoxFutUnit = Pin<Box<dyn Future<Output = ()> + Send>>;
type OnEventCb = Arc<dyn Fn(SignalingEvent) -> BoxFutUnit + Send + Sync>;
pub type OnTerminateSessionCb = Arc<dyn Fn() -> BoxFutUnit + Send + Sync>;

#[derive(Clone)]
pub struct SignalingClient<ST: SignalingTransport, TP: TokenProvider> {
    inner: Arc<SignalingClientInner<ST, TP>>,
    supervisor_task: Arc<JoinHandle<()>>,
}

impl<ST: SignalingTransport, TP: TokenProvider> SignalingClient<ST, TP> {
    #[allow(clippy::too_many_arguments)]
    pub fn new<F, Fut>(
        transport: ST,
        token_provider: TP,
        on_event: F,
        shutdown_token: CancellationToken,
        custom_profile: bool,
        login_timeout: Duration,
        reconnect_max_tries: u8,
        on_terminate_session: Option<OnTerminateSessionCb>,
        handle: &tokio::runtime::Handle,
    ) -> Self
    where
        F: Fn(SignalingEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let inner = Arc::new(SignalingClientInner::new(
            transport,
            token_provider,
            Arc::new(move |e| Box::pin(on_event(e))),
            shutdown_token,
            custom_profile,
            login_timeout,
            reconnect_max_tries,
            on_terminate_session,
        ));

        let inner_clone = inner.clone();
        let supervisor_task = Arc::new(handle.spawn(async move {
            inner_clone.supervisor_task().await;
        }));

        Self {
            inner,
            supervisor_task,
        }
    }

    /// Subscribes to a broadcast channel emitting [`SignalingEvent`]s.
    pub fn subscribe(&self) -> broadcast::Receiver<SignalingEvent> {
        self.inner.subscribe()
    }

    /// Subscribes to a watch containing the current [`SignalingClient`]'s [`State`].
    pub fn subscribe_state(&self) -> watch::Receiver<State> {
        self.inner.subscribe_state()
    }

    pub fn state(&self) -> State {
        self.inner.state()
    }

    pub async fn connect(&self, position_id: Option<PositionId>) -> Result<(), SignalingError> {
        self.inner.set_position_id(position_id);
        self.inner.connect().await
    }

    pub async fn disconnect(&self) {
        self.inner.disconnect(true).await;
    }

    pub async fn send(&self, msg: ClientMessage) -> Result<(), SignalingError> {
        self.inner.send(msg).await
    }

    pub async fn recv(&self) -> Result<ServerMessage, SignalingError> {
        self.inner.recv().await
    }

    pub fn matcher(&self) -> &ResponseMatcher {
        self.inner.matcher()
    }

    pub async fn recv_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<ServerMessage, SignalingError> {
        self.inner.recv_with_timeout(timeout).await
    }
}

impl<ST: SignalingTransport, TP: TokenProvider> Drop for SignalingClient<ST, TP> {
    fn drop(&mut self) {
        self.inner.shutdown_token.cancel();
        self.supervisor_task.abort();
    }
}

#[derive(Clone)]
struct SignalingClientInner<ST: SignalingTransport, TP: TokenProvider> {
    transport: ST,
    token_provider: TP,

    on_event: OnEventCb,

    state_tx: watch::Sender<State>,
    state_rx: watch::Receiver<State>,

    disconnect_token: Arc<Mutex<CancellationToken>>,
    shutdown_token: CancellationToken,

    matcher: ResponseMatcher,
    broadcast_tx: broadcast::Sender<SignalingEvent>,

    send_tx: Arc<Mutex<Option<mpsc::Sender<tungstenite::Message>>>>,

    custom_profile: bool,
    position_id: Arc<RwLock<Option<PositionId>>>,

    login_timeout: Duration,
    reconnect_max_tries: u8,
    reconnect_gate: Arc<Mutex<ReconnectGate>>,
    on_terminate_session: Option<OnTerminateSessionCb>,

    worker_tasks: Arc<Mutex<JoinSet<()>>>,
}

impl<ST: SignalingTransport, TP: TokenProvider> SignalingClientInner<ST, TP> {
    #[allow(clippy::too_many_arguments)]
    #[instrument(level = "debug", skip_all)]
    fn new(
        transport: ST,
        token_provider: TP,
        on_event: OnEventCb,
        shutdown_token: CancellationToken,
        custom_profile: bool,
        login_timeout: Duration,
        reconnect_max_tries: u8,
        on_terminate_session: Option<OnTerminateSessionCb>,
    ) -> Self {
        let (state_tx, state_rx) = watch::channel(State::Disconnected);
        Self {
            transport,
            token_provider,

            on_event,

            state_tx,
            state_rx,

            disconnect_token: Arc::new(Mutex::new(shutdown_token.child_token())),
            shutdown_token,

            matcher: ResponseMatcher::new(),
            broadcast_tx: broadcast::channel(BROADCAST_CHANNEL_SIZE).0,

            send_tx: Arc::new(Mutex::new(None)),

            custom_profile,
            position_id: Arc::new(RwLock::new(None)),

            login_timeout,
            reconnect_max_tries,
            reconnect_gate: Arc::new(Mutex::new(ReconnectGate::default())),
            on_terminate_session,

            worker_tasks: Arc::new(Mutex::new(JoinSet::new())),
        }
    }

    fn matcher(&self) -> &ResponseMatcher {
        &self.matcher
    }

    /// Subscribes to a broadcast channel emitting [`SignalingEvent`]s.
    fn subscribe(&self) -> broadcast::Receiver<SignalingEvent> {
        self.broadcast_tx.subscribe()
    }

    /// Subscribes to a watch containing the current [`SignalingClient`] [`State`].
    fn subscribe_state(&self) -> watch::Receiver<State> {
        self.state_tx.subscribe()
    }

    fn state(&self) -> State {
        *self.state_rx.borrow()
    }

    #[instrument(level = "debug", skip_all)]
    async fn disconnect(&self, requested: bool) {
        if self.state() != State::Disconnected {
            tracing::trace!(?requested, "Sending logout message before disconnecting");
            if let Err(err) = self.send(ClientMessage::Logout).await {
                tracing::warn!(?err, "Failed to send Logout message before disconnecting");
            }
        }
        self.disconnect_token.lock().cancel();
        self.set_state(State::Disconnected);
        self.cleanup().await;
        if requested {
            self.reconnect_gate.lock().clear();
        }
    }

    #[instrument(level = "debug", skip(self, msg), fields(message_type = msg.variant()), err)]
    pub async fn send(&self, msg: ClientMessage) -> Result<(), SignalingError> {
        match self.state() {
            State::Disconnected => {
                tracing::warn!("Tried to send message before signaling client was started");
                return Err(SignalingError::Runtime(
                    SignalingRuntimeError::Disconnected(None),
                ));
            }
            State::Connected if !matches!(msg, ClientMessage::Login { .. }) => {
                tracing::warn!("Tried to send message before login");
                return Err(SignalingError::Runtime(
                    SignalingRuntimeError::Disconnected(None),
                ));
            }
            _ => {}
        };

        let send_tx = {
            self.send_tx.lock().as_ref().cloned().ok_or_else(|| {
                tracing::error!("Client is connected, but send_tx is not initialized");
                SignalingError::Runtime(SignalingRuntimeError::Disconnected(None))
            })?
        };

        let serialized = ClientMessage::serialize(&msg).map_err(|err| {
            tracing::warn!(?err, "Failed to serialize message");
            SignalingError::Runtime(SignalingRuntimeError::SerializationError(err.to_string()))
        })?;

        send_tx
            .send(tungstenite::Message::from(serialized))
            .await
            .map_err(|_| SignalingError::Runtime(SignalingRuntimeError::Disconnected(None)))
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn recv(&self) -> Result<ServerMessage, SignalingError> {
        self.recv_with_timeout(Duration::MAX).await
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn recv_with_timeout(&self, timeout: Duration) -> Result<ServerMessage, SignalingError> {
        let mut broadcast_rx = self.subscribe();

        if self.state() == State::Disconnected {
            tracing::warn!("Tried to receive message without transport being connected");
            return Err(SignalingError::Runtime(
                SignalingRuntimeError::Disconnected(None),
            ));
        }

        let disconnect_token = self.disconnect_token.lock().clone();
        let recv_result = tokio::select! {
            biased;
            _ = disconnect_token.cancelled() => {
                tracing::debug!("Shutdown signal received, aborting receive");
                return Err(SignalingError::Timeout("Shutdown signal received".to_string()))
            }
            res = tokio::time::timeout(timeout, async {
                loop {
                    match broadcast_rx.recv().await {
                        Ok(SignalingEvent::Message(msg)) => return Ok(msg),
                        Err(err) => return Err(err),
                        _ => continue,
                    }
                }
            }) => res,
        };

        match recv_result {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(err)) => Err(SignalingError::Other(err.to_string())),
            Err(_) => {
                tracing::warn!("Timeout waiting for message");
                Err(SignalingError::Timeout(
                    "Timeout waiting for message".to_string(),
                ))
            }
        }
    }

    fn set_position_id(&self, position_id: Option<PositionId>) {
        *self.position_id.write() = position_id;
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn login(
        &self,
    ) -> Result<(ClientInfo, ActiveProfile<Profile>, Vec<StationId>), SignalingError> {
        tracing::trace!("Retrieving auth token from token provider");
        let token = self.token_provider.get_token().await?;

        let position_id = self.position_id.read().clone();
        tracing::debug!("Sending Login message to server");
        self.send(
            client::Login {
                token: token.to_string(),
                protocol_version: VACS_PROTOCOL_VERSION.to_string(),
                custom_profile: self.custom_profile,
                position_id,
            }
            .into(),
        )
        .await?;

        tracing::debug!("Awaiting authentication response from server");
        match self.recv_with_timeout(self.login_timeout).await? {
            ServerMessage::SessionInfo(server::SessionInfo {
                client,
                profile,
                default_call_sources,
            }) => {
                if let SessionProfile::Changed(profile) = profile {
                    tracing::info!(?client, %profile, "Login successful, received session info");
                    Ok((client, profile, default_call_sources))
                } else {
                    tracing::error!(
                        ?client,
                        ?profile,
                        "Login successful, but received unexpected session profile"
                    );
                    Err(SignalingError::ProtocolError(
                        "Expected active profile after Login".to_string(),
                    ))
                }
            }
            ServerMessage::LoginFailure(failure) => {
                tracing::warn!(reason = ?failure.reason, "Login failed");
                Err(SignalingError::LoginError(failure.reason))
            }
            ServerMessage::Error(error) => {
                tracing::error!(reason = ?error.reason, client_id = ?error.client_id, "Server returned error");
                Err(SignalingError::Runtime(SignalingRuntimeError::ServerError(
                    error.reason,
                )))
            }
            other => {
                tracing::error!(?other, "Received unexpected message from server");
                Err(SignalingError::ProtocolError(
                    "Expected own session info after Login".to_string(),
                ))
            }
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    pub async fn connect(&self) -> Result<(), SignalingError> {
        tracing::trace!("Connecting to signaling server");
        let (sender, receiver) = self.transport.connect().await?;

        let (send_tx, send_rx) = mpsc::channel::<tungstenite::Message>(SEND_CHANNEL_SIZE);
        tracing::trace!("Successfully connected to signaling server, starting worker tasks");
        {
            let mut tasks = self.worker_tasks.lock();
            let rt_handle = tokio::runtime::Handle::current();

            let matcher = self.matcher.clone();
            let broadcast_tx = self.broadcast_tx.clone();
            tasks.spawn_on(
                Self::reader_task(
                    receiver,
                    send_tx.clone(),
                    matcher,
                    broadcast_tx,
                    self.disconnect_token.lock().clone(),
                    self.subscribe_state(),
                ),
                &rt_handle,
            );

            let broadcast_tx = self.broadcast_tx.clone();
            tasks.spawn_on(
                Self::writer_task(
                    sender,
                    send_rx,
                    broadcast_tx,
                    self.disconnect_token.lock().clone(),
                    self.subscribe_state(),
                ),
                &rt_handle,
            );
        }

        *self.send_tx.lock() = Some(send_tx);
        self.set_state(State::Connected);

        tracing::trace!("Successfully started worker tasks, logging in");
        match self.login().await {
            Ok((client_info, profile, default_call_sources)) => {
                tracing::trace!("Successfully logged in to server");

                self.set_state(State::LoggedIn);
                if let Err(err) = self.broadcast_tx.send(SignalingEvent::Connected {
                    client_info,
                    profile,
                    default_call_sources,
                }) {
                    tracing::warn!(?err, "Failed to broadcast connected event");
                }

                Ok(())
            }
            Err(err) => {
                tracing::warn!(?err, "Failed to login to server");
                self.disconnect(false).await;
                Err(err)
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    async fn cleanup(&self) {
        let mut worker_tasks = {
            let mut worker_tasks = self.worker_tasks.lock();
            std::mem::replace(&mut *worker_tasks, JoinSet::new())
        };

        tracing::trace!("Aborting worker tasks");
        worker_tasks.abort_all();
        tracing::trace!("Waiting for worker tasks to finish");
        while let Some(res) = worker_tasks.join_next().await {
            if let Err(err) = res
                && !err.is_cancelled()
            {
                tracing::warn!(?err, "Failed to join worker task");
            }
        }

        self.matcher.clear().await;
        *self.disconnect_token.lock() = self.shutdown_token.child_token();
        self.send_tx.lock().take();

        tracing::debug!("Finished cleaning up after disconnect");
    }

    #[instrument(level = "debug", skip(self))]
    async fn supervisor_task(self: Arc<Self>) {
        tracing::debug!("Starting supervisor task");

        let mut broadcast_rx = self.subscribe();

        loop {
            tokio::select! {
                biased;

                _ = self.shutdown_token.cancelled() => {
                    tracing::debug!("Shutdown signal received, exiting supervisor task");
                    self.set_state(State::Disconnected);
                    break;
                }

                event = broadcast_rx.recv() => {
                    match event {
                        Ok(event) => {
                            if let SignalingEvent::Error(err) = &event && err.is_fatal() {
                                (self.on_event)(event.clone()).await;

                                tracing::debug!(?err, "Received error event, disconnecting");
                                self.disconnect(false).await;

                                if err.can_reconnect() {
                                    {
                                        let mut gate = self.reconnect_gate.lock();
                                        if let Err(until) = gate.can_reconnect(Instant::now()) {
                                            tracing::warn!(?until, "Reconnect suppressed due to rapid failures");
                                            if let Err(err) = self.broadcast_tx.send(SignalingEvent::Error(SignalingRuntimeError::ReconnectSuppressed(UntilInstant(until)))) {
                                                tracing::warn!(?err, "Failed to broadcast reconnect suppressed error event");
                                            }
                                            gate.clear();
                                            continue;
                                        }

                                        gate.on_reconnect(Instant::now());
                                    }

                                    let terminate = err.needs_session_terminate();
                                    tracing::info!(?terminate, "Reconnecting after error");
                                    if let Err(err) = self.reconnect(terminate).await {
                                        tracing::warn!(?err, "Received error while reconnecting");
                                        if let Err(err) = self.broadcast_tx.send(SignalingEvent::Error(err)) {
                                            tracing::warn!(?err, "Failed to broadcast reconnect error event");
                                        }
                                    }
                                }
                            } else {
                                (self.on_event)(event).await;
                            }
                        },
                        Err(err) => {
                            tracing::warn!(?err, "Failed to receive broadcast event, exiting supervisor task");
                            self.disconnect(false).await;
                            break;
                        }
                    }
                }
            }
        }

        tracing::debug!("Supervisor task finished");
    }

    fn set_state(&self, state: State) {
        if let Err(err) = self.state_tx.send(state) {
            tracing::warn!(?err, "Failed to update client state");
        }
    }

    #[instrument(level = "debug", skip(self), err)]
    async fn reconnect(&self, terminate: bool) -> Result<(), SignalingRuntimeError> {
        if self.reconnect_max_tries == 0 {
            tracing::debug!("Reconnecting disabled");
            return Ok(());
        }

        let mut retry_strategy = RetryStrategy::default();

        let mut reconnect_error = SignalingError::Other("Unknown".to_string());
        for attempt in 1..=self.reconnect_max_tries {
            if terminate && let Some(ref cb) = self.on_terminate_session {
                tracing::debug!(?attempt, "Terminating session before reconnect");
                cb().await;
            }

            tracing::trace!(?attempt, "Reconnecting");
            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    tracing::warn!(?err, ?attempt, "Failed to reconnect");
                    reconnect_error = err;

                    if attempt < self.reconnect_max_tries {
                        let timeout = retry_strategy.timeout(attempt as u32);
                        tracing::debug!(?attempt, ?timeout, "Sleeping before attempting reconnect");

                        tokio::select! {
                            biased;
                            _ = self.shutdown_token.cancelled() => {
                                tracing::debug!("Shutdown signal received, aborting reconnect");
                                return Ok(());
                            }
                            _ = tokio::time::sleep(timeout) => {}
                        }
                    }
                }
            }
        }

        Err(SignalingRuntimeError::ReconnectFailed(
            reconnect_error.into(),
        ))
    }

    #[instrument(level = "debug", skip(state_rx, broadcast_tx))]
    fn emit_task_error(
        state_rx: &watch::Receiver<State>,
        broadcast_tx: &broadcast::Sender<SignalingEvent>,
        err: SignalingRuntimeError,
    ) {
        let state = *state_rx.borrow();
        tracing::warn!(?state, "Received error from task");
        if state == State::LoggedIn
            && broadcast_tx
                .send(SignalingEvent::Error(err.clone()))
                .is_err()
        {
            tracing::warn!("Failed to broadcast task error signaling event");
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn reader_task<R: SignalingReceiver>(
        mut receiver: R,
        send_tx: mpsc::Sender<tungstenite::Message>,
        matcher: ResponseMatcher,
        broadcast_tx: broadcast::Sender<SignalingEvent>,
        disconnect_token: CancellationToken,
        state_rx: watch::Receiver<State>,
    ) -> impl Future<Output = ()> + Send {
        async move {
            tracing::debug!("Starting transport reader task");
            let _guard = TaskDropLogger::new("reader");

            loop {
                tokio::select! {
                    biased;

                    _ = disconnect_token.cancelled() => {
                        tracing::debug!("Disconnect signal received, exiting transport reader task");
                        break;
                    }

                    msg = receiver.recv(&send_tx) => {
                        match msg {
                            Ok(message) => {
                                tracing::trace!(message_type = message.variant(), "Received message from transport");
                                matcher.try_match(&message);
                                if broadcast_tx.receiver_count() > 0 {
                                    if let Err(err) = broadcast_tx.send(SignalingEvent::Message(message.clone())) {
                                        tracing::warn!(message_type = message.variant(), ?err, "Failed to broadcast message");
                                    }
                                } else {
                                    tracing::trace!(message_type = message.variant(), "No receivers subscribed, not broadcasting message");
                                }
                            }
                            Err(err) => {
                                Self::emit_task_error(&state_rx, &broadcast_tx, err);
                                break;
                            }
                        }
                    }
                }
            }
        }.instrument(tracing::Span::current())
    }

    #[instrument(level = "debug", skip_all)]
    fn writer_task<S: SignalingSender>(
        mut sender: S,
        mut send_rx: mpsc::Receiver<tungstenite::Message>,
        broadcast_tx: broadcast::Sender<SignalingEvent>,
        disconnect_token: CancellationToken,
        state_rx: watch::Receiver<State>,
    ) -> impl Future<Output = ()> + Send {
        async move {
            tracing::debug!("Starting transport writer task");
            let _guard = TaskDropLogger::new("writer");

            loop {
                tokio::select! {
                    biased;

                    _ = disconnect_token.cancelled() => {
                        tracing::debug!("Disconnect signal received, closing sender");

                        if let Err(err) = sender.close().await {
                            tracing::warn!(?err, "Failed to close transport");
                        }

                        tracing::debug!("Successfully closed sender, exiting transport writer task");
                        break;
                    }

                    msg = send_rx.recv() => {
                        match msg {
                            Some(msg) => {
                                if !matches!(msg, tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)) {
                                    tracing::trace!("Sending message to transport");
                                }

                                if let Err(err) = sender.send(msg).await {
                                    Self::emit_task_error(&state_rx, &broadcast_tx, err);
                                    break;
                                }
                            },
                            None => {
                                Self::emit_task_error(&state_rx, &broadcast_tx, SignalingRuntimeError::Disconnected(None));
                                break;
                            }
                        }
                    }
                }
            }
        }.instrument(tracing::Span::current())
    }
}

struct TaskDropLogger {
    name: &'static str,
}

impl TaskDropLogger {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl Drop for TaskDropLogger {
    fn drop(&mut self) {
        tracing::trace!(task_name = ?self.name, "Task dropped");
    }
}

pub struct RetryStrategy {
    base: Duration,
    cap: Duration,
    rng: rand::rngs::StdRng,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(100),
            cap: Duration::from_secs(5),
            rng: rand::make_rng(),
        }
    }
}

impl RetryStrategy {
    fn timeout(&mut self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        // exp = base * 2^(attempt - 1), capped
        let exp_nanos = self
            .base
            .as_nanos()
            .saturating_mul(1u128 << attempt.saturating_sub(1).min(63));
        let max_delay_nanos = exp_nanos.min(self.cap.as_nanos());

        let jitter_nanos = if max_delay_nanos == 0 {
            0
        } else {
            // full jitter
            self.rng.random_range(0..=max_delay_nanos)
        };

        Duration::from_nanos(jitter_nanos.min(u128::from(u64::MAX)) as u64)
    }
}

pub struct ReconnectGate {
    max_in_window: u32,
    window: Duration,
    cooldown: Duration,

    recent: VecDeque<Instant>,
    suppressed_until: Option<Instant>,
}

impl Default for ReconnectGate {
    fn default() -> Self {
        Self::new(3, Duration::from_secs(60), Duration::from_secs(120))
    }
}

impl ReconnectGate {
    fn new(max_in_window: u32, window: Duration, cooldown: Duration) -> Self {
        assert!(max_in_window > 0, "threshold must be greater than 0");
        Self {
            window,
            max_in_window,
            cooldown,
            recent: VecDeque::new(),
            suppressed_until: None,
        }
    }

    #[inline]
    fn prune(&mut self, now: Instant) {
        while self
            .recent
            .front()
            .is_some_and(|t| now.duration_since(*t) > self.window)
        {
            self.recent.pop_front();
        }
    }

    fn can_reconnect(&mut self, now: Instant) -> Result<(), Instant> {
        if let Some(until) = self.suppressed_until {
            if now < until {
                return Err(until);
            }
            self.suppressed_until = None;
        }

        self.prune(now);

        if self.recent.len() >= self.max_in_window as usize {
            let until = now + self.cooldown;
            self.suppressed_until = Some(until);
            return Err(until);
        }
        Ok(())
    }

    fn on_reconnect(&mut self, now: Instant) {
        self.recent.push_back(now);
        self.prune(now);
    }

    fn clear(&mut self) {
        self.recent.clear();
        self.suppressed_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::mock::MockTokenProvider;
    use crate::test_utils::RecvWithTimeoutExt;
    use crate::transport::mock::MockTransport;
    use pretty_assertions::{assert_eq, assert_matches};
    use test_log::test;
    use tokio::sync::Notify;
    use vacs_protocol::vatsim::{ClientId, PositionId};
    use vacs_protocol::ws::server::LoginFailureReason;
    use vacs_protocol::ws::shared::ErrorReason;

    async fn setup_test_client(
        transport: MockTransport,
        custom_profile: bool,
        reconnect_max_tries: u8,
    ) -> (
        Arc<SignalingClient<MockTransport, MockTokenProvider>>,
        CancellationToken,
    ) {
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::SessionInfo(server::SessionInfo {
                    client: ClientInfo {
                        id: ClientId::from("client1"),
                        position_id: Some(PositionId::from("position1")),
                        display_name: "Client 1".into(),
                        frequency: "100.000".into(),
                    },
                    profile: SessionProfile::Changed(ActiveProfile::Specific(Profile {
                        id: vacs_protocol::profile::ProfileId::from("1"),
                        profile_type: vacs_protocol::profile::ProfileType::Tabbed(vec![]),
                    })),
                    default_call_sources: Vec::new(),
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            custom_profile,
            Duration::from_millis(100),
            reconnect_max_tries,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_ok());
        assert_matches!(client.state(), State::LoggedIn);

        (Arc::new(client), shutdown_token)
    }

    #[test(tokio::test)]
    async fn connect() {
        setup_test_client(MockTransport::default(), false, 0).await;
    }

    #[test(tokio::test)]
    async fn shutdown() {
        let (client, shutdown_token) = setup_test_client(MockTransport::default(), false, 0).await;

        shutdown_token.cancel();

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn disconnect() {
        let (client, _shutdown_token) = setup_test_client(MockTransport::default(), false, 0).await;

        client.disconnect().await;

        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn send() {
        let transport = MockTransport::default();
        let mut outgoing_rx = transport.outgoing_tx.subscribe();
        let (client, _shutdown_token) = setup_test_client(transport, false, 0).await;

        let msg = ClientMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
            call_id: vacs_protocol::ws::shared::CallId::new(),
            source: vacs_protocol::ws::shared::CallSource {
                client_id: ClientId::from("client1"),
                position_id: None,
                station_id: None,
            },
            target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
            prio: false,
        });
        let serialized = tungstenite::Message::from(ClientMessage::serialize(&msg).unwrap());

        let result = client.send(msg.clone()).await;
        assert!(result.is_ok());

        let sent_msg = outgoing_rx
            .recv_with_timeout(Duration::from_millis(100), |m| m == &serialized)
            .await;
        assert!(sent_msg.is_ok());
    }

    #[test(tokio::test)]
    async fn send_without_start() {
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let client = SignalingClient::new(
            MockTransport::default(),
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            8,
            None,
            &tokio::runtime::Handle::current(),
        );

        let msg = ClientMessage::Login(client::Login {
            token: "test".to_string(),
            protocol_version: VACS_PROTOCOL_VERSION.to_string(),
            custom_profile: false,
            position_id: None,
        });

        let result = client.send(msg.clone()).await;
        assert_matches!(
            result,
            Err(SignalingError::Runtime(
                SignalingRuntimeError::Disconnected(None)
            ))
        );
    }

    #[test(tokio::test)]
    async fn send_without_login() {
        let transport = MockTransport::default();
        let transport_ready = transport.ready.clone();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, Some(Duration::from_millis(100)));

        let client = Arc::new(SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            8,
            None,
            &tokio::runtime::Handle::current(),
        ));

        let client_clone = client.clone();
        tokio::spawn(async move {
            transport_ready.notified().await;
            let msg = ClientMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
                call_id: vacs_protocol::ws::shared::CallId::new(),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: ClientId::from("client1"),
                    position_id: None,
                    station_id: None,
                },
                target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
                prio: false,
            });

            let result = client_clone.send(msg.clone()).await;
            assert_matches!(
                result,
                Err(SignalingError::Runtime(
                    SignalingRuntimeError::Disconnected(None)
                ))
            );
        });

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(res.unwrap_err(), SignalingError::Timeout(_));
    }

    #[test(tokio::test)]
    async fn send_disconnected() {
        let (client, _shutdown_token) = setup_test_client(MockTransport::default(), false, 0).await;

        client.disconnect().await;

        assert_matches!(client.state(), State::Disconnected);

        let msg = ClientMessage::Login(client::Login {
            token: "test".to_string(),
            protocol_version: VACS_PROTOCOL_VERSION.to_string(),
            custom_profile: false,
            position_id: None,
        });

        let result = client.send(msg.clone()).await;
        assert_matches!(
            result,
            Err(SignalingError::Runtime(
                SignalingRuntimeError::Disconnected(None)
            ))
        );
    }

    #[test(tokio::test)]
    async fn send_shutdown() {
        let (client, shutdown_token) = setup_test_client(MockTransport::default(), false, 0).await;

        shutdown_token.cancel();

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_matches!(client.state(), State::Disconnected);

        let msg = ClientMessage::Login(client::Login {
            token: "test".to_string(),
            protocol_version: VACS_PROTOCOL_VERSION.to_string(),
            custom_profile: false,
            position_id: None,
        });

        let result = client.send(msg.clone()).await;
        assert_matches!(
            result,
            Err(SignalingError::Runtime(
                SignalingRuntimeError::Disconnected(None)
            ))
        );
    }

    #[test(tokio::test)]
    async fn recv() {
        let transport = MockTransport::default();
        let incoming_tx = transport.incoming_tx.clone();
        let (client, _shutdown_token) = setup_test_client(transport, false, 0).await;

        let msg = ServerMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
            call_id: vacs_protocol::ws::shared::CallId::new(),
            source: vacs_protocol::ws::shared::CallSource {
                client_id: ClientId::from("client1"),
                position_id: None,
                station_id: None,
            },
            target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
            prio: false,
        });

        let task = tokio::spawn(async move {
            return client.recv().await;
        });

        let result = incoming_tx.send(tungstenite::Message::from(
            ServerMessage::serialize(&msg).unwrap(),
        ));
        assert!(result.is_ok());

        assert_eq!(task.await.unwrap().unwrap(), msg);
    }

    #[test(tokio::test)]
    async fn recv_shutdown() {
        let transport = MockTransport::default();
        let (client, shutdown_token) = setup_test_client(transport, false, 0).await;

        let ready = Arc::new(Notify::new());
        let ready_clone = ready.clone();
        let task = tokio::spawn(async move {
            ready_clone.notify_one();
            let res = client.recv().await;
            assert!(res.is_err());
            assert_matches!(
                res.unwrap_err(),
                SignalingError::Timeout(msg) if msg == "Shutdown signal received"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_matches!(client.state(), State::Disconnected);
        });

        ready.notified().await;
        shutdown_token.cancel();

        task.await.unwrap();
    }

    #[test(tokio::test)]
    async fn recv_with_timeout() {
        let transport = MockTransport::default();
        let incoming_tx = transport.incoming_tx.clone();
        let (client, _shutdown_token) = setup_test_client(transport, false, 0).await;

        let msg = ServerMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
            call_id: vacs_protocol::ws::shared::CallId::new(),
            source: vacs_protocol::ws::shared::CallSource {
                client_id: ClientId::from("client1"),
                position_id: None,
                station_id: None,
            },
            target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
            prio: false,
        });

        let task = tokio::spawn(async move {
            return client.recv_with_timeout(Duration::from_millis(100)).await;
        });

        let result = incoming_tx.send(tungstenite::Message::from(
            ServerMessage::serialize(&msg).unwrap(),
        ));
        assert!(result.is_ok());

        assert_eq!(task.await.unwrap().unwrap(), msg);
    }

    #[test(tokio::test)]
    async fn recv_with_timeout_expired() {
        let transport = MockTransport::default();
        let incoming_tx = transport.incoming_tx.clone();
        let (client, _shutdown_token) = setup_test_client(transport, false, 0).await;

        let msg = ServerMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
            call_id: vacs_protocol::ws::shared::CallId::new(),
            source: vacs_protocol::ws::shared::CallSource {
                client_id: ClientId::from("client1"),
                position_id: None,
                station_id: None,
            },
            target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
            prio: false,
        });

        let client_clone = client.clone();
        let task = tokio::spawn(async move {
            return client_clone
                .recv_with_timeout(Duration::from_millis(10))
                .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        incoming_tx
            .send(tungstenite::Message::from(
                ServerMessage::serialize(&msg).unwrap(),
            ))
            .unwrap();

        let recv_result = task.await.unwrap();
        assert!(recv_result.is_err());
        assert_eq!(
            recv_result.unwrap_err().to_string(),
            "timeout: Timeout waiting for message".to_string()
        );
    }

    #[test(tokio::test)]
    async fn recv_connection_closed() {
        let transport = MockTransport::default();
        let transport_disconnect_token = transport.disconnect_token();
        let (client, _shutdown_token) = setup_test_client(transport, false, 0).await;

        transport_disconnect_token.cancel();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_matches!(client.state(), State::Disconnected);

        let recv_result = client.recv().await;
        assert!(recv_result.is_err());
        assert_matches!(
            recv_result,
            Err(SignalingError::Runtime(
                SignalingRuntimeError::Disconnected(None)
            ))
        );
    }

    #[test(tokio::test)]
    async fn login_client_timeout() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::LoginFailure(server::LoginFailure {
                    reason: LoginFailureReason::Timeout,
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(
            res.unwrap_err(),
            SignalingError::LoginError(LoginFailureReason::Timeout)
        );
        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn login_server_timeout() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let client = Arc::new(SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        ));

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(res.unwrap_err(), SignalingError::Timeout(_));
    }

    #[test(tokio::test)]
    async fn login_unauthorized() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::LoginFailure(server::LoginFailure {
                    reason: LoginFailureReason::Unauthorized,
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(
            res.unwrap_err(),
            SignalingError::LoginError(LoginFailureReason::Unauthorized)
        );
        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn login_invalid_credentials() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::LoginFailure(server::LoginFailure {
                    reason: LoginFailureReason::InvalidCredentials,
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(
            res.unwrap_err(),
            SignalingError::LoginError(LoginFailureReason::InvalidCredentials)
        );
        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn login_duplicate_id() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::LoginFailure(server::LoginFailure {
                    reason: LoginFailureReason::DuplicateId,
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(
            res.unwrap_err(),
            SignalingError::LoginError(LoginFailureReason::DuplicateId)
        );
        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn login_unexpected_message() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::WebrtcAnswer(
                    vacs_protocol::ws::shared::WebrtcAnswer {
                        call_id: vacs_protocol::ws::shared::CallId::new(),
                        from_client_id: ClientId::from("client1"),
                        to_client_id: ClientId::from("client2"),
                        sdp: "sdp2".to_string(),
                    },
                ))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(res.unwrap_err(), SignalingError::ProtocolError(reason) if reason == "Expected own session info after Login");
        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn login_server_error() {
        let transport = MockTransport::default();
        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::Error(vacs_protocol::ws::shared::Error {
                    reason: ErrorReason::Internal("something failed".to_string()),
                    client_id: None,
                    call_id: None,
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            0,
            None,
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_err());
        assert_matches!(res.unwrap_err(), SignalingError::Runtime(SignalingRuntimeError::ServerError(ErrorReason::Internal(reason))) if reason == "something failed");
        assert_matches!(client.state(), State::Disconnected);
    }

    #[test(tokio::test)]
    async fn on_terminate_session_called_on_disconnect() {
        let transport = MockTransport::default();
        let incoming_tx = transport.incoming_tx.clone();
        let outgoing_tx = transport.outgoing_tx.clone();

        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        let session_info_msg = || {
            tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::SessionInfo(server::SessionInfo {
                    client: ClientInfo {
                        id: ClientId::from("client1"),
                        position_id: Some(PositionId::from("position1")),
                        display_name: "Client 1".into(),
                        frequency: "100.000".into(),
                    },
                    profile: SessionProfile::Changed(ActiveProfile::Specific(Profile {
                        id: vacs_protocol::profile::ProfileId::from("1"),
                        profile_type: vacs_protocol::profile::ProfileType::Tabbed(vec![]),
                    })),
                    default_call_sources: Vec::new(),
                }))
                .unwrap()
                .into(),
            )
        };

        let session_info = session_info_msg();
        tokio::spawn(async move {
            ready.notified().await;
            let _ = mock_tx.send(session_info);
        });

        let hook_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook_called_clone = hook_called.clone();

        let on_terminate_session: OnTerminateSessionCb = Arc::new(move || {
            let called = hook_called_clone.clone();
            Box::pin(async move {
                called.store(true, std::sync::atomic::Ordering::SeqCst);
            })
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(500),
            1,
            Some(on_terminate_session),
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_ok());
        assert_matches!(client.state(), State::LoggedIn);

        let session_info = session_info_msg();
        let reconnect_incoming_tx = incoming_tx.clone();
        let mut outgoing_rx = outgoing_tx.subscribe();
        tokio::spawn(async move {
            loop {
                if let Ok(tungstenite::Message::Text(text)) = outgoing_rx.recv().await
                    && text.contains("\"login\"")
                {
                    let _ = reconnect_incoming_tx.send(session_info);
                    break;
                }
            }
        });

        // Simulate a transport disconnection by sending a Close frame
        let _ = incoming_tx.send(tungstenite::Message::Close(None));
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            hook_called.load(std::sync::atomic::Ordering::SeqCst),
            "on_terminate_session hook should have been called"
        );
        assert_matches!(client.state(), State::LoggedIn);

        shutdown_token.cancel();
    }

    #[test(tokio::test)]
    async fn on_terminate_session_called_on_transport_disconnect() {
        let transport = MockTransport::default();
        let transport_disconnect_token = transport.disconnect_token();

        let shutdown_token = CancellationToken::new();
        let token_provider = MockTokenProvider::new(1, None);

        let mock_tx = transport.incoming_tx.clone();
        let ready = transport.ready.clone();

        tokio::spawn(async move {
            ready.notified().await;
            let msg = tungstenite::Message::Text(
                ServerMessage::serialize(&ServerMessage::SessionInfo(server::SessionInfo {
                    client: ClientInfo {
                        id: ClientId::from("client1"),
                        position_id: Some(PositionId::from("position1")),
                        display_name: "Client 1".into(),
                        frequency: "100.000".into(),
                    },
                    profile: SessionProfile::Changed(ActiveProfile::Specific(Profile {
                        id: vacs_protocol::profile::ProfileId::from("1"),
                        profile_type: vacs_protocol::profile::ProfileType::Tabbed(vec![]),
                    })),
                    default_call_sources: Vec::new(),
                }))
                .unwrap()
                .into(),
            );
            let _ = mock_tx.send(msg);
        });

        let hook_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook_called_clone = hook_called.clone();

        let on_terminate_session: OnTerminateSessionCb = Arc::new(move || {
            let called = hook_called_clone.clone();
            Box::pin(async move {
                called.store(true, std::sync::atomic::Ordering::SeqCst);
            })
        });

        let client = SignalingClient::new(
            transport,
            token_provider,
            |_| async {},
            shutdown_token.clone(),
            false,
            Duration::from_millis(100),
            1,
            Some(on_terminate_session),
            &tokio::runtime::Handle::current(),
        );

        let res = client.connect(None).await;
        assert!(res.is_ok());
        assert_matches!(client.state(), State::LoggedIn);

        // Simulate a full transport disconnection (e.g. network loss).
        // This also poisons new connections from the same MockTransport, so
        // the reconnect attempt will fail - but the hook must still fire.
        transport_disconnect_token.cancel();
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert!(
            hook_called.load(std::sync::atomic::Ordering::SeqCst),
            "on_terminate_session hook should have been called"
        );

        shutdown_token.cancel();
    }

    mod reconnect_gate {
        use super::super::*;
        use pretty_assertions::assert_eq;
        use test_log::test;

        fn gate(max_in_window: u32, window_s: u64, cooldown_s: u64) -> ReconnectGate {
            ReconnectGate::new(
                max_in_window,
                Duration::from_secs(window_s),
                Duration::from_secs(cooldown_s),
            )
        }

        #[test]
        fn allows_until_limit() {
            let mut g = gate(3, 10, 30);
            let t0 = Instant::now();

            assert!(g.can_reconnect(t0).is_ok());
            g.on_reconnect(t0);
            assert_eq!(g.recent.len(), 1);

            let t1 = t0 + Duration::from_secs(1);
            assert!(g.can_reconnect(t1).is_ok());
            g.on_reconnect(t1);
            assert_eq!(g.recent.len(), 2);

            let t2 = t0 + Duration::from_secs(2);
            assert!(g.can_reconnect(t2).is_ok());
            g.on_reconnect(t2);
            assert_eq!(g.recent.len(), 3);

            let t3 = t0 + Duration::from_secs(3);
            let until = g.can_reconnect(t3).unwrap_err();
            assert_eq!(until, t3 + Duration::from_secs(30));
            assert_eq!(g.suppressed_until, Some(until));

            assert_eq!(g.recent.len(), 3);
        }

        #[test]
        fn enforces_suppression_until_cooldown_expires() {
            let mut g = gate(1, 10, 30);
            let t0 = Instant::now();

            assert!(g.can_reconnect(t0).is_ok());
            g.on_reconnect(t0);
            assert_eq!(g.recent.len(), 1);

            let t1 = t0 + Duration::from_secs(1);
            let until = g.can_reconnect(t1).unwrap_err();
            assert_eq!(until, t1 + Duration::from_secs(30));
            assert_eq!(g.suppressed_until, Some(until));

            let mid = t1 + Duration::from_secs(10);
            let mid_err = g.can_reconnect(mid).unwrap_err();
            assert_eq!(mid_err, until);
            assert_eq!(g.suppressed_until, Some(until));

            let t_after = until;
            assert!(g.can_reconnect(t_after).is_ok());
            assert_eq!(g.suppressed_until, None);
        }

        #[test]
        fn allows_after_window_prunes() {
            let mut g = gate(1, 10, 30);

            let t0 = Instant::now();
            assert!(g.can_reconnect(t0).is_ok());
            g.on_reconnect(t0);
            assert_eq!(g.recent.len(), 1);

            let t1 = t0 + Duration::from_secs(11);
            assert!(g.can_reconnect(t1).is_ok());
            g.on_reconnect(t1);
            assert_eq!(g.recent.len(), 1);

            let t2 = t1 + Duration::from_secs(1);
            let until = g.can_reconnect(t2).unwrap_err();
            assert_eq!(until, t2 + Duration::from_secs(30));
        }

        #[test]
        fn allow_does_not_record_attempts() {
            let mut g = gate(1, 10, 30);

            let t0 = Instant::now();
            assert!(g.can_reconnect(t0).is_ok());
            assert!(g.can_reconnect(t0 + Duration::from_secs(1)).is_ok());
            assert!(g.can_reconnect(t0 + Duration::from_secs(2)).is_ok());
            assert_eq!(g.recent.len(), 0);
            assert!(g.suppressed_until.is_none());
        }
    }
}
