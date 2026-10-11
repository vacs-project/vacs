use crate::config;
use crate::metrics::guards::ClientConnectionGuard;
use crate::metrics::labels::AsMetricLabel;
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
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::Instrument;
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
    connected_at: Instant,
    /// Only the copy in `ClientManager::clients` is authoritative; clones go stale.
    pending_disconnect: bool,
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
            connected_at: Instant::now(),
            pending_disconnect: false,
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

    /// Returns `true` if the client connected recently enough that the datafeed
    /// may not have caught up with the slurper yet. During this window, position
    /// changes are ignored and missing-connection disconnects are suppressed.
    #[inline]
    pub fn is_within_position_grace_period(&self, grace_period: &Duration) -> bool {
        self.connected_at.elapsed() < *grace_period
    }

    /// Marks the session for disconnect. The mark dies with the session, so a
    /// reconnect always starts unmarked.
    #[inline]
    pub fn mark_pending_disconnect(&mut self) {
        self.pending_disconnect = true;
    }

    /// Clears a pending disconnect mark, returning whether one was set.
    #[inline]
    pub fn take_pending_disconnect(&mut self) -> bool {
        std::mem::take(&mut self.pending_disconnect)
    }

    pub fn is_pending_disconnect(&self) -> bool {
        self.pending_disconnect
    }

    #[inline]
    pub fn client_info(&self) -> &ClientInfo {
        &self.client_info
    }

    #[inline]
    pub fn active_profile(&self) -> &ActiveProfile<ProfileId> {
        &self.active_profile
    }

    pub fn update_client_info(&mut self, controller_info: &ControllerInfo) -> bool {
        let mut changed = false;
        if self.client_info.display_name != controller_info.callsign {
            tracing::debug!(
                client_id = %self.client_info.id,
                old = %self.client_info.display_name,
                new = %controller_info.callsign,
                "Controller callsign changed"
            );
            self.client_info.display_name = controller_info.callsign.clone();
            changed = true;
        }
        if self.client_info.frequency != controller_info.frequency {
            tracing::debug!(
                client_id = %self.client_info.id,
                old = %self.client_info.frequency,
                new = %controller_info.frequency,
                "Controller frequency changed"
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
                    tracing::trace!(
                        peer_ids = %self.client_info.id,
                        profile_id = %new_profile_id,
                        "Active profile changed"
                    );
                    self.active_profile = ActiveProfile::Specific(new_profile_id.clone());
                    SessionProfile::Changed(ActiveProfile::Specific(profile.into()))
                } else {
                    tracing::warn!(
                        peer_ids = %self.client_info.id,
                        profile_id = %new_profile_id,
                        "Active profile does not exist, falling back to none"
                    );
                    self.active_profile = ActiveProfile::None;
                    SessionProfile::Changed(ActiveProfile::None)
                }
            }
            (_, None) => {
                tracing::trace!(peer_ids = %self.client_info.id, "Active profile cleared");
                self.active_profile = ActiveProfile::None;
                SessionProfile::Changed(ActiveProfile::None)
            }
        }
    }

    pub fn disconnect(&self, disconnect_reason: Option<DisconnectReason>) {
        if let Some(reason) = &disconnect_reason {
            self.client_connection_guard
                .lock()
                .set_disconnect_reason(reason.clone());
        }
        let _ = self.client_shutdown_tx.send(disconnect_reason);
    }

    pub async fn send_message(&self, message: impl Into<ServerMessage>) -> Result<()> {
        self.tx
            .send(message.into())
            .await
            .map_err(|err| ClientManagerError::MessageSendError(err.to_string()))
    }

    pub async fn send_error(&self, err: impl Into<shared::Error>) {
        let err = err.into();
        if let Err(err) = self.send_message(err).await {
            tracing::debug!(%err, "Failed to send error message");
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_interaction<R: WebSocketStream + 'static, T: WebSocketSink + 'static>(
        &mut self,
        app_state: &Arc<AppState>,
        websocket_rx: R,
        websocket_tx: T,
        broadcast_rx: &mut broadcast::Receiver<ServerMessage>,
        rx: &mut mpsc::Receiver<ServerMessage>,
        app_shutdown_rx: &mut watch::Receiver<()>,
    ) -> SessionEnd {
        let (pong_update_tx, pong_update_rx) = watch::channel(Instant::now());

        let (writer_handle, ws_outbound_tx) = ClientSession::spawn_writer(
            websocket_tx,
            app_shutdown_rx.clone(),
            self.client_shutdown_tx.subscribe(),
        )
        .await;
        let (mut reader_handle, mut ws_inbound_rx) = ClientSession::spawn_reader(
            websocket_rx,
            app_shutdown_rx.clone(),
            self.client_shutdown_tx.subscribe(),
            pong_update_tx,
        )
        .await;
        let (ping_handle, mut ping_shutdown_rx) =
            ClientSession::spawn_ping_task(&ws_outbound_tx, pong_update_rx);

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
                max_conf_size: Some(app_state.calls.max_conf_size()),
            },
        )
        .await
        {
            tracing::debug!(
                err = format!("{err:#}"),
                "Failed to send initial session info"
            );
        }

        let clients = app_state.list_clients(Some(&self.client_info.id)).await;
        if let Err(err) = send_message(&ws_outbound_tx, server::ClientList { clients }).await {
            tracing::debug!(
                err = format!("{err:#}"),
                "Failed to send initial client list"
            );
        }

        let stations = app_state
            .list_stations(&self.active_profile, self.client_info.position_id.as_ref())
            .await;
        if let Err(err) = send_message(&ws_outbound_tx, server::StationList { stations }).await {
            tracing::debug!(
                err = format!("{err:#}"),
                "Failed to send initial stations list"
            );
        }

        let end = loop {
            tokio::select! {
                biased;

                _ = app_shutdown_rx.changed() => {
                    break SessionEnd::ServerShutdown;
                }

                end = &mut ping_shutdown_rx => {
                    break end.unwrap_or(SessionEnd::PingFailed);
                }

                msg = ws_inbound_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            match handle_application_message(app_state, self, msg).await {
                                ControlFlow::Continue(()) => continue,
                                ControlFlow::Break(()) => break SessionEnd::Logout,
                            }
                        }
                        None => {
                            break (&mut reader_handle).await.unwrap_or(SessionEnd::ReceiveError);
                        }
                    }
                }

                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            tracing::trace!(kind = msg.as_metric_label(), "Received direct message");
                            let msg = self.refresh_call_update(app_state, msg);
                            if let Err(err) = send_message(&ws_outbound_tx, msg).await {
                                tracing::debug!(err = format!("{err:#}"), "Failed to send direct message");
                            }
                        }
                        None => {
                            break match self.client_shutdown_tx.borrow().as_ref() {
                                Some(reason) => SessionEnd::Disconnected(reason.as_metric_label()),
                                None => SessionEnd::Removed,
                            };
                        }
                    }
                }

                msg = broadcast_rx.recv() => {
                    match msg {
                        Ok(msg) => {
                            if let ServerMessage::ClientInfo(info) = &msg
                                && info.id == self.client_info.id {
                                continue;
                            }

                            tracing::trace!(kind = msg.as_metric_label(), "Received broadcast message");
                            if let Err(err) = send_message(&ws_outbound_tx, msg).await {
                                tracing::debug!(err = format!("{err:#}"), "Failed to send broadcast message");
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "Broadcast receiver lagged, messages dropped");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::debug!("Broadcast channel closed");
                        }
                    }
                }
            }
        };

        writer_handle.abort();
        reader_handle.abort();
        ping_handle.abort();

        end
    }

    /// Fan-outs enqueue after the call lock is released, so a snapshot computed earlier
    /// can land behind a newer one; forwarding the current state keeps deliveries in order.
    fn refresh_call_update(&self, app_state: &AppState, message: ServerMessage) -> ServerMessage {
        let ServerMessage::CallUpdate(update) = &message else {
            return message;
        };
        match app_state
            .calls
            .call_state_for_party(&update.call_id, self.id())
        {
            Some(current) => ServerMessage::CallUpdate(current.for_recipient(self.id())),
            None => message,
        }
    }

    async fn spawn_writer<T: WebSocketSink + 'static>(
        mut websocket_tx: T,
        mut app_shutdown_rx: watch::Receiver<()>,
        mut client_shutdown_rx: watch::Receiver<Option<DisconnectReason>>,
    ) -> (JoinHandle<()>, mpsc::Sender<ws::Message>) {
        let (ws_outbound_tx, mut ws_outbound_rx) =
            mpsc::channel::<ws::Message>(config::CLIENT_WEBSOCKET_TASK_CHANNEL_CAPACITY);

        let join_handle = tokio::spawn(async move {
            let _guard = TaskDropLogger::new("writer");

            loop {
                tokio::select! {
                    biased;

                    _ = app_shutdown_rx.changed() => {
                        break;
                    }

                    _ = client_shutdown_rx.changed() => {
                        let reason_opt = {
                            client_shutdown_rx.borrow().clone()
                        };

                        if let Some(reason) = reason_opt {
                            tracing::trace!(reason = reason.as_metric_label(), "Sending disconnect message");
                            match ServerMessage::serialize(&ServerMessage::from(server::Disconnected {reason})) {
                                Ok(msg) => {
                                    if let Err(err) = websocket_tx.send(ws::Message::from(msg)).await {
                                        tracing::debug!(%err, "Failed to send disconnect message");
                                    }
                                },
                                Err(err) => {
                                    tracing::error!(%err, "Failed to serialize disconnect message");
                                }
                            }
                        }
                        break;
                    }

                    msg = ws_outbound_rx.recv() => {
                        match msg {
                            Some(msg) => {
                                if let Err(err) = websocket_tx.send(msg).await {
                                    tracing::debug!(%err, "Failed to send message to client");
                                    break;
                                }
                            },
                            None => break,
                        }
                    }
                }
            }

            if let Err(err) = websocket_tx.send(ws::Message::Close(None)).await {
                tracing::debug!(%err, "Failed to send close message to client");
            }
        }.instrument(tracing::Span::current()));

        (join_handle, ws_outbound_tx)
    }

    async fn spawn_reader<R: WebSocketStream + 'static>(
        mut websocket_rx: R,
        mut app_shutdown_rx: watch::Receiver<()>,
        mut client_shutdown_rx: watch::Receiver<Option<DisconnectReason>>,
        pong_update_tx: watch::Sender<Instant>,
    ) -> (JoinHandle<SessionEnd>, mpsc::Receiver<ClientMessage>) {
        let (ws_inbound_tx, ws_inbound_rx) =
            mpsc::channel::<ClientMessage>(config::CLIENT_WEBSOCKET_TASK_CHANNEL_CAPACITY);

        let join_handle = tokio::spawn(async move {
            let _guard = TaskDropLogger::new("reader");

            loop {
                tokio::select! {
                    biased;

                    _ = app_shutdown_rx.changed() => {
                        break SessionEnd::ServerShutdown;
                    }

                    _ = client_shutdown_rx.changed() => {
                        break SessionEnd::Disconnected(client_shutdown_rx.borrow().as_metric_label());
                    }

                    msg = receive_message(&mut websocket_rx) => {
                        match msg {
                            MessageResult::ApplicationMessage(message) => {
                                if let Err(err) = ws_inbound_tx.send(message).await {
                                    tracing::debug!(%err, "Failed to forward message to application");
                                    break SessionEnd::Removed;
                                }
                            }
                            MessageResult::ControlMessage => {
                                let _ = pong_update_tx.send(Instant::now());
                            },
                            MessageResult::Disconnected => break SessionEnd::Closed,
                            MessageResult::Error(_) => break SessionEnd::ReceiveError,
                        }
                    }
                }
            }
        }.instrument(tracing::Span::current()));

        (join_handle, ws_inbound_rx)
    }

    fn spawn_ping_task(
        ws_outbound_tx: &mpsc::Sender<ws::Message>,
        pong_update_rx: watch::Receiver<Instant>,
    ) -> (JoinHandle<()>, oneshot::Receiver<SessionEnd>) {
        let (ping_shutdown_tx, ping_shutdown_rx) = oneshot::channel();

        let ws_outbound_tx = ws_outbound_tx.clone();
        let join_handle = tokio::spawn(
            async move {
                let _guard = TaskDropLogger::new("ping");

                let mut interval = tokio::time::interval(config::CLIENT_WEBSOCKET_PING_INTERVAL);
                loop {
                    interval.tick().await;

                    if Instant::now().duration_since(*pong_update_rx.borrow())
                        > config::CLIENT_WEBSOCKET_PONG_TIMEOUT
                    {
                        tracing::debug!("Pong timeout exceeded, disconnecting client");
                        let _ = ping_shutdown_tx.send(SessionEnd::PongTimeout);
                        break;
                    }

                    if let Err(err) = ws_outbound_tx
                        .send(ws::Message::Ping(bytes::Bytes::new()))
                        .await
                    {
                        tracing::debug!(%err, "Failed to queue ping");
                        let _ = ping_shutdown_tx.send(SessionEnd::PingFailed);
                        break;
                    }
                }
            }
            .instrument(tracing::Span::current()),
        );

        (join_handle, ping_shutdown_rx)
    }

    #[cfg(test)]
    pub fn expire_position_grace_period(&mut self, grace_period: &Duration) {
        self.connected_at = Instant::now()
            .checked_sub(*grace_period)
            .expect("monotonic clock too young to rewind past the grace period");
    }
}

/// Why a client's websocket session ended, as logged on the disconnect line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEnd {
    /// The client closed the websocket.
    Closed,
    /// Reading from the websocket failed.
    ReceiveError,
    /// The client stopped answering pings.
    PongTimeout,
    /// A ping could not be queued, because the writer has stopped.
    PingFailed,
    /// The client logged out or asked to be disconnected.
    Logout,
    /// The server disconnected the client, with the metric label of the reason.
    Disconnected(&'static str),
    /// The client was removed from the client manager.
    Removed,
    /// The server is shutting down.
    ServerShutdown,
}

impl AsMetricLabel for SessionEnd {
    fn as_metric_label(&self) -> &'static str {
        match self {
            SessionEnd::Closed => "closed",
            SessionEnd::ReceiveError => "receive_error",
            SessionEnd::PongTimeout => "pong_timeout",
            SessionEnd::PingFailed => "ping_failed",
            SessionEnd::Logout => "logout",
            SessionEnd::Disconnected(reason) => reason,
            SessionEnd::Removed => "removed",
            SessionEnd::ServerShutdown => "server_shutdown",
        }
    }
}

impl Debug for ClientSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientSession")
            .field("client_info", &self.client_info)
            .field("active_profile", &self.active_profile)
            .field("connected_at", &self.connected_at)
            .field("pending_disconnect", &self.pending_disconnect)
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
        tracing::trace!(task_name = self.name, "Task dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::calls::{AcceptCallOutcome, DropTargetOutcome};
    use crate::ws::test_util::{TestSetup, create_client_info};
    use axum::extract::ws;
    use axum::extract::ws::Utf8Bytes;
    use pretty_assertions::{assert_eq, assert_matches};
    use std::collections::{HashMap, HashSet};
    use test_log::test;
    use vacs_protocol::ws::client::CallDropReason;
    use vacs_protocol::ws::shared::{CallId, CallSource, CallTarget};

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

    fn session_for(id: u8) -> ClientSession {
        ClientSession::new(
            create_client_info(id),
            ActiveProfile::None,
            mpsc::channel(1).0,
            ClientConnectionGuard::default(),
        )
    }

    fn ring(app_state: &AppState, call_id: CallId, caller: &ClientId, target: &ClientId) {
        app_state
            .calls
            .attempt_call(
                &call_id,
                caller,
                &CallSource {
                    client_id: caller.clone(),
                    position_id: None,
                    station_id: None,
                },
                &CallTarget::Client(target.clone()),
                &HashSet::from([target.clone()]),
            )
            .expect("call attempt should succeed");
    }

    fn refreshed(
        session: &ClientSession,
        app_state: &AppState,
        call_id: CallId,
    ) -> server::CallUpdate {
        let queued = ServerMessage::CallUpdate(server::CallUpdate {
            call_id,
            invited_targets: HashSet::from([CallTarget::Client(ClientId::from("client9"))]),
            joined_participants: HashMap::new(),
            conference_leader: None,
        });
        match session.refresh_call_update(app_state, queued) {
            ServerMessage::CallUpdate(update) => update,
            other => panic!("a call update must stay a call update, got {other:?}"),
        }
    }

    #[test]
    fn forwarded_call_updates_carry_the_current_roster() {
        let setup = TestSetup::new();
        let caller = setup.session.id().clone();
        let callee = ClientId::from("client2");
        let call_id = CallId::new();
        ring(&setup.app_state, call_id, &caller, &callee);

        let for_invitee = refreshed(&session_for(2), &setup.app_state, call_id);
        assert!(for_invitee.invited_targets.is_empty());
        assert!(for_invitee.joined_participants.is_empty());
        assert_eq!(for_invitee.conference_leader, None);

        assert!(matches!(
            setup.app_state.calls.accept_call(&call_id, &callee),
            AcceptCallOutcome::Accepted { .. }
        ));

        for session in [&setup.session, &session_for(2)] {
            let update = refreshed(session, &setup.app_state, call_id);
            assert!(update.invited_targets.is_empty());
            assert_eq!(
                update
                    .joined_participants
                    .keys()
                    .cloned()
                    .collect::<HashSet<_>>(),
                HashSet::from([caller.clone(), callee.clone()]),
                "{} should see the post-accept roster",
                session.id()
            );
        }
    }

    #[test]
    fn a_ringing_invitee_sees_its_co_targets_but_not_itself() {
        let setup = TestSetup::new();
        let caller = setup.session.id().clone();
        let call_id = CallId::new();
        ring(
            &setup.app_state,
            call_id,
            &caller,
            &ClientId::from("client2"),
        );
        ring(
            &setup.app_state,
            call_id,
            &caller,
            &ClientId::from("client3"),
        );

        let update = refreshed(&session_for(2), &setup.app_state, call_id);
        assert_eq!(
            update.invited_targets,
            HashSet::from([CallTarget::Client(ClientId::from("client3"))])
        );
    }

    #[test]
    fn queued_updates_are_forwarded_unchanged_once_the_client_is_no_party() {
        let setup = TestSetup::new();
        let caller = setup.session.id().clone();
        let callee = ClientId::from("client2");
        let third = ClientId::from("client3");
        let queued = ServerMessage::CallUpdate(server::CallUpdate {
            call_id: CallId::new(),
            invited_targets: HashSet::new(),
            joined_participants: HashMap::new(),
            conference_leader: None,
        });
        assert_eq!(
            setup
                .session
                .refresh_call_update(&setup.app_state, queued.clone()),
            queued,
            "an update for a call that is gone stays as queued"
        );

        let call_id = CallId::new();
        ring(&setup.app_state, call_id, &caller, &callee);
        assert!(matches!(
            setup.app_state.calls.accept_call(&call_id, &callee),
            AcceptCallOutcome::Accepted { .. }
        ));
        ring(&setup.app_state, call_id, &caller, &third);
        assert!(matches!(
            setup.app_state.calls.accept_call(&call_id, &third),
            AcceptCallOutcome::Accepted { .. }
        ));
        assert!(matches!(
            setup.app_state.calls.drop_target(
                &call_id,
                &caller,
                &CallTarget::Client(third.clone()),
                CallDropReason::Requested,
            ),
            DropTargetOutcome::ParticipantDropped(..)
        ));

        let stale = ServerMessage::CallUpdate(server::CallUpdate {
            call_id,
            invited_targets: HashSet::new(),
            joined_participants: HashMap::new(),
            conference_leader: None,
        });
        assert_eq!(
            session_for(3).refresh_call_update(&setup.app_state, stale.clone()),
            stale,
            "a dropped participant gets the queued message, the call end follows it"
        );
        assert_eq!(
            session_for(9).refresh_call_update(&setup.app_state, stale.clone()),
            stale,
            "a client the call never involved gets the queued message"
        );
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
                        r#"{"type":"sessionInfo","client":{"id":"client1","displayName":"Client 1","frequency":"100.000","positionId":"POSITION1"},"profile":{"type":"changed","activeProfile":{"type":"none"}},"defaultCallSources":[],"maxConfSize":8}"#
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
            Utf8Bytes::from_static(r#"{"type":"callInvite","callId":"00000000-0000-0000-0000-000000000000","source":{"clientId":"client1"},"targets":[{"client":"client2"}],"prio":false}"#),
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
            ServerMessage::CallInvitation(vacs_protocol::ws::server::CallInvitation {
                call_id: vacs_protocol::ws::shared::CallId::from(uuid::Uuid::nil()),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: ClientId::from("client1"),
                    position_id: None,
                    station_id: None,
                },
                target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("client2")),
                // The recipient's own target is carried by `target` only.
                invited_targets: HashSet::new(),
                joined_participants: HashMap::new(),
                conference_leader: None,
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
