use crate::config;
use crate::metrics::guards::ClientConnectionGuard;
use crate::state::AppState;
use crate::state::clients::{ClientManagerError, Result};
use crate::ws::application_message::handle_application_message;
use crate::ws::message::{MessageResult, receive_message, send_message};
use crate::ws::traits::{WebSocketSink, WebSocketStream};
use axum::extract::ws;
use futures_util::SinkExt;
use parking_lot::Mutex;
use std::fmt::{Debug, Formatter};
use std::ops::ControlFlow;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{Instrument, instrument};
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{ClientId, PositionId};
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{ClientInfo, DisconnectReason, ServerMessage, SessionProfile};
use vacs_protocol::ws::{server, shared};
use vacs_vatsim::ControllerInfo;
use vacs_vatsim::coverage::network::Network;

#[derive(Clone)]
pub struct ClientSession {
    client_info: ClientInfo,
    active_profile: ActiveProfile<ProfileId>,
    tx: mpsc::Sender<ServerMessage>,
    client_shutdown_tx: watch::Sender<Option<DisconnectReason>>,
    client_connection_guard: Arc<Mutex<ClientConnectionGuard>>,
}

impl ClientSession {
    pub fn new(
        client_info: ClientInfo,
        active_profile: ActiveProfile<ProfileId>,
        tx: mpsc::Sender<ServerMessage>,
        client_connection_guard: ClientConnectionGuard,
    ) -> Self {
        let (client_shutdown_tx, _) = watch::channel(None);
        Self {
            client_info,
            active_profile,
            tx,
            client_shutdown_tx,
            client_connection_guard: Arc::new(Mutex::new(client_connection_guard)),
        }
    }

    #[inline]
    pub fn id(&self) -> &ClientId {
        &self.client_info.id
    }

    #[inline]
    pub fn position_id(&self) -> Option<&PositionId> {
        self.client_info.position_id.as_ref()
    }

    #[inline]
    pub fn client_info(&self) -> &ClientInfo {
        &self.client_info
    }

    #[inline]
    pub fn active_profile(&self) -> &ActiveProfile<ProfileId> {
        &self.active_profile
    }

    #[tracing::instrument(level = "trace")]
    pub fn update_client_info(&mut self, controller_info: &ControllerInfo) -> bool {
        let mut changed = false;
        if self.client_info.display_name != controller_info.callsign {
            tracing::trace!(
                cid = ?self.client_info.id,
                old = ?self.client_info.display_name,
                new = ?controller_info.callsign,
                "Controller callsign changed, updating"
            );
            self.client_info.display_name = controller_info.callsign.clone();
            changed = true;
        }
        if self.client_info.frequency != controller_info.frequency {
            tracing::trace!(
                cid = ?self.client_info.id,
                old = ?self.client_info.frequency,
                new = ?controller_info.frequency,
                "Controller frequency changed, updating"
            );
            self.client_info.frequency = controller_info.frequency.clone();
            changed = true;
        }
        changed
    }

    #[inline]
    pub fn set_position_id(&mut self, position_id: Option<PositionId>) {
        self.client_info.position_id = position_id;
    }

    #[tracing::instrument(level = "trace")]
    pub fn update_active_profile(
        &mut self,
        new_profile_id: Option<ProfileId>,
        network: &Network,
    ) -> SessionProfile {
        match (&self.active_profile, new_profile_id) {
            (ActiveProfile::Specific(old_profile_id), Some(new_profile_id))
                if *old_profile_id == new_profile_id =>
            {
                SessionProfile::Unchanged
            }
            (ActiveProfile::None, None) | (ActiveProfile::Custom, _) => SessionProfile::Unchanged,
            (_, Some(new_profile_id)) => {
                if let Some(profile) = network.get_profile(&new_profile_id) {
                    tracing::trace!(?profile, "Active profile changed, updating");
                    self.active_profile = ActiveProfile::Specific(new_profile_id.clone());
                    SessionProfile::Changed(ActiveProfile::Specific(profile.into()))
                } else {
                    tracing::warn!(
                        ?new_profile_id,
                        "Active profile does not exist, falling back to None"
                    );
                    self.active_profile = ActiveProfile::None;
                    SessionProfile::Changed(ActiveProfile::None)
                }
            }
            (_, None) => {
                tracing::trace!("Active profile cleared, updating");
                self.active_profile = ActiveProfile::None;
                SessionProfile::Changed(ActiveProfile::None)
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    pub fn disconnect(&self, disconnect_reason: Option<DisconnectReason>) {
        tracing::trace!("Disconnecting client");
        if let Some(reason) = &disconnect_reason {
            self.client_connection_guard
                .lock()
                .set_disconnect_reason(reason.clone());
        }
        let _ = self.client_shutdown_tx.send(disconnect_reason);
    }

    #[instrument(level = "trace", skip(self, message), fields(message = tracing::field::Empty), err)]
    pub async fn send_message(&self, message: impl Into<ServerMessage>) -> Result<()> {
        let message = message.into();
        tracing::span::Span::current().record("message", tracing::field::debug(&message));
        self.tx
            .send(message)
            .await
            .map_err(|err| ClientManagerError::MessageSendError(err.to_string()))
    }

    pub async fn send_error(&self, err: impl Into<shared::Error>) {
        let err = err.into();
        if let Err(err) = self.send_message(err).await {
            tracing::warn!(?err, "Failed to send error message");
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[instrument(level = "debug", skip_all, fields(client_id = ?self.client_info.id))]
    pub async fn handle_interaction<R: WebSocketStream + 'static, T: WebSocketSink + 'static>(
        &mut self,
        app_state: &Arc<AppState>,
        websocket_rx: R,
        websocket_tx: T,
        broadcast_rx: &mut broadcast::Receiver<ServerMessage>,
        rx: &mut mpsc::Receiver<ServerMessage>,
        app_shutdown_rx: &mut watch::Receiver<()>,
    ) {
        tracing::debug!("Starting to handle client interaction");

        let (pong_update_tx, pong_update_rx) = watch::channel(Instant::now());

        let (writer_handle, ws_outbound_tx) = ClientSession::spawn_writer(
            websocket_tx,
            app_shutdown_rx.clone(),
            self.client_shutdown_tx.subscribe(),
        )
        .await;
        let (reader_handle, mut ws_inbound_rx) = ClientSession::spawn_reader(
            websocket_rx,
            app_shutdown_rx.clone(),
            self.client_shutdown_tx.subscribe(),
            pong_update_tx,
        )
        .await;
        let (ping_handle, mut ping_shutdown_rx) =
            ClientSession::spawn_ping_task(&ws_outbound_tx, pong_update_rx);

        tracing::trace!("Sending initial session info");
        if let Err(err) = send_message(
            &ws_outbound_tx,
            server::SessionInfo {
                client: self.client_info.clone(),
                profile: match &self.active_profile {
                    ActiveProfile::Specific(profile_id) => {
                        let profile = app_state.clients.get_profile(Some(profile_id));
                        profile
                            .map(|p| SessionProfile::Changed(ActiveProfile::Specific((&p).into())))
                            .unwrap_or(SessionProfile::Changed(ActiveProfile::None))
                    }
                    ActiveProfile::Custom => SessionProfile::Changed(ActiveProfile::Custom),
                    ActiveProfile::None => SessionProfile::Changed(ActiveProfile::None),
                },
                default_call_sources: app_state
                    .clients
                    .get_position(self.position_id())
                    .map(|p| p.default_call_sources.clone())
                    .unwrap_or_default(),
            },
        )
        .await
        {
            tracing::warn!(?err, "Failed to send initial session info");
        }

        tracing::trace!("Sending initial client list");
        let clients = app_state.list_clients(Some(&self.client_info.id)).await;
        if let Err(err) = send_message(&ws_outbound_tx, server::ClientList { clients }).await {
            tracing::warn!(?err, "Failed to send initial client list");
        }

        tracing::trace!("Sending initial stations list");
        let stations = app_state
            .list_stations(&self.active_profile, self.client_info.position_id.as_ref())
            .await;
        if let Err(err) = send_message(&ws_outbound_tx, server::StationList { stations }).await {
            tracing::warn!(?err, "Failed to send initial stations list");
        }

        loop {
            tokio::select! {
                biased;

                _ = app_shutdown_rx.changed() => {
                    tracing::trace!("Shutdown signal received, disconnecting client");
                    break;
                }

                _ = &mut ping_shutdown_rx => {
                    tracing::debug!("Ping task reported client disconnect");
                    break;
                }

                msg = ws_inbound_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            match handle_application_message(app_state, self, msg).await {
                                ControlFlow::Continue(()) => continue,
                                ControlFlow::Break(()) => {
                                    tracing::debug!("Breaking interaction loop");
                                    break;
                                },
                            }
                        }
                        None => {
                            tracing::debug!("Application receiver closed, disconnecting client");
                            break;
                        }
                    }
                }

                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            tracing::trace!("Received direct message");
                            if let Err(err) = send_message(&ws_outbound_tx, msg).await {
                                tracing::warn!(?err, "Failed to send direct message");
                            }
                        }
                        None => {
                            tracing::debug!("Client receiver closed, disconnecting client");
                            break;
                        }
                    }
                }

                msg = broadcast_rx.recv() => {
                    match msg {
                        Ok(msg) => {
                            tracing::trace!("Received broadcast message");
                            if let ServerMessage::ClientInfo(info) = &msg
                                && info.id == self.client_info.id {
                                tracing::trace!("Dropping client info update broadcast for own client");
                                continue;
                            }

                            if let Err(err) = send_message(&ws_outbound_tx, msg).await {
                                tracing::warn!(?err, "Failed to send broadcast message");
                            }
                        }
                        Err(err) => {
                            tracing::debug!(?err, "Broadcast receiver closed, disconnecting client");
                        }
                    }
                }
            }
        }

        writer_handle.abort();
        reader_handle.abort();
        ping_handle.abort();

        tracing::debug!("Finished handling client interaction");
    }

    #[instrument(level = "debug", skip_all)]
    async fn spawn_writer<T: WebSocketSink + 'static>(
        mut websocket_tx: T,
        mut app_shutdown_rx: watch::Receiver<()>,
        mut client_shutdown_rx: watch::Receiver<Option<DisconnectReason>>,
    ) -> (JoinHandle<()>, mpsc::Sender<ws::Message>) {
        let (ws_outbound_tx, mut ws_outbound_rx) =
            mpsc::channel::<ws::Message>(config::CLIENT_WEBSOCKET_TASK_CHANNEL_CAPACITY);

        let join_handle = tokio::spawn(async move {
            tracing::trace!("WebSocket writer task started");
            let _guard = TaskDropLogger::new("writer");

            loop {
                tokio::select! {
                    biased;

                    _ = app_shutdown_rx.changed() => {
                        tracing::trace!("App shutdown signal received, stopping WebSocket writer task");
                        break;
                    }

                    _ = client_shutdown_rx.changed() => {
                        let reason_opt = {
                            client_shutdown_rx.borrow().clone()
                        };

                        if let Some(reason) = reason_opt {
                            tracing::trace!(?reason, "Sending Disconnect message before stopping WebSocket writer task");
                            match ServerMessage::serialize(&ServerMessage::from(server::Disconnected {reason})) {
                                Ok(msg) => {
                                    if let Err(err) = websocket_tx.send(ws::Message::from(msg)).await {
                                        tracing::warn!(?err, "Failed to send Disconnect message");
                                    }
                                },
                                Err(err) => {
                                    tracing::warn!(?err, "Failed to serialize Disconnect message");
                                }
                            }
                        } else {
                            tracing::trace!("Client shutdown signal received, stopping WebSocket writer task");
                        }
                        break;
                    }

                    msg = ws_outbound_rx.recv() => {
                        match msg {
                            Some(msg) => {
                                if let Err(err) = websocket_tx.send(msg).await {
                                    tracing::warn!(?err, "Failed to send message to client");
                                    break;
                                }
                            },
                            None => {
                                tracing::debug!("Outbound WebSocket channel closed, stopping WebSocket writer task");
                                break;
                            }
                        }
                    }
                }
            }

            tracing::trace!("Sending close message to client");
            if let Err(err) = websocket_tx.send(ws::Message::Close(None)).await {
                tracing::warn!(?err, "Failed to send close message to client");
            }

            tracing::trace!("WebSocket writer task finished");
        }.instrument(tracing::Span::current()));

        (join_handle, ws_outbound_tx)
    }

    #[instrument(level = "debug", skip_all)]
    async fn spawn_reader<R: WebSocketStream + 'static>(
        mut websocket_rx: R,
        mut app_shutdown_rx: watch::Receiver<()>,
        mut client_shutdown_rx: watch::Receiver<Option<DisconnectReason>>,
        pong_update_tx: watch::Sender<Instant>,
    ) -> (JoinHandle<()>, mpsc::Receiver<ClientMessage>) {
        let (ws_inbound_tx, ws_inbound_rx) =
            mpsc::channel::<ClientMessage>(config::CLIENT_WEBSOCKET_TASK_CHANNEL_CAPACITY);

        let join_handle = tokio::spawn(async move {
            tracing::trace!("WebSocket reader task started");
            let _guard = TaskDropLogger::new("reader");

            loop {
                tokio::select! {
                    biased;

                    _ = app_shutdown_rx.changed() => {
                        tracing::trace!("App shutdown signal received, stopping WebSocket reader task");
                        break;
                    }

                    _ = client_shutdown_rx.changed() => {
                        tracing::trace!("Client shutdown signal received, stopping WebSocket reader task");
                        break;
                    }

                    msg = receive_message(&mut websocket_rx) => {
                        match msg {
                            MessageResult::ApplicationMessage(message) => {
                                if let Err(err) = ws_inbound_tx.send(message).await {
                                    tracing::warn!(?err, "Failed to forward message to application");
                                    break;
                                }
                            }
                            MessageResult::ControlMessage => {
                                if let Err(err) = pong_update_tx.send(Instant::now()) {
                                    tracing::warn!(?err, "Failed to propagate last pong response, continuing");
                                    continue;
                                }
                            },
                            MessageResult::Disconnected => {
                                tracing::debug!("Client disconnected");
                                break;
                            }
                            MessageResult::Error(err) => {
                                tracing::warn!(?err, "Error while receiving message from client");
                                break;
                            }
                        }
                    }
                }
            }
            tracing::trace!("WebSocket reader task finished");
        }.instrument(tracing::Span::current()));

        (join_handle, ws_inbound_rx)
    }

    #[instrument(level = "debug", skip_all)]
    fn spawn_ping_task(
        ws_outbound_tx: &mpsc::Sender<ws::Message>,
        pong_update_rx: watch::Receiver<Instant>,
    ) -> (JoinHandle<()>, oneshot::Receiver<()>) {
        let (ping_shutdown_tx, ping_shutdown_rx) = oneshot::channel();

        let ws_outbound_tx = ws_outbound_tx.clone();
        let join_handle = tokio::spawn(
            async move {
                tracing::trace!("WebSocket ping task started");
                let _guard = TaskDropLogger::new("ping");

                let mut interval = tokio::time::interval(config::CLIENT_WEBSOCKET_PING_INTERVAL);
                loop {
                    interval.tick().await;

                    if Instant::now().duration_since(*pong_update_rx.borrow())
                        > config::CLIENT_WEBSOCKET_PONG_TIMEOUT
                    {
                        tracing::warn!("Pong timeout exceeded, disconnecting client");
                        let _ = ping_shutdown_tx.send(());
                        break;
                    }

                    if let Err(err) = ws_outbound_tx
                        .send(ws::Message::Ping(bytes::Bytes::new()))
                        .await
                    {
                        tracing::warn!(?err, "Failed to send ping to client");
                        let _ = ping_shutdown_tx.send(());
                        break;
                    }
                }
                tracing::trace!("WebSocket ping task finished");
            }
            .instrument(tracing::Span::current()),
        );

        (join_handle, ping_shutdown_rx)
    }
}

impl Debug for ClientSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientSession")
            .field("client_info", &self.client_info)
            .field("active_profile", &self.active_profile)
            .finish_non_exhaustive()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::test_util::{TestSetup, create_client_info};
    use axum::extract::ws;
    use axum::extract::ws::Utf8Bytes;
    use pretty_assertions::{assert_eq, assert_matches};
    use test_log::test;

    #[test(tokio::test)]
    async fn new_client_session() {
        let client_info_1 = create_client_info(1);
        let profile_id_1 = ProfileId::from("profile1");
        let active_profile = ActiveProfile::Specific(profile_id_1.clone());
        let (tx, _rx) = mpsc::channel::<ServerMessage>(10);
        let session = ClientSession::new(
            client_info_1.clone(),
            active_profile,
            tx,
            ClientConnectionGuard::default(),
        );

        assert_eq!(session.id(), &ClientId::from("client1"));
        assert_eq!(session.client_info(), &client_info_1);
        assert_matches!(session.active_profile(), ActiveProfile::Specific(profile_id) if *profile_id == profile_id_1);
    }

    #[test(tokio::test)]
    async fn send_message() {
        let client_info_1 = create_client_info(1);
        let (tx, mut rx) = mpsc::channel(10);
        let session = ClientSession::new(
            client_info_1,
            ActiveProfile::None,
            tx,
            ClientConnectionGuard::default(),
        );

        let client_info_2 = create_client_info(2);
        let message = ServerMessage::ClientList(server::ClientList {
            clients: vec![client_info_2],
        });
        let result = session.send_message(message.clone()).await;

        assert!(result.is_ok());
        let received = rx.recv().await.expect("Expected message to be received");
        assert_eq!(received, message);
    }

    #[test(tokio::test)]
    async fn send_message_error() {
        let client_info_1 = create_client_info(1);
        let (tx, _) = mpsc::channel(10);
        let session = ClientSession::new(
            client_info_1,
            ActiveProfile::None,
            tx.clone(),
            ClientConnectionGuard::default(),
        );
        drop(tx); // Drop the sender to simulate the client disconnecting

        let client_info_2 = create_client_info(2);
        let message = ServerMessage::ClientList(server::ClientList {
            clients: vec![client_info_2],
        });
        let result = session.send_message(message.clone()).await;

        assert!(result.is_err_and(|err| err.to_string().contains("failed to send message")));
    }

    #[test(tokio::test)]
    async fn initial_client_list_without_self() {
        let setup = TestSetup::new();
        let client_info_1 = create_client_info(1);
        setup.register_client(client_info_1).await;
        let websocket_rx = setup.websocket_rx.clone();

        let (handle_task, _shutdown_tx) = setup.spawn_session_handle_interaction();

        let _ = websocket_rx.lock().await.recv().await; // skip client info message
        let message = websocket_rx.lock().await.recv().await;
        match message {
            Some(ws::Message::Text(text)) => {
                assert_eq!(
                    text,
                    Utf8Bytes::from_static(r#"{"type":"clientList","clients":[]}"#)
                );
            }
            _ => panic!("Expected client list message"),
        }

        handle_task.await.unwrap();
    }

    #[test(tokio::test)]
    async fn initial_client_info() {
        let setup = TestSetup::new();
        let client_info_1 = create_client_info(1);
        setup.register_client(client_info_1).await;
        let websocket_rx = setup.websocket_rx.clone();

        let (handle_task, _shutdown_tx) = setup.spawn_session_handle_interaction();

        let message = websocket_rx.lock().await.recv().await;
        match message {
            Some(ws::Message::Text(text)) => {
                assert_eq!(
                    text,
                    Utf8Bytes::from_static(
                        r#"{"type":"sessionInfo","client":{"id":"client1","displayName":"Client 1","frequency":"100.000","positionId":"POSITION1"},"profile":{"type":"changed","activeProfile":{"type":"none"}},"defaultCallSources":[]}"#
                    )
                );
            }
            _ => panic!("Expected client info message"),
        }

        handle_task.await.unwrap();
    }

    #[test(tokio::test)]
    async fn initial_client_list() {
        let setup = TestSetup::new();
        let client_info_1 = create_client_info(1);
        let client_info_2 = create_client_info(2);
        setup.register_client(client_info_1.clone()).await;
        setup.register_client(client_info_2).await;
        let websocket_rx = setup.websocket_rx.clone();

        let (handle_task, _shutdown_tx) = setup.spawn_session_handle_interaction();

        let _ = websocket_rx.lock().await.recv().await; // skip client info message
        let message = websocket_rx.lock().await.recv().await;
        match message {
            Some(ws::Message::Text(text)) => {
                assert_eq!(
                    text,
                    Utf8Bytes::from_static(
                        r#"{"type":"clientList","clients":[{"id":"client2","displayName":"Client 2","frequency":"200.000","positionId":"POSITION2"}]}"#
                    )
                );
            }
            _ => panic!("Expected client list message"),
        }

        handle_task.await.unwrap();
    }

    #[test(tokio::test)]
    async fn handle_interaction() {
        let client_info_2 = create_client_info(2);
        let setup = TestSetup::new().with_messages(vec![Ok(ws::Message::Text(
            Utf8Bytes::from_static(r#"{"type":"callInvite","callId":"00000000-0000-0000-0000-000000000000","source":{"clientId":"client1"},"target":{"client":"client2"},"prio":false}"#),
        ))]);
        let (_, mut client2_rx) = setup.register_client(client_info_2).await;
        let websocket_rx = setup.websocket_rx.clone();

        let (handle_task, _shutdown_tx) = setup.spawn_session_handle_interaction();

        let _ = websocket_rx.lock().await.recv().await; // skip client info message
        let message = websocket_rx.lock().await.recv().await;
        match message {
            Some(ws::Message::Text(text)) => {
                assert_eq!(
                    text,
                    Utf8Bytes::from_static(
                        r#"{"type":"clientList","clients":[{"id":"client2","displayName":"Client 2","frequency":"200.000","positionId":"POSITION2"}]}"#
                    )
                );
            }
            _ => panic!("Expected client list message"),
        }

        let call_invite = client2_rx.recv().await.unwrap();
        assert_eq!(
            call_invite,
            ServerMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
                call_id: vacs_protocol::ws::shared::CallId::from(uuid::Uuid::nil()),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: ClientId::from("client1"),
                    position_id: None,
                    station_id: None,
                },
                target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
                prio: false,
            })
        );

        handle_task.await.unwrap();
    }

    #[test(tokio::test)]
    async fn handle_interaction_websocket_error() {
        let setup = TestSetup::new().with_messages(vec![Err(axum::Error::new("Test error"))]);
        let websocket_rx = setup.websocket_rx.clone();

        let (handle_task, _shutdown_tx) = setup.spawn_session_handle_interaction();

        let _ = websocket_rx.lock().await.recv().await; // skip client info message
        let message = websocket_rx.lock().await.recv().await;
        match message {
            Some(ws::Message::Text(text)) => {
                assert_eq!(
                    text,
                    Utf8Bytes::from_static(r#"{"type":"clientList","clients":[]}"#)
                );
            }
            _ => panic!("Expected client list message"),
        }

        assert!(websocket_rx.lock().await.is_closed());

        handle_task.await.unwrap();
    }
}
