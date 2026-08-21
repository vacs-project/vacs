use crate::app::state::calls::Call;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::{AppState, AppStateInner, sealed};
use crate::audio::manager::AudioManagerHandle;
use crate::audio::source_type::SourceType;
use crate::error::{CallError, CallErrorOrigin, Error};
use anyhow::Context;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{Debug, Formatter};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use vacs_audio::sources::AudioSourceId;
use vacs_signaling::protocol::http::webrtc::IceConfig;
use vacs_signaling::protocol::vatsim::ClientId;
use vacs_signaling::protocol::ws::shared;
use vacs_signaling::protocol::ws::shared::{CallErrorReason, CallId};
use vacs_webrtc::error::WebrtcError;
use vacs_webrtc::{Peer, PeerConnectionState, PeerEvent};

const ENCODED_AUDIO_FRAME_BUFFER_SIZE: usize = 512;
const ICE_CONFIG_EXPIRY_LEEWAY: Duration = Duration::from_mins(15);
const START_SOUND_THRESHOLD: Duration = Duration::from_millis(200);

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebrtcUpdateEvent {
    call_id: CallId,
    peer_id: ClientId,
}

fn emit_webrtc_update(app: &AppHandle, event: &str, call_id: CallId, peer_id: &ClientId) {
    app.emit(
        event,
        WebrtcUpdateEvent {
            call_id,
            peer_id: peer_id.clone(),
        },
    )
    .ok();
}

pub struct WebrtcPeer {
    peer_id: ClientId,
    peer: Peer,
    audio_source_id: Option<AudioSourceId>,
    /// Cancels the peer events task when the peer is replaced or the call is cleaned up, so
    /// events of a stale peer (e.g. its Closed state) cannot tear down the current call.
    events_cancel: CancellationToken,
    /// Whether this link ever carried media. Replacement peers inherit it, the call was live before
    /// the swap. Decides whether ending the call plays the call end sound.
    connected: bool,
    /// Whether this peer connection is a replacement established by an in-call reconnect.
    /// Prevents reconnect loops and suppresses the call start sound when it connects.
    reconnected: bool,
    /// Whether the peer's offer/answer advertised support for in-call reconnects
    /// ([`SDP_RECONNECT_CAPABILITY_KEY`]). Reconnects towards older clients would fail the call.
    peer_supports_reconnect: bool,
}

impl Debug for WebrtcPeer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebrtcPeer")
            .field("peer_id", &self.peer_id)
            .field("audio_source_id", &self.audio_source_id)
            .field("connected", &self.connected)
            .field("reconnected", &self.reconnected)
            .field("peer_supports_reconnect", &self.peer_supports_reconnect)
            .finish()
    }
}

impl WebrtcPeer {
    pub fn new(
        peer_id: ClientId,
        peer: Peer,
        call_cancel: &CancellationToken,
        peer_supports_reconnect: bool,
    ) -> Self {
        Self {
            peer_id,
            peer,
            audio_source_id: None,
            events_cancel: call_cancel.child_token(),
            connected: false,
            reconnected: false,
            peer_supports_reconnect,
        }
    }
    pub fn with_reconnect(mut self) -> Self {
        self.connected = true;
        self.reconnected = true;
        self
    }

    pub async fn shutdown(mut self) {
        self.events_cancel.cancel();
        if let Err(err) = self.peer.close().await {
            log::warn!("Failed to close peer {}: {err:?}", self.peer_id);
        }
    }

    pub async fn accept_answer(&mut self, answer_sdp: String) -> Result<(), WebrtcError> {
        self.peer_supports_reconnect = has_reconnect_capability(&answer_sdp);
        self.peer.accept_answer(answer_sdp).await
    }
}

#[derive(Debug)]
pub struct WebrtcCall {
    call_id: CallId,
    cancel: CancellationToken,
    peers: HashMap<ClientId, WebrtcPeer>,
    last_call_start_sound: Option<Instant>,
}

impl WebrtcCall {
    pub fn new(call_id: CallId, shutdown_token: &CancellationToken) -> Self {
        Self {
            call_id,
            cancel: shutdown_token.child_token(),
            peers: HashMap::new(),
            last_call_start_sound: None,
        }
    }

    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    pub fn has_peer(&self, peer_id: &ClientId) -> bool {
        self.peers.contains_key(peer_id)
    }

    pub fn peer(&self, peer_id: &ClientId) -> Option<&WebrtcPeer> {
        self.peers.get(peer_id)
    }
    pub fn peer_mut(&mut self, peer_id: &ClientId) -> Option<&mut WebrtcPeer> {
        self.peers.get_mut(peer_id)
    }

    #[allow(clippy::result_large_err)]
    pub fn add_peer(&mut self, peer: WebrtcPeer) -> Result<(), WebrtcPeer> {
        match self.peers.entry(peer.peer_id.clone()) {
            Entry::Occupied(_) => Err(peer),
            Entry::Vacant(entry) => {
                entry.insert(peer);
                Ok(())
            }
        }
    }
    pub fn remove_peer(&mut self, peer_id: &ClientId) -> Option<WebrtcPeer> {
        self.peers.remove(peer_id)
    }

    pub fn was_connected(&self) -> bool {
        self.peers.values().any(|peer| peer.connected)
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn has_other_connected_peer(&self, peer_id: &ClientId) -> bool {
        self.peers
            .values()
            .any(|peer| &peer.peer_id != peer_id && peer.connected)
    }

    pub fn into_peers(self) -> impl Iterator<Item = WebrtcPeer> {
        self.peers.into_values()
    }

    pub async fn shutdown(self) {
        self.cancel.cancel();

        let mut closing = JoinSet::new();
        for peer in self.into_peers() {
            closing.spawn(peer.shutdown());
        }
        closing.join_all().await;
    }
}

pub trait AppStateWebrtcExt: sealed::Sealed {
    fn webrtc_call(&self, call_id: CallId) -> Option<&WebrtcCall>;
    fn webrtc_call_mut(&mut self, call_id: CallId) -> Option<&mut WebrtcCall>;
    fn webrtc_peer(&self, call_id: CallId, peer_id: &ClientId) -> Option<&WebrtcPeer>;
    fn webrtc_peer_mut(&mut self, call_id: CallId, peer_id: &ClientId) -> Option<&mut WebrtcPeer>;
    async fn negotiate_peer(
        &mut self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        own_client_id: &ClientId,
        offer_sdp: Option<String>,
    ) -> Result<String, Error>;
    async fn accept_call_answer(
        &mut self,
        call_id: CallId,
        peer_id: &ClientId,
        answer_sdp: String,
    ) -> Result<(), Error>;
    async fn set_remote_ice_candidate(
        &self,
        call_id: CallId,
        peer_id: &ClientId,
        candidate: String,
    );
    async fn cleanup_call_peer(&mut self, call_id: CallId, peer_id: &ClientId) -> bool;
    async fn cleanup_current_call(&mut self, call_id: CallId) -> bool;
    async fn end_call_if_no_peers(&mut self, call_id: CallId) -> bool;
    fn emit_call_error(
        &self,
        app: &AppHandle,
        call_id: CallId,
        is_local: bool,
        origin: CallErrorOrigin,
        reason: CallErrorReason,
    );
    fn set_ice_config(&mut self, config: IceConfig);
    fn is_ice_config_expired(&self) -> bool;
}

impl AppStateWebrtcExt for AppStateInner {
    fn webrtc_call(&self, call_id: CallId) -> Option<&WebrtcCall> {
        self.current_call(call_id).map(Call::webrtc)
    }
    fn webrtc_call_mut(&mut self, call_id: CallId) -> Option<&mut WebrtcCall> {
        self.current_call_mut(call_id).map(Call::webrtc_mut)
    }

    fn webrtc_peer(&self, call_id: CallId, peer_id: &ClientId) -> Option<&WebrtcPeer> {
        self.webrtc_call(call_id)?.peer(peer_id)
    }
    fn webrtc_peer_mut(&mut self, call_id: CallId, peer_id: &ClientId) -> Option<&mut WebrtcPeer> {
        self.webrtc_call_mut(call_id)?.peer_mut(peer_id)
    }

    async fn negotiate_peer(
        &mut self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        own_client_id: &ClientId,
        offer_sdp: Option<String>,
    ) -> Result<String, Error> {
        let call = match self.current_call.as_mut() {
            Some(call) if call.call_id() == call_id => call.webrtc_mut(),
            Some(_) => return Err(WebrtcError::CallActive.into()),
            None => return Err(WebrtcError::NoCallActive.into()),
        };

        let replacing = call.has_peer(&peer_id);
        let call_cancel = call.cancel.clone();

        let force_relay = self.config.client.call.force_relay || (replacing && offer_sdp.is_none());

        if replacing {
            log::info!("Replacing peer connection with peer {peer_id} in call {call_id}");
            emit_webrtc_update(&app, "webrtc:call-reconnecting", call_id, &peer_id);
        } else {
            log::debug!("Negotiating peer connection with peer {peer_id} in call {call_id}");
        }

        let (peer, sdp) = match self
            .create_peer(
                app,
                call_id,
                peer_id.clone(),
                own_client_id,
                &call_cancel,
                offer_sdp,
                replacing,
                force_relay,
            )
            .await
        {
            Ok(res) => res,
            Err(err) => {
                return Err(err);
            }
        };

        let Some(call) = self.webrtc_call_mut(call_id) else {
            // unreachable, defensive guard
            log::error!("Call {call_id} ended while negotiating with peer {peer_id}");
            peer.shutdown().await;
            return Err(WebrtcError::NoCallActive.into());
        };

        let old_peer = call.remove_peer(&peer_id);
        let added = call.add_peer(peer);

        if let Some(old_peer) = old_peer {
            let audio_source_id = old_peer.audio_source_id;
            old_peer.shutdown().await;

            if let Some(audio_source_id) = audio_source_id {
                let mut audio_manager = self.audio_manager.write();
                audio_manager.detach_call_output(audio_source_id);
                audio_manager.detach_input_device();
            }
        }

        if let Err(peer) = added {
            // unreachable, defensive guard
            log::error!("Peer {peer_id} already exists for call {call_id}");
            peer.shutdown().await;

            return Err(WebrtcError::CallActive.into());
        }

        Ok(sdp)
    }

    async fn accept_call_answer(
        &mut self,
        call_id: CallId,
        peer_id: &ClientId,
        answer_sdp: String,
    ) -> Result<(), Error> {
        let Some(peer) = self.webrtc_peer_mut(call_id, peer_id) else {
            log::warn!(
                "Received WebRTC answer for call {call_id} from peer {peer_id}, but no active WebRTC call exists, ignoring"
            );
            return Err(WebrtcError::NoCallActive.into());
        };

        peer.accept_answer(answer_sdp).await.map_err(Error::from)
    }

    async fn set_remote_ice_candidate(
        &self,
        call_id: CallId,
        peer_id: &ClientId,
        candidate: String,
    ) {
        let Some(peer) = self.webrtc_peer(call_id, peer_id) else {
            log::warn!(
                "Received WebRTC ICE candidate for call {call_id} from peer {peer_id}, but no active WebRTC call exists, ignoring"
            );
            return;
        };

        if let Err(err) = peer.peer.add_remote_ice_candidate(candidate).await {
            log::warn!(
                "Failed to add remote WebRTC ICE candidate for call {call_id} with peer {peer_id}: {err:?}"
            );
        }
    }

    async fn cleanup_call_peer(&mut self, call_id: CallId, peer_id: &ClientId) -> bool {
        let Some((peer, last)) = self.take_webrtc_peer(call_id, peer_id) else {
            log::debug!("No peer {peer_id} in call {call_id} to clean up");
            return false;
        };

        log::debug!("Cleaning up peer {peer_id} in call {call_id}");

        if last {
            self.keybind_engine.read().await.set_call_active(false);
        }

        let audio_source_id = peer.audio_source_id;
        let connected = peer.connected;
        peer.shutdown().await;

        let left_sound = connected
            .then(|| self.peer_left_sound(call_id, peer_id))
            .flatten();

        {
            let mut audio_manager = self.audio_manager.write();

            if let Some(audio_source_id) = audio_source_id {
                audio_manager.detach_call_output(audio_source_id);
            }

            if let Some(left_sound) = left_sound {
                audio_manager.restart(left_sound);
            }

            audio_manager.detach_input_device();
        }

        true
    }

    async fn cleanup_current_call(&mut self, call_id: CallId) -> bool {
        let Some(call) = self.current_call.take_if(|call| call.call_id() == call_id) else {
            log::debug!("No current call {call_id} to cleanup");
            return false;
        };

        log::debug!("Cleaning up call {call_id}");

        let webrtc_call = call.into_webrtc();

        let audio_source_ids: Vec<AudioSourceId> = webrtc_call
            .peers
            .values()
            .filter_map(|peer| peer.audio_source_id)
            .collect();

        self.keybind_engine.read().await.set_call_active(false);

        let was_connected = webrtc_call.was_connected();
        webrtc_call.shutdown().await;

        {
            let mut audio_manager = self.audio_manager.write();

            audio_manager.stop(SourceType::Ringback);

            if self.config.client.call.enable_call_end_sound && was_connected {
                audio_manager.restart(SourceType::CallEnd);
            }

            for audio_source_id in audio_source_ids {
                audio_manager.detach_call_output(audio_source_id);
            }
            audio_manager.detach_input_device();
        }

        true
    }

    async fn end_call_if_no_peers(&mut self, call_id: CallId) -> bool {
        if !self.current_call(call_id).is_some_and(Call::is_empty)
            || !self.webrtc_call(call_id).is_some_and(WebrtcCall::is_empty)
        {
            return false;
        }

        log::debug!("No peer connections remain in call {call_id}, ending call");

        self.cancel_all_unanswered_call_timers(call_id);
        self.cleanup_current_call(call_id).await;

        true
    }

    fn emit_call_error(
        &self,
        app: &AppHandle,
        call_id: CallId,
        is_local: bool,
        origin: CallErrorOrigin,
        reason: CallErrorReason,
    ) {
        app.emit(
            "webrtc:call-error",
            CallError::new(call_id, is_local, origin, reason),
        )
        .ok();
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
    async fn fail_peer(
        &mut self,
        app: &AppHandle,
        call_id: CallId,
        peer_id: &ClientId,
        reason: CallErrorReason,
    ) {
        self.cleanup_call_peer(call_id, peer_id).await;

        self.try_send_call_error(call_id, reason.clone(), None)
            .await;
        self.emit_call_error(app, call_id, true, peer_id.clone().into(), reason); // TODO: This needs some attention

        if self.end_call_if_no_peers(call_id).await {
            app.emit("signaling:force-call-end", &call_id).ok();
        }
    }

    fn peer_joined_sound(&self, call_id: CallId, peer_id: &ClientId) -> Option<SourceType> {
        let call_config = &self.config.client.call;

        if let Some(call) = self.webrtc_call(call_id)
            && call.has_other_connected_peer(peer_id)
        {
            if let Some(last_start_sound) = call.last_call_start_sound
                && last_start_sound + START_SOUND_THRESHOLD > Instant::now()
            {
                return None;
            }

            call_config
                .enable_participant_joined_sound
                .then_some(SourceType::ParticipantJoined)
        } else {
            call_config
                .enable_call_start_sound
                .then_some(SourceType::CallStart)
        }
    }

    fn peer_left_sound(&self, call_id: CallId, peer_id: &ClientId) -> Option<SourceType> {
        let call_config = &self.config.client.call;

        if self
            .webrtc_call(call_id)
            .is_some_and(|call| call.has_other_connected_peer(peer_id))
        {
            call_config
                .enable_participant_left_sound
                .then_some(SourceType::ParticipantLeft)
        } else {
            call_config
                .enable_call_end_sound
                .then_some(SourceType::CallEnd)
        }
    }

    fn take_webrtc_peer(
        &mut self,
        call_id: CallId,
        peer_id: &ClientId,
    ) -> Option<(WebrtcPeer, bool)> {
        let call = self.webrtc_call_mut(call_id)?;
        let peer = call.remove_peer(peer_id)?;
        let last = call.is_empty();

        Some((peer, last))
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_peer(
        &self,
        app: AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        own_client_id: &ClientId,
        call_cancel: &CancellationToken,
        offer_sdp: Option<String>,
        reconnect: bool,
        force_relay: bool,
    ) -> Result<(WebrtcPeer, String), Error> {
        let (peer, events_rx) = Peer::new(self.config.ice.clone(), force_relay)
            .await
            .context("Failed to create WebRTC peer")?;

        // As the offerer, the peer's capabilities are only known once its answer arrives;
        // see accept_call_answer.
        let peer_supports_reconnect = offer_sdp.as_deref().is_some_and(has_reconnect_capability);

        let sdp = match offer_sdp {
            Some(sdp) => peer
                .accept_offer(sdp)
                .await
                .context("Failed to accept WebRTC offer")?,
            None => peer
                .create_offer()
                .await
                .context("Failed to create WebRTC offer")?,
        };

        let webrtc_peer = if reconnect {
            WebrtcPeer::new(peer_id, peer, call_cancel, peer_supports_reconnect).with_reconnect()
        } else {
            WebrtcPeer::new(peer_id, peer, call_cancel, peer_supports_reconnect)
        };

        spawn_peer_events_task(
            app,
            call_id,
            webrtc_peer.peer_id.clone(),
            own_client_id.clone(),
            events_rx,
            webrtc_peer.events_cancel.clone(),
        );

        Ok((webrtc_peer, tag_reconnect_capability(&sdp)))
    }

    async fn on_peer_connected(
        &mut self,
        app: &AppHandle,
        call_id: CallId,
        peer_id: &ClientId,
    ) -> Result<(), Error> {
        let Some(peer) = self.webrtc_peer(call_id, peer_id) else {
            log::warn!(
                "Peer {peer_id} connected for call {call_id}, but no WebRTC peer exists, ignoring"
            );
            return Err(WebrtcError::NoCallActive.into());
        };
        let reconnected = peer.reconnected;

        // An in-call reconnect resumes the existing call, so it announces nothing at all
        let joined_sound = (!reconnected)
            .then(|| self.peer_joined_sound(call_id, peer_id))
            .flatten();

        log::debug!("Starting peer {peer_id} for call {call_id} in WebRTC manager");

        let (output_tx, output_rx) = mpsc::channel(ENCODED_AUDIO_FRAME_BUFFER_SIZE);

        let attach_muted = {
            let keybind_engine = self.keybind_engine.read().await;
            keybind_engine.set_call_active(true);
            keybind_engine.should_attach_input_muted()
        };

        let audio_config = self.config.audio.clone();
        let (audio_source_id, input_rx) = {
            let mut audio_manager = self.audio_manager.write();

            log::debug!("Attaching call to audio manager");
            let audio_source_id = match audio_manager.attach_call_output(
                output_rx,
                audio_config.output_device_volume,
                audio_config.output_device_volume_amp,
            ) {
                Ok(audio_source_id) => audio_source_id,
                Err(err) => {
                    log::warn!("Failed to attach call to audio manager: {err:?}");
                    return Err(err);
                }
            };

            let input_rx =
                match audio_manager.attach_input_device(app.clone(), &audio_config, attach_muted) {
                    Ok(input_rx) => input_rx,
                    Err(err) => {
                        log::warn!("Failed to attach input device to audio manager: {err:?}");
                        audio_manager.detach_call_output(audio_source_id);
                        return Err(err);
                    }
                };

            if let Some(joined_sound) = joined_sound {
                audio_manager.restart(joined_sound);
            }

            (audio_source_id, input_rx)
        };

        if joined_sound == Some(SourceType::CallStart)
            && let Some(webrtc_call) = self.webrtc_call_mut(call_id)
        {
            webrtc_call.last_call_start_sound = Some(Instant::now());
        }

        let Some(peer) = self.webrtc_peer_mut(call_id, peer_id) else {
            {
                let mut audio_manager = self.audio_manager.write();
                audio_manager.detach_call_output(audio_source_id);
                drop(input_rx);
                audio_manager.detach_input_device();
            }
            return Err(WebrtcError::NoCallActive.into());
        };

        if let Err(err) = peer.peer.start(input_rx, output_tx) {
            log::warn!(
                "Failed to start peer {peer_id} for call {call_id} in WebRTC manager: {err:?}"
            );
            {
                let mut audio_manager = self.audio_manager.write();
                audio_manager.detach_call_output(audio_source_id);
                audio_manager.detach_input_device();
            }
            return Err(err.into());
        }

        if let Some(peer) = self.webrtc_peer_mut(call_id, peer_id) {
            peer.connected = true;
            peer.audio_source_id = Some(audio_source_id);
        }

        log::info!("Successfully established connection to peer {peer_id} in call {call_id}");
        emit_webrtc_update(app, "webrtc:call-connected", call_id, peer_id);

        Ok(())
    }

    /// Attempts to re-establish the active call over a relayed (TURN-only) connection after the
    /// media watchdog reported no inbound media. Returns the new offer SDP to signal to the
    /// peer, or `None` if no reconnect was attempted (no matching call, the connection is
    /// already relayed, or the peer's client does not support in-call reconnects).
    async fn try_relay_reconnect(
        &mut self,
        app: &AppHandle,
        call_id: CallId,
        peer_id: ClientId,
        own_client_id: &ClientId,
    ) -> Result<Option<String>, Error> {
        let Some(peer) = self.webrtc_peer(call_id, &peer_id) else {
            log::debug!("No peer {peer_id} in call {call_id} for relay reconnect");
            return Ok(None);
        };

        if peer.reconnected || self.config.client.call.force_relay {
            log::warn!(
                "No inbound media although the connection to peer {peer_id} in call \
            {call_id} is already relayed, not reconnecting again"
            );
            emit_webrtc_update(app, "webrtc:call-degraded", call_id, &peer_id);
            return Ok(None);
        }

        if !peer.peer_supports_reconnect {
            log::warn!(
                "No inbound media on call {call_id} with peer {peer_id}, but the peer's client version does not \
                 support in-call reconnects, leaving the call as-is. Enabling force relay (call \
                 settings) may help if this happens regularly"
            );
            emit_webrtc_update(app, "webrtc:call-degraded", call_id, &peer_id);
            return Ok(None);
        }
        log::warn!(
            "No inbound media on call {call_id} with peer {peer_id}, reconnecting via relay"
        );

        self.negotiate_peer(app.clone(), call_id, peer_id, own_client_id, None)
            .await
            .map(Some)
    }
}

/// Handles events of a single peer connection for the given call. The task exits when `cancel`
/// is triggered (peer replaced or call cleaned up) or the peer's event channel closes.
fn spawn_peer_events_task(
    app: AppHandle,
    call_id: CallId,
    peer_id: ClientId,
    own_client_id: ClientId,
    mut events_rx: broadcast::Receiver<PeerEvent>,
    cancel: CancellationToken,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            let event = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    log::trace!("Peer events task for peer {peer_id} in call {call_id} cancelled");
                    break;
                }
                event = events_rx.recv() => event,
            };

            match event {
                Ok(peer_event) => match peer_event {
                    PeerEvent::ConnectionState(state) => match state {
                        PeerConnectionState::Connected => {
                            log::info!("Connected to peer {peer_id} in call {call_id}");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;
                            if let Err(err) = state.on_peer_connected(&app, call_id, &peer_id).await
                            {
                                let reason = err.into_call_error_reason(own_client_id.clone());
                                state.fail_peer(&app, call_id, &peer_id, reason).await;
                            }
                        }
                        PeerConnectionState::Disconnected => {
                            log::info!("Disconnected from peer {peer_id} in call {call_id}");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;

                            let mut sender = None;
                            let mut was_connected = false;
                            if let Some(peer) = state.webrtc_peer_mut(call_id, &peer_id) {
                                sender = peer.peer.pause();
                                let audio_source_id = peer.audio_source_id.take();

                                was_connected = peer.connected;
                                peer.connected = false;

                                if let Some(audio_source_id) = audio_source_id {
                                    state
                                        .audio_manager
                                        .write()
                                        .detach_call_output(audio_source_id);
                                }
                            }

                            if was_connected
                                && let Some(left_sound) = state.peer_left_sound(call_id, &peer_id)
                            {
                                state.audio_manager.read().restart(left_sound);
                            }
                            drop(state);

                            emit_webrtc_update(&app, "webrtc:call-disconnected", call_id, &peer_id);

                            // The sender task holds an input subscription, so it must be
                            // joined before the detach below can take effect
                            if let Some(sender) = sender
                                && let Err(err) = sender.stop().await
                            {
                                log::debug!("Received error while stopping sender: {err:?}");
                            }
                            app.state::<AudioManagerHandle>()
                                .write()
                                .detach_input_device();
                        }
                        PeerConnectionState::Failed => {
                            log::info!("Connection to peer {peer_id} in call {call_id} failed");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;

                            let reason = CallErrorReason::WebrtcFailure(own_client_id.clone());
                            state.fail_peer(&app, call_id, &peer_id, reason).await;
                        }
                        PeerConnectionState::Closed => {
                            // Graceful close
                            log::info!("Peer {peer_id} in call {call_id} closed connection");

                            let app_state = app.state::<AppState>();
                            let mut state = app_state.lock().await;

                            state.cleanup_call_peer(call_id, &peer_id).await;

                            if state.end_call_if_no_peers(call_id).await {
                                app.emit("signaling:call-end", &call_id).ok();
                            }
                        }
                        state => {
                            log::trace!(
                                "Received connection state for peer {peer_id} in call {call_id}: {state:?}"
                            );
                        }
                    },
                    PeerEvent::IceCandidate(candidate) => {
                        let app_state = app.state::<AppState>();
                        let mut state = app_state.lock().await;

                        if let Err(err) = state
                            .send_signaling_message(shared::WebrtcIceCandidate {
                                call_id,
                                from_client_id: own_client_id.clone(),
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

                        match state
                            .try_relay_reconnect(&app, call_id, peer_id.clone(), &own_client_id)
                            .await
                        {
                            Ok(Some(sdp)) => {
                                if let Err(err) = state
                                    .send_signaling_message(shared::WebrtcOffer {
                                        call_id,
                                        from_client_id: own_client_id.clone(),
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
                                log::warn!(
                                    "Failed to reconnect to peer {peer_id} in call {call_id} via relay: {err:?}"
                                );

                                let reason = CallErrorReason::WebrtcFailure(own_client_id.clone());
                                state.fail_peer(&app, call_id, &peer_id, reason).await;
                            }
                        }
                    }
                    PeerEvent::Error(err) => {
                        log::warn!(
                            "Received error peer event for peer {peer_id} in call {call_id}: {err}"
                        );
                    }
                },
                Err(err) => {
                    log::warn!(
                        "Failed to receive peer event for peer {peer_id} in call {call_id}: {err:?}"
                    );
                    if err == RecvError::Closed {
                        break;
                    }
                }
            }
        }

        log::trace!("WebRTC events task for peer {peer_id} in call {call_id} finished");
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
