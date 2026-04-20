use crate::app::state::http::HttpState;
use crate::app::state::webrtc::{AppStateWebrtcExt, UnansweredCallGuard};
use crate::app::state::{AppState, AppStateInner, sealed};
use crate::audio::manager::{AudioManagerHandle, SourceType};
use crate::config::{BackendEndpoint, WS_LOGIN_TIMEOUT};
use crate::error::{Error, FrontendError};
use crate::signaling::auth::TauriTokenProvider;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use vacs_signaling::client::{SignalingClient, SignalingEvent, State};
use vacs_signaling::error::{SignalingError, SignalingRuntimeError};
use vacs_signaling::protocol::http::webrtc::IceConfig;
use vacs_signaling::protocol::vatsim::{ClientId, PositionId, StationChange};
use vacs_signaling::protocol::ws::client::{CallRejectReason, ClientMessage};
use vacs_signaling::protocol::ws::server::{
    CallCancelReason, DisconnectReason, LoginFailureReason, ServerMessage, SessionProfile,
};
use vacs_signaling::protocol::ws::shared::{
    CallErrorReason, CallId, CallInvite, CallSource, ErrorReason,
};
use vacs_signaling::protocol::ws::{client, server, shared};
use vacs_signaling::transport::tokio::TokioTransport;

const INCOMING_CALLS_LIMIT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    #[allow(dead_code)]
    Test,
}

pub trait AppStateSignalingExt: sealed::Sealed {
    async fn connect_signaling(
        &self,
        app: &AppHandle,
        position_id: Option<PositionId>,
    ) -> Result<(), Error>;
    async fn disconnect_signaling(&mut self, app: &AppHandle);
    async fn handle_signaling_connection_closed(&mut self, app: &AppHandle);
    async fn send_signaling_message(&mut self, msg: impl Into<ClientMessage>) -> Result<(), Error>;
    fn set_client_id(&mut self, client_id: Option<ClientId>);
    fn outgoing_call_id(&self) -> Option<&CallId>;
    fn set_outgoing_call(&mut self, invite: Option<CallInvite>);
    fn remove_outgoing_call(&mut self, call_id: &CallId) -> bool;
    fn incoming_calls_len(&self) -> usize;
    fn add_incoming_call(&mut self, invite: CallInvite);
    fn remove_incoming_call(&mut self, call_id: &CallId) -> bool;
    fn add_incoming_call_to_call_list(
        &mut self,
        app: &AppHandle,
        call_id: &CallId,
        source: &CallSource,
    );
    fn new_signaling_client(
        app: AppHandle,
        ws_url: &str,
        shutdown_token: CancellationToken,
        max_reconnect_attempts: u8,
    ) -> SignalingClient<TokioTransport, TauriTokenProvider>;
    fn start_unanswered_call_timer(&mut self, app: &AppHandle, call_id: &CallId);
    fn cancel_unanswered_call_timer(&mut self, call_id: &CallId);
    async fn accept_call(
        &mut self,
        app: &AppHandle,
        call_id: Option<CallId>,
    ) -> Result<bool, Error>;
    async fn end_call(&mut self, app: &AppHandle, call_id: Option<CallId>) -> Result<bool, Error>;
    fn clear_session_cache(&mut self);
}

impl AppStateSignalingExt for AppStateInner {
    async fn connect_signaling(
        &self,
        app: &AppHandle,
        position_id: Option<PositionId>,
    ) -> Result<(), Error> {
        if self.signaling_client.state() != State::Disconnected {
            log::info!("Already connected and logged in with signaling server");
            return Err(Error::Signaling(Box::from(SignalingError::Other(
                "Already connected".to_string(),
            ))));
        }

        log::info!("Connecting to signaling server with position ID: {position_id:?}");
        match self.signaling_client.connect(position_id).await {
            Ok(()) => {}
            Err(SignalingError::LoginError(LoginFailureReason::AmbiguousVatsimPosition(
                positions,
            ))) => {
                log::warn!(
                    "Connection to signaling server failed, ambiguous VATSIM position: {positions:?}"
                );
                app.emit("signaling:ambiguous-position", &positions).ok();
                return Err(SignalingError::LoginError(
                    LoginFailureReason::AmbiguousVatsimPosition(positions),
                )
                .into());
            }
            Err(err) => return Err(err.into()),
        }

        log::info!("Successfully connected to signaling server");
        Ok(())
    }

    async fn disconnect_signaling(&mut self, app: &AppHandle) {
        log::info!("Disconnecting from signaling server");

        self.cleanup_signaling(app).await;
        app.emit("signaling:disconnected", Value::Null).ok();
        self.signaling_client.disconnect().await;

        log::debug!("Successfully disconnected from signaling server");
    }

    async fn handle_signaling_connection_closed(&mut self, app: &AppHandle) {
        log::info!("Handling signaling server connection closed");

        self.cleanup_signaling(app).await;

        app.emit("signaling:disconnected", Value::Null).ok();
        log::debug!("Successfully handled closed signaling server connection");
    }

    async fn send_signaling_message(&mut self, msg: impl Into<ClientMessage>) -> Result<(), Error> {
        let msg = msg.into();
        log::trace!("Sending signaling message: {msg:?}");

        if let Err(err) = self.signaling_client.send(msg).await {
            log::warn!("Failed to send signaling message: {err:?}");
            return Err(err.into());
        }

        log::trace!("Successfully sent signaling message");
        Ok(())
    }

    fn set_client_id(&mut self, client_id: Option<ClientId>) {
        self.client_id = client_id;
    }

    fn outgoing_call_id(&self) -> Option<&CallId> {
        self.outgoing_call.as_ref().map(|c| &c.call_id)
    }

    fn set_outgoing_call(&mut self, invite: Option<CallInvite>) {
        self.outgoing_call = invite;
    }

    fn remove_outgoing_call(&mut self, call_id: &CallId) -> bool {
        if self
            .outgoing_call
            .as_ref()
            .is_some_and(|c| c.call_id == *call_id)
        {
            self.outgoing_call = None;
            self.audio_manager.read().stop(SourceType::Ringback);
            true
        } else {
            false
        }
    }

    fn incoming_calls_len(&self) -> usize {
        self.incoming_calls.len()
    }

    fn add_incoming_call(&mut self, invite: CallInvite) {
        self.incoming_calls.insert(invite.call_id, invite);
    }

    fn remove_incoming_call(&mut self, call_id: &CallId) -> bool {
        let found = self.incoming_calls.remove(call_id).is_some();
        if self.incoming_calls.is_empty() {
            self.audio_manager.read().stop(SourceType::Ring);
            self.audio_manager.read().stop(SourceType::PriorityRing);
        }
        found
    }

    fn add_incoming_call_to_call_list(
        &mut self,
        app: &AppHandle,
        call_id: &CallId,
        source: &CallSource,
    ) {
        #[derive(Clone, Serialize)]
        #[serde(rename_all = "camelCase")]
        struct IncomingCallListEntry<'a> {
            call_id: &'a CallId,
            source: &'a CallSource,
        }

        app.emit(
            "signaling:add-incoming-to-call-list",
            IncomingCallListEntry { call_id, source },
        )
        .ok();
    }

    fn new_signaling_client(
        app: AppHandle,
        ws_url: &str,
        shutdown_token: CancellationToken,
        max_reconnect_attempts: u8,
    ) -> SignalingClient<TokioTransport, TauriTokenProvider> {
        let on_terminate_session = Self::on_terminate_session(app.clone());

        SignalingClient::new(
            TokioTransport::new(ws_url),
            TauriTokenProvider::new(app.clone()),
            move |e| {
                let handle = app.clone();
                async move {
                    Self::handle_signaling_event(&handle, e).await;
                }
            },
            shutdown_token,
            false,
            WS_LOGIN_TIMEOUT,
            max_reconnect_attempts,
            Some(on_terminate_session),
            tauri::async_runtime::handle().inner(),
        )
    }

    fn start_unanswered_call_timer(&mut self, app: &AppHandle, call_id: &CallId) {
        self.cancel_unanswered_call_timer(call_id);

        let Some(own_client_id) = self.client_id.as_ref().cloned() else {
            log::warn!("Cannot start unanswered call timer without own client ID");
            return;
        };

        let timeout = Duration::from_secs(self.config.client.auto_hangup_seconds);
        if timeout.is_zero() {
            return;
        }

        let cancel = self.shutdown_token.child_token();

        let handle = tauri::async_runtime::spawn({
            let app = app.clone();
            let cancel = cancel.clone();
            let call_id = *call_id;
            async move {
                log::debug!("Starting unanswered call timer of {timeout:?} for call {call_id}");
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        log::debug!("Unanswered call timer cancelled for call {call_id}");
                    }
                    _ = tokio::time::sleep(timeout) => {
                        log::debug!("Unanswered call timer expired for call {call_id}, hanging up");

                        let state = app.state::<AppState>();
                        let mut state = state.lock().await;

                        if let Err(err) = state.send_signaling_message(shared::CallEnd { call_id, ending_client_id: own_client_id }).await {
                            log::warn!("Failed to send call end message after call timer expired: {err:?}");
                        }

                        state.cleanup_call(&call_id).await;
                        state.set_outgoing_call(None);

                        let audio_manager = app.state::<AudioManagerHandle>();
                        audio_manager.read().stop(SourceType::Ringback);

                        state.emit_call_error(&app, call_id, false, CallErrorReason::AutoHangup);
                    }
                }
            }
        });

        self.unanswered_call_guard = Some(UnansweredCallGuard {
            call_id: *call_id,
            cancel,
            handle,
        });
    }

    fn cancel_unanswered_call_timer(&mut self, call_id: &CallId) {
        if let Some(guard) = self
            .unanswered_call_guard
            .take_if(|g| g.call_id == *call_id)
        {
            log::trace!(
                "Cancelling unanswered call timer for call {}",
                guard.call_id
            );
            guard.cancel.cancel();
            guard.handle.abort();
        }
    }

    async fn accept_call(
        &mut self,
        app: &AppHandle,
        call_id: Option<CallId>,
    ) -> Result<bool, Error> {
        let Some(own_client_id) = self.client_id.as_ref().cloned() else {
            log::warn!("Cannot accept call without own client ID");
            return Err(Error::Unauthorized);
        };

        let call_id = match call_id.or_else(|| self.incoming_calls.keys().next().copied()) {
            Some(id) => id,
            None => return Ok(false),
        };
        log::debug!("Accepting call {call_id:?}");

        if !self.config.ice.is_default() && self.is_ice_config_expired() {
            match app
                .state::<HttpState>()
                .http_get::<IceConfig>(BackendEndpoint::IceConfig, None)
                .await
            {
                Ok(config) => {
                    self.config.ice = config;
                }
                Err(err) => {
                    log::warn!("Failed to refresh ICE config, using cached one: {err:?}");
                }
            };
        }

        self.send_signaling_message(shared::CallAccept {
            call_id,
            accepting_client_id: own_client_id,
        })
        .await?;
        self.remove_incoming_call(&call_id);

        self.audio_manager.read().stop(SourceType::Ring);
        self.audio_manager.read().stop(SourceType::PriorityRing);

        app.emit("signaling:accept-incoming-call", call_id).ok();

        Ok(true)
    }

    async fn end_call(&mut self, app: &AppHandle, call_id: Option<CallId>) -> Result<bool, Error> {
        let Some(own_client_id) = self.client_id.as_ref().cloned() else {
            log::warn!("Cannot end call without own client ID");
            return Err(Error::Unauthorized);
        };

        let Some(call_id) =
            call_id.or_else(|| self.active_call_id().or(self.outgoing_call_id()).cloned())
        else {
            return Ok(false);
        };
        log::debug!("Ending call {call_id}");

        self.send_signaling_message(shared::CallEnd {
            call_id,
            ending_client_id: own_client_id,
        })
        .await?;

        self.cleanup_call(&call_id).await;

        self.cancel_unanswered_call_timer(&call_id);
        self.set_outgoing_call(None);

        self.audio_manager.read().stop(SourceType::Ringback);

        app.emit("signaling:force-call-end", call_id).ok();

        Ok(true)
    }

    fn clear_session_cache(&mut self) {
        self.connection_state = ConnectionState::Disconnected;
        self.session_info = None;
        self.stations.clear();
        self.clients.clear();
    }
}

impl AppStateInner {
    /// Returns a callback that terminates the current WebSocket session via the HTTP API.
    /// Called by the signaling client before each reconnect attempt when the original
    /// disconnect was caused by a connection loss (heartbeat timeout, transport error).
    fn on_terminate_session(app: AppHandle) -> vacs_signaling::client::OnTerminateSessionCb {
        Arc::new(move || {
            let app = app.clone();
            Box::pin(async move {
                if let Err(err) = app
                    .state::<HttpState>()
                    .http_delete::<()>(BackendEndpoint::TerminateWsSession, None)
                    .await
                {
                    log::warn!("Failed to terminate session before reconnect: {err:?}");
                }
            })
        })
    }

    async fn handle_signaling_event(app: &AppHandle, event: SignalingEvent) {
        match event {
            SignalingEvent::Connected {
                client_info,
                profile,
                default_call_sources,
            } => {
                log::debug!(
                    "Successfully connected to signaling server. Display name: {}, frequency: {}, profile: {profile}",
                    &client_info.display_name,
                    &client_info.frequency,
                );

                let session_info = server::SessionInfo {
                    client: client_info,
                    profile: SessionProfile::Changed(profile),
                    default_call_sources: default_call_sources.clone(),
                };

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.connection_state = ConnectionState::Connected;
                    state.session_info = Some(session_info.clone());
                    state.default_call_sources = default_call_sources;
                }

                app.emit("signaling:connected", session_info).ok();
            }
            SignalingEvent::Message(msg) => Self::handle_signaling_message(msg, app).await,
            SignalingEvent::Error(error) => {
                if error.is_fatal() {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.handle_signaling_connection_closed(app).await;

                    if let SignalingRuntimeError::Disconnected(Some(
                        DisconnectReason::AmbiguousVatsimPosition(positions),
                    )) = error
                    {
                        log::warn!(
                            "Disconnected from signaling server, ambiguous VATSIM position: {positions:?}"
                        );

                        app.emit("signaling:ambiguous-position", &positions).ok();
                    } else if error.can_reconnect() {
                        state.connection_state = ConnectionState::Connecting;
                        app.emit("signaling:reconnecting", Value::Null).ok();
                    } else {
                        app.emit::<FrontendError>("error", Error::from(error).into())
                            .ok();
                    }
                }
            }
        }
    }

    async fn handle_signaling_message(msg: ServerMessage, app: &AppHandle) {
        match msg {
            ServerMessage::CallInvite(
                ref msg @ shared::CallInvite {
                    ref call_id,
                    ref source,
                    ref target,
                    ref prio,
                },
            ) => {
                let caller_id = &source.client_id;
                log::trace!("Call invite received from {caller_id} for target {target:?}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                let Some(own_client_id) = state.client_id.as_ref().cloned() else {
                    log::warn!("Cannot handle call invite without own client ID");
                    return;
                };

                if state.config.client.ignored.contains(caller_id) {
                    log::trace!("Ignoring call invite from {caller_id}");
                    return;
                }

                state.add_incoming_call_to_call_list(app, call_id, source);

                if state.incoming_calls_len() >= INCOMING_CALLS_LIMIT {
                    if let Err(err) = state
                        .send_signaling_message(client::CallReject {
                            call_id: *call_id,
                            rejecting_client_id: own_client_id,
                            reason: CallRejectReason::Busy,
                        })
                        .await
                    {
                        log::warn!("Failed to reject call invite: {err:?}");
                    }
                    return;
                }

                state.add_incoming_call(msg.clone());
                app.emit("signaling:call-invite", msg).ok();

                if *prio && state.config.client.call.enable_priority_calls {
                    state.audio_manager.read().restart(SourceType::PriorityRing);
                } else {
                    state.audio_manager.read().restart(SourceType::Ring);
                }
            }
            ServerMessage::CallAccept(
                ref msg @ shared::CallAccept {
                    ref call_id,
                    ref accepting_client_id,
                },
            ) => {
                log::trace!("Call accept received for call {call_id} from {accepting_client_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                let Some(own_client_id) = state.client_id.as_ref().cloned() else {
                    log::warn!("Cannot handle call accept without own client ID");
                    return;
                };

                state.cancel_unanswered_call_timer(call_id);
                let res = if state.remove_outgoing_call(call_id) {
                    app.emit("signaling:outgoing-call-accepted", msg).ok();

                    match state
                        .init_call(app.clone(), *call_id, accepting_client_id.clone(), None)
                        .await
                    {
                        Ok(sdp) => {
                            state
                                .send_signaling_message(shared::WebrtcOffer {
                                    call_id: *call_id,
                                    from_client_id: own_client_id,
                                    to_client_id: accepting_client_id.clone(),
                                    sdp,
                                })
                                .await
                        }
                        Err(err) => {
                            log::warn!("Failed to start call: {err:?}");

                            let reason: CallErrorReason = err.into();
                            state.emit_call_error(app, *call_id, true, reason);
                            state
                                .send_signaling_message(shared::CallError {
                                    call_id: *call_id,
                                    reason,
                                    message: None,
                                })
                                .await
                        }
                    }
                } else {
                    log::warn!("Received call accept message for peer that is not set as outgoing");
                    state
                        .send_signaling_message(shared::CallError {
                            call_id: *call_id,
                            reason: CallErrorReason::CallFailure,
                            message: None,
                        })
                        .await
                };

                if let Err(err) = res {
                    log::warn!("Failed to send call message: {err:?}");
                }
            }
            ServerMessage::WebrtcOffer(shared::WebrtcOffer {
                call_id,
                from_client_id,
                to_client_id,
                sdp,
            }) => {
                log::trace!("WebRTC offer for call {call_id} received from {from_client_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                let res = match state
                    .init_call(app.clone(), call_id, from_client_id.clone(), Some(sdp))
                    .await
                {
                    Ok(sdp) => {
                        state
                            .send_signaling_message(shared::WebrtcAnswer {
                                call_id,
                                to_client_id: from_client_id,
                                from_client_id: to_client_id,
                                sdp,
                            })
                            .await
                    }
                    Err(err) => {
                        log::warn!("Failed to accept call offer: {err:?}");
                        let reason: CallErrorReason = err.into();
                        state.emit_call_error(app, call_id, true, reason);
                        state
                            .send_signaling_message(shared::CallError {
                                call_id,
                                reason,
                                message: None,
                            })
                            .await
                    }
                };

                if let Err(err) = res {
                    log::warn!("Failed to send call message: {err:?}");
                }
            }
            ServerMessage::WebrtcAnswer(shared::WebrtcAnswer {
                call_id,
                from_client_id,
                sdp,
                ..
            }) => {
                log::trace!("WebRTC answer for call {call_id} received from {from_client_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if let Err(err) = state.accept_call_answer(&from_client_id, sdp).await {
                    log::warn!("Failed to accept answer: {err:?}");
                    if let Err(err) = state
                        .send_signaling_message(shared::CallError {
                            call_id,
                            reason: err.into(),
                            message: None,
                        })
                        .await
                    {
                        log::warn!("Failed to send call end message: {err:?}");
                    }
                };
            }
            ServerMessage::CallEnd(shared::CallEnd {
                call_id,
                ending_client_id,
            }) => {
                log::trace!("Call end for call {call_id} received from {ending_client_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if !state.cleanup_call(&call_id).await {
                    log::debug!("Received call end message for call that is not active");
                }

                state.remove_incoming_call(&call_id);

                app.emit("signaling:call-end", &call_id).ok();
            }
            ServerMessage::CallError(shared::CallError {
                call_id,
                reason,
                message,
            }) => {
                log::trace!(
                    "Call error for call {call_id} received. Reason: {reason:?}, message: {message:?}"
                );

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if !state.cleanup_call(&call_id).await {
                    log::debug!("Received call error message for call that is not active");
                }

                state.remove_outgoing_call(&call_id);
                state.remove_incoming_call(&call_id);

                state.cancel_unanswered_call_timer(&call_id);

                state.emit_call_error(app, call_id, false, reason);
            }
            ServerMessage::CallCancelled(server::CallCancelled { call_id, reason }) => {
                log::trace!("Call {call_id} cancelled. Reason: {reason:?}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                // Stop any active webrtc call
                state.cleanup_call(&call_id).await;

                // Remove from outgoing and incoming states
                state.remove_outgoing_call(&call_id);
                state.remove_incoming_call(&call_id);

                state.cancel_unanswered_call_timer(&call_id);

                match reason {
                    CallCancelReason::AnsweredElsewhere(_) | CallCancelReason::CallerCancelled => {
                        app.emit("signaling:call-end", &call_id).ok();
                    }
                    CallCancelReason::Disconnected => {
                        app.emit("signaling:force-call-end", &call_id).ok();
                    }
                    CallCancelReason::Rejected(_) => {
                        app.emit("signaling:call-reject", &call_id).ok();
                    }
                    CallCancelReason::Errored(reason) => {
                        state.emit_call_error(app, call_id, false, reason);
                    }
                }
            }
            ServerMessage::WebrtcIceCandidate(shared::WebrtcIceCandidate {
                call_id,
                from_client_id,
                candidate,
                ..
            }) => {
                log::trace!("ICE candidate for call {call_id} received from {from_client_id}");

                let state = app.state::<AppState>();
                let state = state.lock().await;

                state.set_remote_ice_candidate(&call_id, candidate).await;
            }
            ServerMessage::ClientConnected(server::ClientConnected { client }) => {
                log::trace!("Client connected: {client:?}");

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.clients.push(client.clone());
                }

                app.emit("signaling:client-connected", client).ok();
            }
            ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
                log::trace!("Client disconnected: {client_id:?}");

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.clients.retain(|c| c.id != client_id);
                }

                app.emit("signaling:client-disconnected", client_id).ok();
            }
            ServerMessage::ClientList(server::ClientList { clients }) => {
                log::trace!("Received client list: {} clients connected", clients.len());

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.clients = clients.clone();
                }

                app.emit("signaling:client-list", clients).ok();
            }
            ServerMessage::ClientInfo(info) => {
                log::trace!("Received client info: {info:?}");

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    if let Some(existing) = state.clients.iter_mut().find(|c| c.id == info.id) {
                        *existing = info.clone();
                    } else {
                        state.clients.push(info.clone());
                    }
                }

                app.emit("signaling:client-connected", info).ok();
            }
            ServerMessage::SessionInfo(session_info) => {
                log::trace!(
                    "Received session info for client {:?}: {}",
                    &session_info.client,
                    &session_info.profile
                );

                if let SessionProfile::Changed(ref active_profile) = session_info.profile {
                    log::debug!("Active profile changed: {active_profile}");
                }

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.session_info = Some(session_info.clone());
                }

                app.emit("signaling:connected", session_info).ok();
            }
            ServerMessage::StationList(server::StationList { stations }) => {
                log::trace!(
                    "Received station list: {} stations covered ({} by self)",
                    stations.len(),
                    stations.iter().filter(|s| s.own).count()
                );

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    state.stations = stations.clone();
                }

                app.emit("signaling:station-list", stations).ok();
            }
            ServerMessage::StationChanges(server::StationChanges { changes }) => {
                log::trace!("Received station changes: {changes:?}");

                {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    let own_position_id = state
                        .session_info
                        .as_ref()
                        .and_then(|s| s.client.position_id.clone());

                    for change in &changes {
                        match change {
                            StationChange::Online {
                                station_id,
                                position_id,
                            } => {
                                state.stations.push(server::StationInfo {
                                    id: station_id.clone(),
                                    own: own_position_id.as_ref() == Some(position_id),
                                });
                            }
                            StationChange::Handoff {
                                station_id,
                                to_position_id,
                                ..
                            } => {
                                if let Some(s) =
                                    state.stations.iter_mut().find(|s| s.id == *station_id)
                                {
                                    s.own = own_position_id.as_ref() == Some(to_position_id);
                                }
                            }
                            StationChange::Offline { station_id } => {
                                state.stations.retain(|s| s.id != *station_id);
                            }
                        }
                    }
                }

                app.emit("signaling:station-changes", changes).ok();
            }
            ServerMessage::Error(shared::Error {
                reason,
                client_id,
                call_id,
            }) => match reason {
                ErrorReason::MalformedMessage => {
                    log::warn!("Received malformed error message from signaling server");

                    app.emit::<FrontendError>(
                        "error",
                        FrontendError::from(Error::from(SignalingRuntimeError::ServerError(
                            reason,
                        )))
                        .timeout(5000),
                    )
                    .ok();
                }
                ErrorReason::Internal(ref msg) => {
                    log::warn!("Received internal error message from signaling server: {msg}");

                    app.emit::<FrontendError>(
                        "error",
                        FrontendError::from(Error::from(SignalingRuntimeError::ServerError(
                            reason,
                        ))),
                    )
                    .ok();
                }
                ErrorReason::UnexpectedMessage(ref msg) => {
                    log::warn!("Received unexpected message error from signaling server: {msg}");

                    app.emit::<FrontendError>(
                        "error",
                        FrontendError::from(Error::from(SignalingRuntimeError::ServerError(
                            reason,
                        ))),
                    )
                    .ok();
                }
                ErrorReason::RateLimited { retry_after_secs } => {
                    log::warn!(
                        "Received rate limited error from signaling server, rate limited for {retry_after_secs}"
                    );

                    if let Some(call_id) = call_id {
                        let state = app.state::<AppState>();
                        let mut state = state.lock().await;

                        state.cleanup_call(&call_id).await;
                        state.remove_outgoing_call(&call_id);
                        state.remove_incoming_call(&call_id);

                        app.emit("signaling:force-call-end", call_id).ok();
                    }
                    app.emit::<FrontendError>(
                        "error",
                        FrontendError::from(Error::from(SignalingRuntimeError::RateLimited(
                            retry_after_secs.into(),
                        ))),
                    )
                    .ok();
                }
                ErrorReason::PeerConnection => {
                    let client_id = client_id.unwrap_or_default();
                    log::warn!(
                        "Received peer connection error from signaling server with peer {client_id}"
                    );

                    app.emit::<FrontendError>(
                        "error",
                        FrontendError::from(Error::from(SignalingRuntimeError::ServerError(
                            ErrorReason::PeerConnection,
                        ))),
                    )
                    .ok();
                }
                ErrorReason::ClientNotFound => {
                    let client_id = client_id.unwrap_or_default();
                    log::warn!(
                        "Received client not found error from signaling server with peer {client_id}"
                    );

                    app.emit("signaling:client-not-found", client_id).ok();
                }
            },
            ServerMessage::Disconnected(_) | ServerMessage::LoginFailure(_) => {}
        }
    }

    async fn cleanup_signaling(&mut self, app: &AppHandle) {
        self.incoming_calls.clear();
        self.outgoing_call = None;
        self.clear_session_cache();

        {
            let mut audio_manager = self.audio_manager.write();
            audio_manager.stop(SourceType::Ring);
            audio_manager.stop(SourceType::PriorityRing);
            audio_manager.stop(SourceType::Ringback);

            audio_manager.detach_call_output();
            audio_manager.detach_input_device();
        }

        self.keybind_engine.read().await.set_call_active(false);

        if let Some(call_id) = self.active_call_id().cloned() {
            self.cleanup_call(&call_id).await;
        };
        let call_ids = self.held_calls.keys().cloned().collect::<Vec<_>>();
        for call_id in call_ids {
            self.cleanup_call(&call_id).await;
            app.emit("signaling:call-end", &call_id).ok();
        }

        if let Some(guard) = self.unanswered_call_guard.take() {
            log::trace!(
                "Cancelling unanswered call timer for call {}",
                guard.call_id
            );
            guard.cancel.cancel();
            guard.handle.abort();
        }
    }
}
