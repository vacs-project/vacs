use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::{AppState, AppStateInner, sealed};
use crate::audio::source_type::SourceType;
use crate::error::{CallError, Error};
use anyhow::Context;
use std::collections::HashSet;
use std::fmt::{Debug, Formatter};
use std::time::{Duration, UNIX_EPOCH};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use vacs_signaling::protocol::http::webrtc::IceConfig;
use vacs_signaling::protocol::vatsim::ClientId;
use vacs_signaling::protocol::ws::shared;
use vacs_signaling::protocol::ws::shared::{CallErrorReason, CallId, CallTarget};
use vacs_webrtc::error::WebrtcError;
use vacs_webrtc::{Peer, PeerConnectionState, PeerEvent};

const ENCODED_AUDIO_FRAME_BUFFER_SIZE: usize = 512;
const ICE_CONFIG_EXPIRY_LEEWAY: Duration = Duration::from_mins(15);

/// Extra key added to the JSON-serialized session descriptions we signal, advertising that this
/// client can replace the peer connection of an active call (relay reconnect). Older clients
/// deserialize the JSON with serde, which silently ignores unknown fields, so the marker is
/// invisible to them; a reconnect is only ever initiated towards peers that advertised it.
const SDP_RECONNECT_CAPABILITY_KEY: &str = "vacsSupportsReconnect";

fn tag_reconnect_capability(sdp: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(sdp) {
        Ok(mut value) => {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(SDP_RECONNECT_CAPABILITY_KEY.to_string(), true.into());
                value.to_string()
            } else {
                sdp.to_string()
            }
        }
        Err(_) => sdp.to_string(),
    }
}

fn has_reconnect_capability(sdp: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(sdp)
        .ok()
        .and_then(|value| {
            value
                .get(SDP_RECONNECT_CAPABILITY_KEY)
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(false)
}

#[derive(Debug)]
pub struct UnansweredCallGuard {
    pub call_id: CallId,
    pub cancel: CancellationToken,
    pub handle: JoinHandle<()>,
}

pub struct WebrtcCall {
    pub(super) call_id: CallId,
    pub(super) peer_id: ClientId,
    peer: Peer,
    /// Cancels the peer events task when the peer is replaced or the call is cleaned up, so
    /// events of a stale peer (e.g. its Closed state) cannot tear down the current call.
    events_cancel: CancellationToken,
    /// Whether this peer connection is a replacement established by an in-call reconnect.
    /// Prevents reconnect loops and suppresses the call start sound when it connects.
    reconnected: bool,
    /// True while a replacement peer connection has not connected yet. Call audio is detached
    /// during the swap, so [`AppStateWebrtcExt::cleanup_call`] uses this to still play the call
    /// end sound if the reconnect fails.
    reconnect_pending: bool,
    /// Whether the peer's offer/answer advertised support for in-call reconnects
    /// ([`SDP_RECONNECT_CAPABILITY_KEY`]). Reconnects towards older clients would fail the call.
    peer_supports_reconnect: bool,
}

impl Debug for WebrtcCall {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

pub trait AppStateWebrtcExt: sealed::Sealed {
    async fn init_call(
        &mut self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        offer_sdp: Option<String>,
    ) -> Result<String, Error>;
    fn is_active_call_peer(&self, call_id: &CallId, peer_id: &ClientId) -> bool;
    async fn reaccept_call_offer(
        &mut self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        offer_sdp: String,
    ) -> Result<String, Error>;
    async fn accept_call_answer(
        &mut self,
        peer_id: &ClientId,
        answer_sdp: String,
    ) -> Result<(), Error>;
    async fn set_remote_ice_candidate(&self, call_id: &CallId, candidate: String);
    async fn cleanup_call(&mut self, call_id: &CallId) -> bool;
    fn emit_call_error(
        &self,
        app: &AppHandle,
        call_id: CallId,
        is_local: bool,
        targets: HashSet<CallTarget>,
        reason: CallErrorReason,
    );
    fn active_call_id(&self) -> Option<&CallId>;
    fn set_ice_config(&mut self, config: IceConfig);
    fn is_ice_config_expired(&self) -> bool;
}

impl AppStateWebrtcExt for AppStateInner {
    async fn init_call(
        &mut self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        offer_sdp: Option<String>,
    ) -> Result<String, Error> {
        if self.active_webrtc_call.is_some() {
            return Err(WebrtcError::CallActive.into());
        }

        let (peer, events_rx) =
            Peer::new(self.config.ice.clone(), self.config.client.call.force_relay)
                .await
                .context("Failed to create WebRTC peer")?;

        // As the offerer, the peer's capabilities are only known once its answer arrives; see
        // accept_call_answer.
        let peer_supports_reconnect = offer_sdp.as_deref().is_some_and(has_reconnect_capability);

        let sdp = if let Some(sdp) = offer_sdp {
            peer.accept_offer(sdp)
                .await
                .context("Failed to accept WebRTC offer")?
        } else {
            peer.create_offer()
                .await
                .context("Failed to create WebRTC offer")?
        };

        let events_cancel = CancellationToken::new();
        spawn_peer_events_task(
            app,
            call_id,
            peer_id.clone(),
            events_rx,
            events_cancel.clone(),
        );

        self.active_webrtc_call = Some(WebrtcCall {
            call_id,
            peer_id,
            peer,
            events_cancel,
            reconnected: false,
            reconnect_pending: false,
            peer_supports_reconnect,
        });

        Ok(tag_reconnect_capability(&sdp))
    }

    fn is_active_call_peer(&self, call_id: &CallId, peer_id: &ClientId) -> bool {
        self.active_webrtc_call
            .as_ref()
            .is_some_and(|call| call.call_id == *call_id && call.peer_id == *peer_id)
    }

    /// Accepts a new offer for the already active call by replacing the peer connection,
    /// keeping the call itself alive. The peer sends this when it detected a broken media path
    /// and reconnects via relay.
    async fn reaccept_call_offer(
        &mut self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        offer_sdp: String,
    ) -> Result<String, Error> {
        if !self.is_active_call_peer(&call_id, &peer_id) {
            return Err(WebrtcError::NoCallActive.into());
        }

        log::info!(
            "Received new WebRTC offer for active call {call_id}, replacing peer connection"
        );

        // Shows as "connecting" until the replacement peer emits call-connected
        app.emit("webrtc:call-reconnecting", &call_id).ok();

        let old_call = self
            .active_webrtc_call
            .take()
            .expect("active call checked directly above");
        self.teardown_call_peer(old_call).await;

        let peer_supports_reconnect = has_reconnect_capability(&offer_sdp);

        // The old peer is gone; if the replacement cannot be set up, the call is over and the
        // end sound has to be played here since cleanup_call no longer knows the call
        let replacement = async {
            let (peer, events_rx) =
                Peer::new(self.config.ice.clone(), self.config.client.call.force_relay)
                    .await
                    .context("Failed to create WebRTC peer")?;

            let sdp = peer
                .accept_offer(offer_sdp)
                .await
                .context("Failed to accept WebRTC offer")?;

            Ok::<_, Error>((peer, events_rx, sdp))
        }
        .await;

        let (peer, events_rx, sdp) = match replacement {
            Ok(replacement) => replacement,
            Err(err) => {
                self.play_call_end_sound();
                return Err(err);
            }
        };

        let events_cancel = CancellationToken::new();
        spawn_peer_events_task(
            app,
            call_id,
            peer_id.clone(),
            events_rx,
            events_cancel.clone(),
        );

        self.active_webrtc_call = Some(WebrtcCall {
            call_id,
            peer_id,
            peer,
            events_cancel,
            // The peer already reconnected once; don't trigger another reconnect from this side
            // if media is still broken.
            reconnected: true,
            reconnect_pending: true,
            peer_supports_reconnect,
        });

        Ok(tag_reconnect_capability(&sdp))
    }

    async fn accept_call_answer(
        &mut self,
        peer_id: &ClientId,
        answer_sdp: String,
    ) -> Result<(), Error> {
        if let Some(call) = &mut self.active_webrtc_call {
            if call.peer_id == *peer_id {
                call.peer_supports_reconnect = has_reconnect_capability(&answer_sdp);
                call.peer.accept_answer(answer_sdp).await?;
                return Ok(());
            } else {
                log::warn!(
                    "Tried to accept answer, but peer_id does not match. Peer id: {peer_id}"
                );
            }
        }

        Err(WebrtcError::NoCallActive.into())
    }

    async fn set_remote_ice_candidate(&self, call_id: &CallId, candidate: String) {
        let res = if let Some(call) = &self.active_webrtc_call
            && call.call_id == *call_id
        {
            call.peer.add_remote_ice_candidate(candidate).await
        } else if let Some(call) = self.held_calls.get(call_id) {
            call.peer.add_remote_ice_candidate(candidate).await
        } else {
            Err(anyhow::anyhow!("Unknown call {call_id:?}").into())
        };

        if let Err(err) = res {
            log::warn!("Failed to add remote ICE candidate: {err:?}");
        }
    }

    async fn cleanup_call(&mut self, call_id: &CallId) -> bool {
        log::debug!(
            "Cleaning up call {call_id:?} (active: {:?})",
            self.active_webrtc_call.as_ref()
        );
        let res = if let Some(call) = &mut self.active_webrtc_call
            && call.call_id == *call_id
        {
            {
                let mut audio_manager = self.audio_manager.write();
                // During a pending reconnect, call audio is detached even though the call was
                // live; play the end sound regardless
                if self.config.client.call.enable_call_end_sound
                    && (audio_manager.is_input_device_attached() || call.reconnect_pending)
                {
                    audio_manager.restart(SourceType::CallEnd);
                }
                audio_manager.detach_call_output();
                audio_manager.detach_input_device();
            }

            self.keybind_engine.read().await.set_call_active(false);

            call.events_cancel.cancel();
            let result = call.peer.close().await;
            self.active_webrtc_call = None;
            result
        } else if let Some(mut call) = self.held_calls.remove(call_id) {
            call.events_cancel.cancel();
            call.peer.close().await
        } else {
            Err(anyhow::anyhow!("Unknown call {call_id:?}").into())
        };

        if let Err(err) = &res {
            log::warn!("Failed to cleanup call: {err:?}");
            return false;
        }

        true
    }

    fn emit_call_error(
        &self,
        app: &AppHandle,
        call_id: CallId,
        is_local: bool,
        targets: HashSet<CallTarget>,
        reason: CallErrorReason,
    ) {
        app.emit(
            "webrtc:call-error",
            CallError::new(call_id, is_local, targets, reason),
        )
        .ok();
    }

    fn active_call_id(&self) -> Option<&CallId> {
        self.active_call.as_ref().map(|call| &call.call_id)
    }

    fn set_ice_config(&mut self, config: IceConfig) {
        self.config.ice = config;
    }

    fn is_ice_config_expired(&self) -> bool {
        if self.config.ice.is_default() {
            return false;
        }

        let expires_at = match self.config.ice.expires_at {
            Some(expires_at) => expires_at,
            None => return false,
        };

        let now = UNIX_EPOCH.elapsed().unwrap_or_default().as_secs();
        if now >= expires_at.saturating_sub(ICE_CONFIG_EXPIRY_LEEWAY.as_secs()) {
            log::debug!(
                "ICE config is expired, expiry {} is less than leeway of {:?}",
                expires_at,
                ICE_CONFIG_EXPIRY_LEEWAY
            );
            true
        } else {
            log::debug!(
                "ICE config is still valid, expiry {} is greater than leeway of {:?}",
                expires_at,
                ICE_CONFIG_EXPIRY_LEEWAY
            );
            false
        }
    }
}

impl AppStateInner {
    async fn on_peer_connected(
        &mut self,
        app: &AppHandle,
        call_id: &CallId,
        peer_id: &ClientId,
    ) -> Result<(), Error> {
        if let Some(call) = &mut self.active_webrtc_call
            && call.peer_id == *peer_id
        {
            let (output_tx, output_rx) = mpsc::channel(ENCODED_AUDIO_FRAME_BUFFER_SIZE);
            let (input_tx, input_rx) = mpsc::channel(ENCODED_AUDIO_FRAME_BUFFER_SIZE);

            log::debug!("Starting peer {peer_id} in WebRTC manager");
            if let Err(err) = call.peer.start(input_rx, output_tx) {
                log::warn!("Failed to start peer in WebRTC manager: {err:?}");
                return Err(err.into());
            }
            call.reconnect_pending = false;

            let attach_muted = {
                let keybind_engine = self.keybind_engine.read().await;
                keybind_engine.set_call_active(true);
                keybind_engine.should_attach_input_muted()
            };

            let audio_config = self.config.audio.clone();
            let mut audio_manager = self.audio_manager.write();
            log::debug!("Attaching call to audio manager");
            if let Err(err) = audio_manager.attach_call_output(
                output_rx,
                audio_config.output_device_volume,
                audio_config.output_device_volume_amp,
            ) {
                log::warn!("Failed to attach call to audio manager: {err:?}");
                return Err(err);
            }

            log::debug!("Attaching input device to audio manager");
            if let Err(err) = audio_manager.attach_input_device(
                app.clone(),
                &audio_config,
                input_tx,
                attach_muted,
            ) {
                log::warn!("Failed to attach input device to audio manager: {err:?}");
                return Err(err);
            }

            // An in-call reconnect resumes the existing call, so don't signal a new one
            if self.config.client.call.enable_call_start_sound && !call.reconnected {
                audio_manager.restart(SourceType::CallStart);
            }

            log::info!("Successfully established call to peer");
            app.emit("webrtc:call-connected", call_id).ok();
        } else {
            log::debug!("Peer connected is not the active call, checking held calls");
            if self.held_calls.contains_key(call_id) {
                log::info!("Held peer connection with peer {peer_id} reconnected");
                app.emit("webrtc:call-connected", call_id).ok();
            } else {
                log::debug!("Peer {peer_id} is not held, ignoring");
            }
        }
        Ok(())
    }

    /// Tears down a call's peer connection without ending the call itself: stops the peer
    /// events task, detaches audio (so the replacement peer can re-attach on connect) and
    /// closes the peer connection.
    async fn teardown_call_peer(&mut self, mut call: WebrtcCall) {
        call.events_cancel.cancel();

        {
            let mut audio_manager = self.audio_manager.write();
            audio_manager.detach_call_output();
            audio_manager.detach_input_device();
        }

        if let Err(err) = call.peer.close().await {
            log::warn!("Failed to close peer during reconnect: {err:?}");
        }
    }

    fn play_call_end_sound(&self) {
        if self.config.client.call.enable_call_end_sound {
            self.audio_manager.read().restart(SourceType::CallEnd);
        }
    }

    /// Attempts to re-establish the active call over a relayed (TURN-only) connection after the
    /// media watchdog reported no inbound media. Returns the new offer SDP to signal to the
    /// peer, or `None` if no reconnect was attempted (no matching call, the connection is
    /// already relayed, or the peer's client does not support in-call reconnects).
    async fn try_relay_reconnect(
        &mut self,
        app: &AppHandle,
        call_id: &CallId,
    ) -> Result<Option<String>, Error> {
        let Some(call) = self.active_webrtc_call.as_ref() else {
            log::debug!("No active call for relay reconnect");
            return Ok(None);
        };
        if call.call_id != *call_id {
            log::debug!("Active call does not match relay reconnect request");
            return Ok(None);
        }
        if call.reconnected || self.config.client.call.force_relay {
            log::warn!(
                "No inbound media although the call is already relayed, not reconnecting again"
            );
            app.emit("webrtc:call-degraded", call_id).ok();
            return Ok(None);
        }
        if !call.peer_supports_reconnect {
            log::warn!(
                "No inbound media on call {call_id}, but the peer's client version does not \
                 support in-call reconnects, leaving the call as-is. Enabling force relay (call \
                 settings) may help if this happens regularly"
            );
            app.emit("webrtc:call-degraded", call_id).ok();
            return Ok(None);
        }
        log::warn!("No inbound media on call {call_id}, reconnecting via relay");

        // Shows as "connecting" until the replacement peer emits call-connected (or the call
        // fails and errors out)
        app.emit("webrtc:call-reconnecting", call_id).ok();

        let old_call = self
            .active_webrtc_call
            .take()
            .expect("active call checked directly above");
        let peer_id = old_call.peer_id.clone();
        self.teardown_call_peer(old_call).await;

        // The old peer is gone; if the replacement cannot be set up, the call is over and the
        // end sound has to be played here since cleanup_call no longer knows the call
        let replacement = async {
            let (peer, events_rx) = Peer::new(self.config.ice.clone(), true)
                .await
                .context("Failed to create relayed WebRTC peer")?;

            let sdp = peer
                .create_offer()
                .await
                .context("Failed to create WebRTC offer for relay reconnect")?;

            Ok::<_, Error>((peer, events_rx, sdp))
        }
        .await;

        let (peer, events_rx, sdp) = match replacement {
            Ok(replacement) => replacement,
            Err(err) => {
                self.play_call_end_sound();
                return Err(err);
            }
        };

        let events_cancel = CancellationToken::new();
        spawn_peer_events_task(
            app.clone(),
            *call_id,
            peer_id.clone(),
            events_rx,
            events_cancel.clone(),
        );

        self.active_webrtc_call = Some(WebrtcCall {
            call_id: *call_id,
            peer_id,
            peer,
            events_cancel,
            reconnected: true,
            reconnect_pending: true,
            peer_supports_reconnect: true,
        });

        Ok(Some(tag_reconnect_capability(&sdp)))
    }
}

/// Handles events of a single peer connection for the given call. The task exits when `cancel`
/// is triggered (peer replaced or call cleaned up) or the peer's event channel closes.
fn spawn_peer_events_task(
    app: AppHandle,
    call_id: CallId,
    peer_id: ClientId,
    mut events_rx: broadcast::Receiver<PeerEvent>,
    cancel: CancellationToken,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            let event = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    log::trace!("Peer events task cancelled");
                    break;
                }
                event = events_rx.recv() => event,
            };

            match event {
                Ok(peer_event) => match peer_event {
                    PeerEvent::ConnectionState(state) => match state {
                        PeerConnectionState::Connected => {
                            log::info!("Connected to peer");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;
                            if let Err(err) =
                                state.on_peer_connected(&app, &call_id, &peer_id).await
                            {
                                let reason: CallErrorReason = err.into();
                                state.cleanup_call(&call_id).await;
                                if let Err(err) = state
                                    .send_signaling_message(shared::CallError {
                                        call_id,
                                        reason: reason.clone(),
                                        message: None,
                                    })
                                    .await
                                {
                                    log::warn!("Failed to send call message: {err:?}");
                                }
                                state.emit_call_error(&app, call_id, true, reason);
                            }
                        }
                        PeerConnectionState::Disconnected => {
                            log::info!("Disconnected from peer");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;

                            if let Some(call) = &mut state.active_webrtc_call
                                && call.peer_id == peer_id
                            {
                                call.peer.pause();
                                let mut audio_manager = state.audio_manager.write();

                                if state.config.client.call.enable_call_end_sound
                                    && audio_manager.is_input_device_attached()
                                {
                                    audio_manager.restart(SourceType::CallEnd);
                                }

                                audio_manager.detach_call_output();
                                audio_manager.detach_input_device();
                            }

                            app.emit("webrtc:call-disconnected", &call_id).ok();
                        }
                        PeerConnectionState::Failed => {
                            log::info!("Connection to peer failed");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;
                            state.cleanup_call(&call_id).await;

                            state.emit_call_error(
                                &app,
                                call_id,
                                true,
                                CallErrorReason::WebrtcFailure,
                            );
                        }
                        PeerConnectionState::Closed => {
                            // Graceful close
                            log::info!("Peer closed connection");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;

                            state.cleanup_call(&call_id).await;
                            app.emit("signaling:call-end", &call_id).ok();
                        }
                        state => {
                            log::trace!("Received connection state: {state:?}");
                        }
                    },
                    PeerEvent::IceCandidate(candidate) => {
                        let app_state = app.state::<AppState>();
                        let mut state = app_state.lock().await;

                        let Some(own_client_id) = state.client_id.as_ref().cloned() else {
                            log::warn!("Cannot send ICE candidate without own client ID");
                            return;
                        };

                        if let Err(err) = state
                            .send_signaling_message(shared::WebrtcIceCandidate {
                                call_id,
                                from_client_id: own_client_id,
                                to_client_id: peer_id.clone(),
                                candidate,
                            })
                            .await
                        {
                            log::warn!("Failed to send ICE candidate: {err:?}");
                        }
                    }
                    PeerEvent::NoInboundMedia => {
                        let app_state = app.state::<AppState>();
                        let mut state = app_state.lock().await;

                        match state.try_relay_reconnect(&app, &call_id).await {
                            Ok(Some(sdp)) => {
                                let Some(own_client_id) = state.client_id.as_ref().cloned() else {
                                    log::warn!("Cannot send WebRTC offer without own client ID");
                                    return;
                                };

                                if let Err(err) = state
                                    .send_signaling_message(shared::WebrtcOffer {
                                        call_id,
                                        from_client_id: own_client_id,
                                        to_client_id: peer_id.clone(),
                                        sdp,
                                    })
                                    .await
                                {
                                    log::warn!("Failed to send relay reconnect offer: {err:?}");
                                }
                            }
                            Ok(None) => {}
                            Err(err) => {
                                log::warn!("Failed to reconnect call via relay: {err:?}");

                                let reason = CallErrorReason::WebrtcFailure;
                                state.cleanup_call(&call_id).await;
                                if let Err(err) = state
                                    .send_signaling_message(shared::CallError {
                                        call_id,
                                        reason: reason.clone(),
                                        message: None,
                                    })
                                    .await
                                {
                                    log::warn!("Failed to send call message: {err:?}");
                                }
                                state.emit_call_error(&app, call_id, true, reason);
                            }
                        }
                    }
                    PeerEvent::Error(err) => {
                        log::warn!("Received error peer event: {err}");
                    }
                },
                Err(err) => {
                    log::warn!("Failed to receive peer event: {err:?}");
                    if err == RecvError::Closed {
                        break;
                    }
                }
            }
        }

        log::trace!("WebRTC events task finished");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_detects_reconnect_capability() {
        let sdp = r#"{"type":"offer","sdp":"v=0\r\n"}"#;
        assert!(!has_reconnect_capability(sdp));

        let tagged = tag_reconnect_capability(sdp);
        assert!(has_reconnect_capability(&tagged));

        // The tagged SDP must still deserialize for older clients, which parse it into
        // RTCSessionDescription via serde and thus ignore unknown fields.
        #[derive(serde::Deserialize)]
        struct SessionDescription {
            #[serde(rename = "type")]
            sdp_type: String,
            sdp: String,
        }

        let session =
            serde_json::from_str::<SessionDescription>(&tagged).expect("tagged SDP deserializes");
        assert_eq!(session.sdp_type, "offer");
        assert_eq!(session.sdp, "v=0\r\n");
    }

    #[test]
    fn leaves_invalid_sdp_untouched() {
        assert_eq!(tag_reconnect_capability("not-json"), "not-json");
        assert!(!has_reconnect_capability("not-json"));
    }
}
