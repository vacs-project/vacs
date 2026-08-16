use crate::app::state::calls::Call;
use crate::app::state::http::HttpState;
use crate::app::state::webrtc::{AppStateWebrtcExt, UnansweredCallGuard};
use crate::app::state::{AppState, AppStateInner, sealed};
use crate::audio::source_type::SourceType;
use crate::config::BackendEndpoint;
use crate::error::{CallErrorOrigin, Error, FrontendError};
use crate::signaling::auth::TauriTokenProvider;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use vacs_signaling::client::{SignalingClient, SignalingEvent, State};
use vacs_signaling::error::{SignalingError, SignalingRuntimeError};
use vacs_signaling::protocol::http::webrtc::IceConfig;
use vacs_signaling::protocol::vatsim::{ClientId, PositionId, StationChange};
use vacs_signaling::protocol::ws::client::{CallInvite, CallRejectReason, ClientMessage};
use vacs_signaling::protocol::ws::server::{
    CallCancelReason, DisconnectReason, LoginFailureReason, ServerMessage, SessionProfile,
};
use vacs_signaling::protocol::ws::shared::{
    CallErrorReason, CallId, CallSource, CallTarget, ErrorReason,
};
use vacs_signaling::protocol::ws::{client, server, shared};
use vacs_signaling::transport::tokio::TokioTransport;
use vacs_webrtc::error::WebrtcError;

const INCOMING_CALLS_LIMIT: usize = 5;
const WS_LOGIN_TIMEOUT: Duration = Duration::from_secs(10);

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
    async fn try_send_call_error(
        &mut self,
        call_id: CallId,
        reason: CallErrorReason,
        message: Option<String>,
    );
    async fn try_send_call_error_with_client_id<F>(
        &mut self,
        call_id: CallId,
        make_reason: F,
        message: Option<String>,
    ) where
        F: FnOnce(ClientId) -> CallErrorReason;
    fn set_client_id(&mut self, client_id: Option<ClientId>);
    fn current_call_id(&self) -> Option<CallId>;
    fn incoming_calls_len(&self) -> usize;
    fn remove_incoming_call(&mut self, call_id: CallId) -> bool;
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
    fn start_unanswered_call_timer_for_targets(
        &mut self,
        app: &AppHandle,
        call_id: &CallId,
        targets: HashSet<CallTarget>,
    );
    fn cancel_unanswered_call_timers_for_targets<'a>(
        &mut self,
        call_id: &CallId,
        targets: impl Iterator<Item = &'a CallTarget>,
    );
    fn cancel_all_unanswered_call_timers(&mut self, call_id: CallId);
    async fn accept_call(
        &mut self,
        app: &AppHandle,
        call_id: Option<CallId>,
    ) -> Result<bool, Error>;
    async fn invite_to_call(
        &mut self,
        app: &AppHandle,
        source: CallSource,
        targets: HashSet<CallTarget>,
        prio: bool,
    ) -> Result<CallId, Error>;
    async fn end_call(&mut self, app: &AppHandle, call_id: CallId) -> Result<bool, Error>;
    fn clear_session_cache(&mut self);
    fn current_call(&self, call_id: CallId) -> Option<&Call>;
    fn current_call_mut(&mut self, call_id: CallId) -> Option<&mut Call>;
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

        self.cleanup_signaling().await;
        app.emit("signaling:disconnected", Value::Null).ok();
        self.signaling_client.disconnect().await;

        log::debug!("Successfully disconnected from signaling server");
    }

    async fn handle_signaling_connection_closed(&mut self, app: &AppHandle) {
        log::info!("Handling signaling server connection closed");

        self.cleanup_signaling().await;

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

    async fn try_send_call_error(
        &mut self,
        call_id: CallId,
        reason: CallErrorReason,
        message: Option<String>,
    ) {
        if let Err(err) = self
            .send_signaling_message(shared::CallError {
                call_id,
                reason,
                message,
            })
            .await
        {
            log::warn!("Failed to send call error signaling message: {err:?}");
        }
    }

    async fn try_send_call_error_with_client_id<F>(
        &mut self,
        call_id: CallId,
        make_reason: F,
        message: Option<String>,
    ) where
        F: FnOnce(ClientId) -> CallErrorReason,
    {
        let Ok(own_client_id) = self.require_client_id() else {
            return;
        };
        self.try_send_call_error(call_id, make_reason(own_client_id), message)
            .await;
    }

    fn set_client_id(&mut self, client_id: Option<ClientId>) {
        self.client_id = client_id;
    }

    fn current_call_id(&self) -> Option<CallId> {
        self.current_call.as_ref().map(|c| c.call_id())
    }

    fn incoming_calls_len(&self) -> usize {
        self.incoming_calls.len()
    }

    fn remove_incoming_call(&mut self, call_id: CallId) -> bool {
        let found = self.incoming_calls.remove(&call_id).is_some();
        self.stop_ringing_if_no_incoming_calls();
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

    fn start_unanswered_call_timer_for_targets(
        &mut self,
        app: &AppHandle,
        call_id: &CallId,
        targets: HashSet<CallTarget>,
    ) {
        self.cancel_unanswered_call_timers_for_targets(call_id, targets.iter());

        let Ok(own_client_id) = self.require_client_id() else {
            return;
        };

        let timeout = Duration::from_secs(self.config.client.auto_hangup_seconds);
        if timeout.is_zero() {
            return;
        }

        for target in targets.into_iter() {
            let cancel = self.shutdown_token.child_token();

            let handle = tauri::async_runtime::spawn({
                let app = app.clone();
                let cancel = cancel.clone();

                let own_client_id = own_client_id.clone();
                let target = target.clone();

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
                            } // TODO: This should be a CallDropTarget message

                            state.cleanup_current_call(call_id).await;

                            state.unanswered_call_guards.remove(&target);

                            state.emit_call_error(&app, call_id, false, target.into(), CallErrorReason::AutoHangup);
                        }
                    }
                }
            });

            self.unanswered_call_guards.insert(
                target,
                UnansweredCallGuard {
                    call_id: *call_id,
                    cancel,
                    handle,
                },
            );
        }
    }

    fn cancel_unanswered_call_timers_for_targets<'a>(
        &mut self,
        call_id: &CallId,
        targets: impl Iterator<Item = &'a CallTarget>,
    ) {
        for target in targets {
            if let Some(guard) = self.unanswered_call_guards.remove(target)
                && &guard.call_id == call_id
            {
                guard.cancel.cancel();
                guard.handle.abort();
            }
        }
    }

    fn cancel_all_unanswered_call_timers<'a>(&mut self, call_id: CallId) {
        self.unanswered_call_guards.retain(|_, guard| {
            if guard.call_id == call_id {
                guard.cancel.cancel();
                guard.handle.abort();

                false
            } else {
                true
            }
        });
    }

    async fn accept_call(
        &mut self,
        app: &AppHandle,
        call_id: Option<CallId>,
    ) -> Result<bool, Error> {
        let own_client_id = self.require_client_id()?;

        let call_id = match call_id.or_else(|| self.incoming_calls.keys().next().copied()) {
            Some(id) => id,
            None => return Ok(false),
        };

        if self.current_call.is_some() {
            log::debug!("Tried to accept call {call_id}, but another call is already active");
            return Err(WebrtcError::CallActive.into());
        }

        let Some(call) = self.incoming_calls.remove(&call_id) else {
            log::warn!("Tried to accept call {call_id} that is not incoming");
            return Ok(false);
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

        if let Err(err) = self
            .send_signaling_message(client::CallAccept {
                call_id,
                accepting_client_id: own_client_id,
            })
            .await
        {
            // The call was not accepted, so it must keep ringing
            self.incoming_calls.insert(call_id, call);
            return Err(err);
        }

        self.current_call = Some(call);

        self.stop_ringing_if_no_incoming_calls();

        app.emit("signaling:accept-incoming-call", call_id).ok();

        Ok(true)
    }

    async fn invite_to_call(
        &mut self,
        app: &AppHandle,
        source: CallSource,
        targets: HashSet<CallTarget>,
        prio: bool,
    ) -> Result<CallId, Error> {
        let call_id = if let Some(current_call) = self.current_call.as_ref() {
            let call_id = current_call.call_id();
            log::debug!("Starting call {call_id} as source {source:?} with targets {targets:?}");
            call_id
        } else {
            let call_id = CallId::new();
            log::debug!(
                "Inviting targets {targets:?} to existing call {call_id} as source {source:?}"
            );
            call_id
        };

        let invite = CallInvite {
            call_id,
            targets: targets.clone(),
            source,
            prio,
        };
        self.send_signaling_message(invite.clone()).await?;

        if let Some(current_call) = self.current_call.as_mut() {
            current_call.add_invited_targets(targets);
        } else {
            self.current_call = Some(Call::from_invite(&invite, &self.shutdown_token));
        }

        self.start_unanswered_call_timer_for_targets(app, &invite.call_id, invite.targets.clone());

        self.audio_manager.read().restart(SourceType::Ringback);

        Ok(call_id)
    }

    async fn end_call(&mut self, app: &AppHandle, call_id: CallId) -> Result<bool, Error> {
        let own_client_id = self.require_client_id()?;

        log::debug!("Ending call {call_id}");

        self.cancel_all_unanswered_call_timers(call_id);

        self.send_signaling_message(shared::CallEnd {
            call_id,
            ending_client_id: own_client_id,
        })
        .await?;

        let found = self.cleanup_current_call(call_id).await;

        app.emit("signaling:force-call-end", call_id).ok();

        Ok(found)
    }

    fn clear_session_cache(&mut self) {
        self.connection_state = ConnectionState::Disconnected;
        self.session_info = None;
        self.stations.clear();
        self.clients.clear();
    }

    fn current_call(&self, call_id: CallId) -> Option<&Call> {
        self.current_call
            .as_ref()
            .filter(|call| call.call_id() == call_id)
    }
    fn current_call_mut(&mut self, call_id: CallId) -> Option<&mut Call> {
        self.current_call
            .as_mut()
            .filter(|call| call.call_id() == call_id)
    }
}

impl AppStateInner {
    /// Returns the own client ID, logging when it is unknown.
    fn require_client_id(&self) -> Result<ClientId, Error> {
        self.client_id.clone().ok_or_else(|| {
            log::warn!("Own client ID unknown, ignoring call action");
            Error::Unauthorized
        })
    }

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

    fn stop_ringing_if_no_incoming_calls(&self) {
        if self.incoming_calls.is_empty() {
            let audio_manager = self.audio_manager.read();
            audio_manager.stop(SourceType::Ring);
            audio_manager.stop(SourceType::PriorityRing);
        }
    }

    fn stop_ringback_if_no_invited_targets(&self, call_id: CallId) {
        if self
            .current_call(call_id)
            .is_some_and(|call| call.invited_targets().is_empty())
        {
            self.audio_manager.read().stop(SourceType::Ringback);
        }
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
                    client_info.display_name,
                    client_info.frequency,
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
            ServerMessage::CallInvitation(
                ref msg @ server::CallInvitation {
                    ref call_id,
                    ref source,
                    ref target,
                    ref invited_targets,
                    ref joined_participants,
                    ref prio,
                },
            ) => {
                let caller_id = &source.client_id;
                log::trace!(
                    "Call invite received from {caller_id} for target {target:?} (invited targets: {invited_targets:?}, joined participants: {joined_participants:?})"
                );

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if state.config.client.ignored.contains(caller_id) {
                    log::trace!("Ignoring call invite from {caller_id}");
                    return;
                }

                let Ok(own_client_id) = state.require_client_id() else {
                    return;
                };

                state.add_incoming_call_to_call_list(app, call_id, source); // TODO redefine call list

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

                let call = Call::from_invitation(msg, &state.shutdown_token);
                state.incoming_calls.insert(*call_id, call);

                app.emit("signaling:call-invitation", msg).ok();

                if *prio && state.config.client.call.enable_priority_calls {
                    state.audio_manager.read().restart(SourceType::PriorityRing);
                } else {
                    state.audio_manager.read().restart(SourceType::Ring);
                }
            }
            ServerMessage::CallUpdate(
                ref msg @ server::CallUpdate {
                    ref call_id,
                    ref invited_targets,
                    ref joined_participants,
                },
            ) => {
                log::trace!(
                    "Call update received for call {call_id} (invited targets: {invited_targets:?}, joined participants: {joined_participants:?})"
                );

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                let Ok(own_client_id) = state.require_client_id() else {
                    return;
                };

                if let Entry::Occupied(mut entry) = state.incoming_calls.entry(*call_id) {
                    if invited_targets.is_empty() && joined_participants.is_empty() {
                        log::debug!(
                            "Call {call_id} has no participants left, removing incoming call"
                        );

                        entry.remove();
                        state.stop_ringing_if_no_incoming_calls();

                        app.emit("signaling:force-call-end", call_id).ok();
                    } else {
                        log::trace!("Updating incoming call");

                        let incoming_call = entry.get_mut();
                        incoming_call.update(
                            &own_client_id,
                            invited_targets.clone(),
                            joined_participants.clone(),
                        );

                        app.emit("signaling:call-update", msg).ok();
                    }

                    return;
                }

                let Some(current_call) = state.current_call_mut(*call_id) else {
                    log::debug!(
                        "Received call update for call {call_id} that is not current, ignoring"
                    );
                    return;
                };

                let was_active = current_call.is_active(&own_client_id);
                let (newly_joined, left) = current_call.update(
                    &own_client_id,
                    invited_targets.clone(),
                    joined_participants.clone(),
                );
                let is_active = current_call.is_active(&own_client_id);

                if (invited_targets.is_empty() && joined_participants.is_empty())
                    || (was_active && !is_active)
                {
                    log::debug!("Call {call_id} ended locally, cleaning up");

                    state.cancel_all_unanswered_call_timers(*call_id);
                    state.cleanup_current_call(*call_id).await;

                    app.emit("signaling:force-call-end", call_id).ok();
                    return;
                }

                if invited_targets.is_empty() {
                    state.cancel_all_unanswered_call_timers(*call_id);
                    state.audio_manager.read().stop(SourceType::Ringback);
                } else {
                    state.cancel_unanswered_call_timers_for_targets(call_id, newly_joined.values());
                }

                for peer_id in left {
                    state.cleanup_call_peer(*call_id, &peer_id).await;
                }

                // For every pair of participants, the one with the lower client ID creates
                // the WebRTC offer and the other one answers, independent of the order in
                // which the call updates arrive.
                let mut attempted_peers = 0;
                for (peer_id, target) in newly_joined {
                    if own_client_id >= peer_id {
                        continue;
                    }
                    attempted_peers += 1;

                    match state
                        .negotiate_peer(
                            app.clone(),
                            *call_id,
                            peer_id.clone(),
                            &own_client_id,
                            None,
                        )
                        .await
                    {
                        Ok(sdp) => {
                            if let Err(err) = state
                                .send_signaling_message(shared::WebrtcOffer {
                                    call_id: *call_id,
                                    from_client_id: own_client_id.clone(),
                                    to_client_id: peer_id.clone(),
                                    sdp,
                                })
                                .await
                            {
                                log::warn!(
                                    "Failed to send WebRTC offer to peer {peer_id} in call {call_id}: {err:?}"
                                );
                                state.cleanup_call_peer(*call_id, &peer_id).await;
                                state
                                    .try_send_call_error(
                                        *call_id,
                                        CallErrorReason::SignalingFailure(own_client_id.clone()),
                                        None,
                                    )
                                    .await;
                            }
                        }
                        Err(err) => {
                            log::warn!(
                                "Failed to negotiate connection to peer {peer_id} for call {call_id}: {err:?}"
                            );

                            let reason = err.into_call_error_reason(own_client_id.clone());
                            state
                                .try_send_call_error(*call_id, reason.clone(), None)
                                .await;
                            state.emit_call_error(app, *call_id, true, target.into(), reason);
                        }
                    }
                }

                if attempted_peers > 0 && state.end_call_if_no_peers(*call_id).await {
                    log::warn!(
                        "Failed to connect to any participant of call {call_id}, ending call"
                    );

                    app.emit("signaling:force-call-end", call_id).ok();
                    return;
                }

                app.emit("signaling:call-update", msg).ok();
            }
            ServerMessage::WebrtcOffer(shared::WebrtcOffer {
                call_id,
                from_client_id,
                sdp,
                ..
            }) => {
                log::trace!("Received WebRTC offer from peer {from_client_id} for call {call_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                let Ok(own_client_id) = state.require_client_id() else {
                    return;
                };

                let res = state
                    .negotiate_peer(
                        app.clone(),
                        call_id,
                        from_client_id.clone(),
                        &own_client_id,
                        Some(sdp),
                    )
                    .await;

                let res = match res {
                    Ok(sdp) => {
                        state
                            .send_signaling_message(shared::WebrtcAnswer {
                                call_id,
                                to_client_id: from_client_id,
                                from_client_id: own_client_id,
                                sdp,
                            })
                            .await
                    }
                    Err(err) => {
                        log::warn!("Failed to accept call offer: {err:?}");

                        let reason: CallErrorReason = err.into_call_error_reason(own_client_id);
                        state.emit_call_error(
                            app,
                            call_id,
                            true,
                            from_client_id.into(),
                            reason.clone(),
                        );

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
                log::trace!("Received WebRTC answer from peer {from_client_id} for call {call_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                let Ok(own_client_id) = state.require_client_id() else {
                    return;
                };

                if let Err(err) = state
                    .accept_call_answer(call_id, &from_client_id, sdp)
                    .await
                {
                    log::warn!("Failed to accept answer: {err:?}");
                    if let Err(err) = state
                        .send_signaling_message(shared::CallError {
                            call_id,
                            reason: err.into_call_error_reason(own_client_id),
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
                log::trace!("Received call end from peer {ending_client_id} for call {call_id}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if !state.cleanup_current_call(call_id).await {
                    log::debug!("Received call end message for call that is not active");
                }

                state.remove_incoming_call(call_id);
                state.cancel_all_unanswered_call_timers(call_id);

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

                match &reason {
                    CallErrorReason::TargetsNotFound(targets) => {
                        let state = app.state::<AppState>();
                        let mut state = state.lock().await;

                        let Some(current_call) = state.current_call_mut(call_id) else {
                            log::debug!("Received call error for unknown call {call_id}, ignoring");
                            return;
                        };

                        current_call.remove_invited_targets(targets);
                        if current_call.is_empty() {
                            state.cancel_all_unanswered_call_timers(call_id);
                            state.cleanup_current_call(call_id).await;
                        } else {
                            state.cancel_unanswered_call_timers_for_targets(
                                &call_id,
                                targets.iter(),
                            );
                        }
                        state.stop_ringback_if_no_invited_targets(call_id);

                        state.emit_call_error(app, call_id, false, targets.into(), reason);
                    }
                    CallErrorReason::CallActive
                    | CallErrorReason::NotParticipant
                    | CallErrorReason::CallNotFound
                    | CallErrorReason::CallFailure
                    | CallErrorReason::Other => {
                        let state = app.state::<AppState>();
                        let mut state = state.lock().await;

                        if !state.cleanup_current_call(call_id).await {
                            log::debug!(
                                "Received call error message for call {call_id} that is not active"
                            );
                        }

                        state.remove_incoming_call(call_id);

                        state.cancel_all_unanswered_call_timers(call_id);

                        state.emit_call_error(app, call_id, false, CallErrorOrigin::Call, reason);
                    }
                    CallErrorReason::WebrtcFailure(erroring_client_id)
                    | CallErrorReason::AudioFailure(erroring_client_id)
                    | CallErrorReason::SignalingFailure(erroring_client_id) => {
                        let state = app.state::<AppState>();
                        let state = state.lock().await;

                        if state.current_call(call_id).is_none() {
                            log::debug!(
                                "Received call error for peer {erroring_client_id} in unknown call {call_id}, ignoring"
                            );
                            return;
                        };

                        // Errors are only informal and emitted to the frontend - actual cleanup
                        // will be performed by CallUpdate received directly afterward.
                        state.emit_call_error(
                            app,
                            call_id,
                            false,
                            erroring_client_id.into(),
                            reason,
                        );
                    }
                    CallErrorReason::NotConferenceLeader(target)
                    | CallErrorReason::AlreadyParticipant(target) => {
                        let state = app.state::<AppState>();
                        let mut state = state.lock().await;

                        let Some(current_call) = state.current_call_mut(call_id) else {
                            log::debug!("Received call error for unknown call {call_id}, ignoring");
                            return;
                        };

                        let targets = HashSet::from([target.clone()]);
                        current_call.remove_invited_targets(&targets);
                        if current_call.is_empty() {
                            state.cancel_all_unanswered_call_timers(call_id);
                            state.cleanup_current_call(call_id).await;
                        }

                        state.cancel_unanswered_call_timers_for_targets(&call_id, targets.iter());
                        state.stop_ringback_if_no_invited_targets(call_id);

                        state.emit_call_error(app, call_id, false, targets.into(), reason);
                    }
                    CallErrorReason::AutoHangup => {} // should not be sent in a CallError message
                }
            }
            ServerMessage::CallCancelled(server::CallCancelled {
                call_id,
                targets,
                reason,
            }) => {
                log::trace!("Call {call_id} cancelled for targets {targets:?}: {reason:?}");

                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                match reason {
                    CallCancelReason::AnsweredElsewhere(_) | CallCancelReason::CallerCancelled => {
                        state.cleanup_current_call(call_id).await;

                        state.remove_incoming_call(call_id);

                        state.cancel_all_unanswered_call_timers(call_id);

                        app.emit("signaling:call-end", &call_id).ok();
                    }
                    CallCancelReason::Disconnected => {
                        let Some(current_call) = state.current_call_mut(call_id) else {
                            log::debug!(
                                "Received call cancelled for unknown call {call_id}, ignoring"
                            );
                            return;
                        };

                        current_call.remove_invited_targets(&targets);
                        if current_call.is_empty() {
                            state.cancel_all_unanswered_call_timers(call_id);
                            state.cleanup_current_call(call_id).await;

                            app.emit("signaling:force-call-end", call_id).ok();
                        } else {
                            let update = server::CallUpdate::from(&*current_call);

                            state.cancel_unanswered_call_timers_for_targets(
                                &call_id,
                                targets.iter(),
                            );
                            state.stop_ringback_if_no_invited_targets(call_id);

                            app.emit("signaling:call-update", update).ok();
                        }
                    }
                    CallCancelReason::Rejected(_) => {
                        #[derive(Clone, Serialize)]
                        #[serde(rename_all = "camelCase")]
                        struct RejectTargets {
                            call_id: CallId,
                            targets: HashSet<CallTarget>,
                        }

                        let Some(current_call) = state.current_call_mut(call_id) else {
                            log::debug!(
                                "Received call cancelled for unknown call {call_id}, ignoring"
                            );
                            return;
                        };

                        current_call.remove_invited_targets(&targets);
                        if current_call.is_empty() {
                            state.cancel_all_unanswered_call_timers(call_id);
                            state.cleanup_current_call(call_id).await;
                        }

                        state.cancel_unanswered_call_timers_for_targets(&call_id, targets.iter());
                        state.stop_ringback_if_no_invited_targets(call_id);

                        app.emit("signaling:call-reject", RejectTargets { call_id, targets })
                            .ok();
                    }
                    CallCancelReason::Errored(reason) => {
                        let Some(current_call) = state.current_call_mut(call_id) else {
                            log::debug!(
                                "Received call cancelled for unknown call {call_id}, ignoring"
                            );
                            return;
                        };

                        current_call.remove_invited_targets(&targets);
                        if current_call.is_empty() {
                            state.cancel_all_unanswered_call_timers(call_id);
                            state.cleanup_current_call(call_id).await;
                        }

                        state.cancel_unanswered_call_timers_for_targets(&call_id, targets.iter());
                        state.stop_ringback_if_no_invited_targets(call_id);

                        state.emit_call_error(app, call_id, false, targets.into(), reason);
                    }
                }
            }
            ServerMessage::WebrtcIceCandidate(shared::WebrtcIceCandidate {
                call_id,
                from_client_id,
                candidate,
                ..
            }) => {
                log::trace!("Received ICE candidate from peer {from_client_id} for call {call_id}");

                let state = app.state::<AppState>();
                let state = state.lock().await;

                state
                    .set_remote_ice_candidate(call_id, &from_client_id, candidate)
                    .await;
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
                    session_info.client,
                    session_info.profile
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
                    // TODO: this needs to have targets, which invite was rate limited, to not remove the entire call, but only those targets
                    log::warn!(
                        "Received rate limited error from signaling server, rate limited for {retry_after_secs}"
                    );

                    if let Some(call_id) = call_id {
                        let state = app.state::<AppState>();
                        let mut state = state.lock().await;

                        state.cleanup_current_call(call_id).await;
                        state.remove_incoming_call(call_id);
                        state.cancel_all_unanswered_call_timers(call_id);

                        app.emit("signaling:force-call-end", call_id).ok(); // TODO: emit force-call-end if all targets gone, or call-update if only some
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
            ServerMessage::Disconnected(_)
            | ServerMessage::LoginFailure(_)
            | ServerMessage::Unknown => {}
        }
    }

    async fn cleanup_signaling(&mut self) {
        self.incoming_calls.clear();
        self.clear_session_cache();

        if let Some(call_id) = self.current_call_id() {
            self.cleanup_current_call(call_id).await;
        }

        {
            let mut audio_manager = self.audio_manager.write();
            audio_manager.stop(SourceType::Ring);
            audio_manager.stop(SourceType::PriorityRing);
            audio_manager.stop(SourceType::Ringback);

            audio_manager.detach_all_call_outputs();
            audio_manager.detach_input_device();
        }

        self.keybind_engine.read().await.set_call_active(false);

        for (target, guard) in self.unanswered_call_guards.drain() {
            log::trace!(
                "Cancelling unanswered call timer for call {} and target {target:?}",
                guard.call_id
            );
            guard.cancel.cancel();
            guard.handle.abort();
        }
    }
}
