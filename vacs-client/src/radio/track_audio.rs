use crate::app::state::AppState;
use crate::audio::manager::AudioManagerHandle;
use crate::playback::commands::stop_playing_source;
use crate::playback::recorder::PlaybackRecorderHandle;
use crate::radio::{
    Frequency, Radio, RadioError, RadioHandle, RadioState, RadioStation, StationStateUpdate,
    TransmissionState,
};
use parking_lot::RwLock;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use trackaudio::messages::commands::SetStationState;
use trackaudio::messages::events::StationState;
use trackaudio::{
    ClientEvent, ConnectionState, TrackAudioClient, TrackAudioConfig, TrackAudioError,
};

/// Capacity of the [`TrackAudioRadio`] event fan-out broadcast channel.
const EVENT_FANOUT_CAPACITY: usize = 256;

/// Consecutive failed transmit attempts before the radio is reported as [`RadioState::Error`].
///
/// A single failure is usually just TrackAudio being briefly busy and is not worth flapping the
/// radio indicator over. A streak means PTT is silently doing nothing, which the user has no other
/// way of noticing.
const TRANSMIT_FAILURE_THRESHOLD: usize = 3;

// Deliberately not `Clone`: `Drop` cancels the shared cancellation token (killing the events
// task), so a dropped by-value clone would tear down the original radio's machinery. The radio
// is only ever shared via `Arc` (see `RadioConfig::radio`).
pub struct TrackAudioRadio {
    #[allow(dead_code)]
    app: AppHandle,
    client: TrackAudioClient,
    state: Arc<TrackAudioState>,
    #[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
    events_tx: broadcast::Sender<trackaudio::Event>,
    cancellation_token: CancellationToken,
}

impl TrackAudioRadio {
    const TRANSMIT_TIMEOUT: Duration = Duration::from_millis(250);
    const VOICE_CONNECTED_STATE_TIMEOUT: Duration = Duration::from_millis(250);
    const STATION_STATES_TIMEOUT: Duration = Duration::from_millis(250);
    const STATION_STATE_TIMEOUT: Duration = Duration::from_millis(250);
    const ADD_STATION_TIMEOUT: Duration = Duration::from_millis(500);

    pub async fn new(
        app: AppHandle,
        endpoint: Option<impl AsRef<str>>,
    ) -> Result<Self, RadioError> {
        app.emit("radio:state", RadioState::Disconnected).ok();

        let config = match endpoint {
            Some(endpoint) => TrackAudioConfig::new(endpoint),
            None => Ok(TrackAudioConfig::default()),
        }
        .map_err(|err| {
            app.emit("radio:state", RadioState::Error).ok();
            RadioError::Integration(format!("Failed to build TrackAudioConfig: {err}"))
        })?
        .with_backoff_config(Duration::from_secs(1), Duration::from_secs(30), 2.0);

        let client = TrackAudioClient::connect(config).await.map_err(|err| {
            app.emit("radio:state", RadioState::Error).ok();
            RadioError::Integration(format!("Failed to connect to TrackAudio: {err}"))
        })?;

        let cancellation_token = CancellationToken::new();

        let state = Arc::new(TrackAudioState::default());
        let (events_tx, _) = broadcast::channel(EVENT_FANOUT_CAPACITY);

        {
            let app = app.clone();
            let client = client.clone();
            let token = cancellation_token.clone();
            let state = state.clone();
            let events_tx = events_tx.clone();

            tauri::async_runtime::spawn(async move {
                Self::events_task(app, client, token, state, events_tx).await;
            });
        }

        let radio = Self {
            app,
            client,
            state,
            events_tx,
            cancellation_token,
        };

        Ok(radio)
    }

    /// Independent, cloneable handle to this radio's event stream. Safe for long-lived
    /// consumers (e.g. the playback recorder) to hold onto without keeping the radio itself
    /// alive - subscribing via the returned `Sender` doesn't require the radio to still exist.
    #[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
    pub fn events(&self) -> broadcast::Sender<trackaudio::Event> {
        self.events_tx.clone()
    }

    /// Independent handle to this radio's cached connection/station state. Safe for
    /// long-lived consumers to hold onto without keeping the radio itself alive.
    #[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
    pub fn state_handle(&self) -> Arc<TrackAudioState> {
        self.state.clone()
    }

    async fn events_task(
        app: AppHandle,
        client: TrackAudioClient,
        cancellation_token: CancellationToken,
        state: Arc<TrackAudioState>,
        events_tx: broadcast::Sender<trackaudio::Event>,
    ) {
        log::debug!("Starting TrackAudio events task");

        let mut events = client.subscribe();
        loop {
            tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => {
                    log::info!("TrackAudio events task cancelled");
                    break;
                }
                result = events.recv() => {
                    match result {
                        Ok(event) => {
                            if events_tx.receiver_count() > 0 {
                                let _ = events_tx.send(event.clone());
                            }
                            Self::handle_event(event, &state, &app, &client).await;
                        }
                        // Recoverable: the receiver stays valid and only missed events. Breaking
                        // here would permanently stop every radio state update - TX/RX indication,
                        // station sync and connection handling - until the radio is rebuilt.
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            log::warn!("Lagged by {skipped} TrackAudio events");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            log::error!("TrackAudio event channel closed");
                            state.clear();
                            app.emit("radio:state", RadioState::Error).ok();
                            break;
                        }
                    }
                }
            }
        }

        log::debug!("TrackAudio events task ended");
    }

    async fn handle_event(
        event: trackaudio::Event,
        state: &TrackAudioState,
        app: &AppHandle,
        client: &TrackAudioClient,
    ) {
        use trackaudio::Event;

        match event {
            Event::TxBegin(_) => {
                log::trace!("TrackAudio TX begin");
                state.set_transmitting(app, true);
            }
            Event::TxEnd(_) => {
                log::trace!("TrackAudio TX end");
                state.set_transmitting(app, false);
            }
            Event::RxBegin(rx_begin) => {
                state.set_receiving(app, rx_begin.frequency, true);
            }
            Event::RxEnd(rx_end) => {
                state.set_receiving(
                    app,
                    rx_end.frequency,
                    !rx_end.active_transmitters.is_none_or(|t| t.is_empty()),
                );
            }
            Event::VoiceConnectedState(payload) => {
                log::trace!("TrackAudio voice connection state changed: {payload:?}");
                state.set_voice_connected(app, payload.connected);

                let station_states = if payload.connected {
                    client
                        .api()
                        .get_station_states(Some(Self::STATION_STATES_TIMEOUT))
                        .await
                        .unwrap_or_default()
                } else {
                    vec![]
                };

                state.sync_stations(app, station_states);
            }
            Event::Client(ClientEvent::ConnectionStateChanged(connection_state)) => {
                match connection_state {
                    ConnectionState::Connected => {
                        let state = app.state::<AppState>();
                        let state = state.lock().await;

                        let radio = app.state::<RadioHandle>().read().clone();

                        if let Some(radio) = radio {
                            log::info!("trackaudio radio state connected; starting recorder");
                            let _ = state.config.client.playback.start(app, radio).await;
                        }
                    }
                    _ => {
                        let handle = app.state::<PlaybackRecorderHandle>();
                        stop_playing_source(&handle, &app.state::<AudioManagerHandle>());
                        let existing = handle.write().take();
                        if let Some(recorder) = existing {
                            recorder.shutdown().await;
                            log::info!(
                                "trackaudio radio state changed to {connection_state:?}; stopped active recorder"
                            );
                        }
                    }
                }
                Self::handle_connection_state(connection_state, state, app, client).await;
            }
            Event::Client(ClientEvent::CommandSendFailed { error, command }) => {
                log::warn!(
                    "TrackAudio client command send failed. Command: {command:?}. Err: {error}"
                );
                app.emit("radio:state", RadioState::Error).ok();
            }
            Event::Client(ClientEvent::EventDeserializationFailed { error, raw }) => {
                log::warn!(
                    "TrackAudio client event deserialization failed. Raw Message: {raw}. Err: {error}"
                );
            }
            Event::StationAdded(payload) => {
                log::trace!("TrackAudio station added: {}", payload.callsign);
                state.add_station(app, payload.callsign, payload.frequency);
            }
            Event::StationStateUpdate(payload) => {
                log::trace!("TrackAudio station state update: {payload:?}");
                state.update_station(app, &payload);
            }
            Event::StationStates(payload) => {
                log::trace!(
                    "Received full station states list for {} stations",
                    payload.stations.len()
                );
                state.sync_stations(app, payload.stations.into_iter().map(|s| s.value).collect());
            }
            Event::FrequencyRemoved(payload) => {
                log::trace!("TrackAudio frequency removed: {:?}", payload.frequency);
                state.remove_station(app, payload.frequency);
            }
            _ => {
                log::trace!("Received TrackAudio event: {event:?}");
            }
        }
    }

    async fn handle_connection_state(
        connection_state: ConnectionState,
        state: &TrackAudioState,
        app: &AppHandle,
        client: &TrackAudioClient,
    ) {
        match connection_state {
            ConnectionState::Connected => {
                log::debug!("Successfully connected to TrackAudio");
                state.set_connected(app, true); // This will emit, but we do more specific emissions after fetch

                let api = client.api();
                let voice_connected = api
                    .get_voice_connected_state(Some(Self::VOICE_CONNECTED_STATE_TIMEOUT))
                    .await
                    .unwrap_or(false);
                state.apply_voice_connected(voice_connected);

                let station_states = api
                    .get_station_states(Some(Self::STATION_STATES_TIMEOUT))
                    .await
                    .unwrap_or_default();

                state.sync_stations(app, station_states);
            }
            ConnectionState::Connecting { .. } | ConnectionState::Reconnecting { .. } => {
                state.clear();
                state.emit(app);
            }
            ConnectionState::Disconnected { .. } => {
                state.clear();
                state.emit(app);
            }
            ConnectionState::ReconnectFailed { .. } => {
                log::warn!("TrackAudio reconnect failed");
                state.clear();
                app.emit("radio:state", RadioState::Error).ok();
            }
        }
    }
}

#[async_trait::async_trait]
impl Radio for TrackAudioRadio {
    async fn transmit(&self, state: TransmissionState) -> Result<(), RadioError> {
        let active = match state {
            TransmissionState::Active if !self.state.ptt_active.swap(true, Ordering::Relaxed) => {
                true
            }
            TransmissionState::Inactive if self.state.ptt_active.swap(false, Ordering::Relaxed) => {
                false
            }
            _ => {
                log::trace!("Ignoring redundant transmission request {state:?}");
                return Ok(());
            }
        };

        log::trace!("Setting transmission {state:?}, sending active {active}");

        let result = self
            .client
            .api()
            .transmit(active, Some(Self::TRANSMIT_TIMEOUT))
            .await;

        match result {
            Ok(()) => {
                if self.state.transmit_failures.swap(0, Ordering::Relaxed)
                    >= TRANSMIT_FAILURE_THRESHOLD
                {
                    self.state.emit(&self.app);
                }

                Ok(())
            }
            Err(err) => {
                // Only a send-side failure proves the command never reached the client task. On a
                // timed out ack the command *was* enqueued, so the latch still reflects what
                // TrackAudio was told - rolling it back there would re-latch a released PTT and
                // silently no-op the next press.
                if matches!(
                    err,
                    TrackAudioError::Send(_) | TrackAudioError::ClientTaskTerminated
                ) {
                    self.state.ptt_active.store(!active, Ordering::Relaxed);
                }

                self.state.transmit_failures.fetch_add(1, Ordering::Relaxed);

                if matches!(err, TrackAudioError::Timeout) {
                    // Reports `Error` only once the failure streak crosses the threshold.
                    self.state.emit(&self.app);
                } else {
                    self.app.emit("radio:state", RadioState::Error).ok();
                }

                Err(RadioError::Transmit(format!(
                    "Failed to transmit via TrackAudio: {err}"
                )))
            }
        }
    }

    async fn reconnect(&self) -> Result<(), RadioError> {
        self.state.clear();
        self.state.emit(&self.app);
        self.client.reconnect().map_err(|err| {
            self.app.emit("radio:state", RadioState::Error).ok();
            RadioError::Integration(format!("Failed to reconnect to TrackAudio: {err}"))
        })?;
        Ok(())
    }

    fn state(&self) -> RadioState {
        self.state.as_ref().into()
    }

    async fn add_station(&self, callsign: &str) -> Result<RadioStation, RadioError> {
        self.client
            .api()
            .add_station(callsign, Some(Self::ADD_STATION_TIMEOUT))
            .await
            .map(|s| RadioStation::from(&s))
            .map_err(|err| RadioError::Integration(format!("Failed to add station: {err}")))
    }

    async fn set_station_state(
        &self,
        frequency: Frequency,
        update: StationStateUpdate,
    ) -> Result<RadioStation, RadioError> {
        let mut cmd = SetStationState::new(frequency);
        if let Some(rx) = update.rx {
            cmd = cmd.rx(rx);
        }
        if let Some(tx) = update.tx {
            cmd = cmd.tx(tx);
        }
        if let Some(xca) = update.xca {
            cmd = cmd.xca(xca);
        }
        if let Some(headset) = update.headset {
            cmd = cmd.headset(headset);
        }
        if let Some(output_muted) = update.output_muted {
            cmd = cmd.output_muted(output_muted);
        }

        self.client
            .api()
            .set_station_state(cmd, Some(Self::STATION_STATE_TIMEOUT))
            .await
            .map(|s| RadioStation::from(&s))
            .map_err(|err| RadioError::Integration(format!("Failed to set station state: {err}")))
    }

    async fn get_stations(&self) -> Result<Vec<RadioStation>, RadioError> {
        Ok(self.state.stations())
    }

    async fn fast_couple(&self) -> Result<(), RadioError> {
        for station in self.state.stations().iter().filter(|s| s.tx && !s.xca) {
            let cmd = SetStationState::new(station.frequency).xca(true);
            if let Err(err) = self
                .client
                .api()
                .set_station_state(cmd, Some(Self::STATION_STATE_TIMEOUT))
                .await
            {
                log::warn!("Failed to fast couple station {}: {err}", station.frequency);
            }
        }

        Ok(())
    }

    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

impl Debug for TrackAudioRadio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackAudioRadio")
            .field("state", &self.state)
            .field("client", &self.client)
            .finish()
    }
}

impl Drop for TrackAudioRadio {
    fn drop(&mut self) {
        log::debug!("Dropping TrackAudioRadio");

        if self.state.ptt_active.load(Ordering::Relaxed)
            && let Err(err) =
                tauri::async_runtime::block_on(self.transmit(TransmissionState::Inactive))
        {
            log::warn!("Failed to set transmission Inactive while dropping: {err}");
        }

        self.state.clear();
        self.app.emit("radio:state", RadioState::NotConfigured).ok();

        self.cancellation_token.cancel();
    }
}

#[derive(Default)]
pub(crate) struct TrackAudioState {
    connected: AtomicBool,
    voice_connected: AtomicBool,
    transmitting: AtomicBool,
    receiving: RwLock<HashSet<Frequency>>,
    stations: RwLock<HashMap<Frequency, RadioStation>>,

    /// Local press/release edge latch, so a held key does not re-send `kPttPressed`.
    ///
    /// Lives here rather than on [`TrackAudioRadio`] so that [`TrackAudioState::clear`] resets it
    /// on every disconnect. A latch stuck at `true` while TrackAudio is not transmitting makes
    /// every later press a silent no-op, and a fresh socket means TrackAudio's PTT state is fresh.
    ptt_active: AtomicBool,

    /// Consecutive failed transmit attempts, see [`TRANSMIT_FAILURE_THRESHOLD`].
    transmit_failures: AtomicUsize,
}

impl From<&StationState> for RadioStation {
    fn from(s: &StationState) -> Self {
        Self {
            callsign: s.callsign.clone(),
            frequency: s.frequency.unwrap_or(Frequency::from_mhz(199.998)),
            rx: s.rx.unwrap_or(false),
            tx: s.tx.unwrap_or(false),
            xc: s.xc.unwrap_or(false),
            xca: s.xca.unwrap_or(false),
            headset: s.headset.unwrap_or(true),
            output_muted: s.is_output_muted.unwrap_or(false),
            is_available: s.is_available,
        }
    }
}

impl From<&TrackAudioState> for RadioState {
    fn from(value: &TrackAudioState) -> Self {
        if !value.connected.load(Ordering::Relaxed) {
            return RadioState::Disconnected;
        }

        if !value.voice_connected.load(Ordering::Relaxed) {
            return RadioState::Connected;
        }

        // A run of failed transmits means PTT is doing nothing while the socket still looks
        // healthy, which is otherwise invisible to the user. Checked below the voice connection so
        // that transmitting while TrackAudio is off the network stays a plain `Connected`: those
        // timeouts are expected, and reconnecting the socket would not fix them anyway.
        if value.transmit_failures.load(Ordering::Relaxed) >= TRANSMIT_FAILURE_THRESHOLD {
            return RadioState::Error;
        }

        // Priority: TX > RX > Idle
        if value.transmitting.load(Ordering::Relaxed) {
            return RadioState::TxActive;
        }

        {
            let receiving = value.receiving.read();
            if !receiving.is_empty() {
                return RadioState::RxActive(receiving.clone());
            }
        }

        if value.stations.read().values().any(|s| s.rx) {
            return RadioState::RxIdle;
        }

        RadioState::VoiceConnected
    }
}

impl From<TrackAudioState> for RadioState {
    fn from(value: TrackAudioState) -> Self {
        Self::from(&value)
    }
}

impl Debug for TrackAudioState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackAudioState")
            .field("connected", &self.connected)
            .field("voice_connected", &self.voice_connected)
            .field("transmitting", &self.transmitting)
            .field("ptt_active", &self.ptt_active)
            .field("transmit_failures", &self.transmit_failures)
            .field("receiving", &self.receiving)
            .field("stations", &self.stations.read().len())
            .finish()
    }
}

impl TrackAudioState {
    fn emit(&self, app: &AppHandle) {
        RadioState::from(self).emit(app);
    }

    fn clear(&self) {
        self.connected.store(false, Ordering::Relaxed);
        self.voice_connected.store(false, Ordering::Relaxed);
        self.transmitting.store(false, Ordering::Relaxed);
        self.ptt_active.store(false, Ordering::Relaxed);
        self.transmit_failures.store(0, Ordering::Relaxed);
        self.receiving.write().clear();
        self.stations.write().clear();
    }

    /// Returns the cached `headset` flag for `frequency`, or `None` if the station is unknown.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn headset_for_frequency(&self, frequency: Frequency) -> Option<bool> {
        self.stations.read().get(&frequency).map(|s| s.headset)
    }

    fn set_transmitting(&self, app: &AppHandle, active: bool) {
        self.transmitting.store(active, Ordering::Relaxed);
        self.emit(app);
    }

    fn set_receiving(&self, app: &AppHandle, frequency: Frequency, active: bool) {
        if active {
            self.receiving.write().insert(frequency);
        } else {
            self.receiving.write().remove(&frequency);
        }
        self.emit(app);
    }

    fn apply_voice_connected(&self, connected: bool) {
        self.voice_connected.store(connected, Ordering::Relaxed);
        // Failures collected under the previous voice state say nothing about the new one. Without
        // this, pressing PTT while TrackAudio is off the network banks up a streak that surfaces as
        // an error the instant it connects.
        self.transmit_failures.store(0, Ordering::Relaxed);
    }

    fn set_voice_connected(&self, app: &AppHandle, connected: bool) {
        self.apply_voice_connected(connected);
        self.emit(app);
    }

    fn set_connected(&self, app: &AppHandle, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
        self.emit(app);
    }

    fn add_station(&self, app: &AppHandle, callsign: String, frequency: Frequency) {
        let station = RadioStation {
            callsign: Some(callsign),
            frequency,
            rx: false,
            tx: false,
            xc: false,
            xca: false,
            headset: false,
            output_muted: false,
            is_available: true,
        };

        {
            self.stations.write().insert(frequency, station.clone());
        }

        app.emit("radio:station-added", &station).ok();
        self.emit(app);
    }

    fn update_station(&self, app: &AppHandle, station_state: &StationState) {
        let Some(frequency) = station_state.frequency else {
            return;
        };

        {
            let mut stations = self.stations.write();
            if !station_state.is_available {
                stations.remove(&frequency);
                self.receiving.write().remove(&frequency);
                app.emit("radio:station-removed", frequency).ok();
            } else if let Some(existing) = stations.get_mut(&frequency) {
                if let Some(rx) = station_state.rx {
                    if !rx {
                        self.receiving.write().remove(&frequency);
                    }
                    existing.rx = rx;
                }
                if let Some(tx) = station_state.tx {
                    existing.tx = tx;
                }
                if let Some(xc) = station_state.xc {
                    existing.xc = xc;
                }
                if let Some(xca) = station_state.xca {
                    existing.xca = xca;
                }
                if let Some(headset) = station_state.headset {
                    existing.headset = headset;
                }
                if let Some(output_muted) = station_state.is_output_muted {
                    existing.output_muted = output_muted;
                }
                if let Some(callsign) = &station_state.callsign {
                    existing.callsign = Some(callsign.clone());
                }
                app.emit("radio:station-updated", &*existing).ok();
            } else {
                let station = RadioStation::from(station_state);
                app.emit("radio:station-added", &station).ok();
                stations.insert(frequency, station);
            }
        }

        self.emit(app);
    }

    fn remove_station(&self, app: &AppHandle, frequency: Frequency) {
        {
            self.stations.write().remove(&frequency);
            self.receiving.write().remove(&frequency);
        }

        app.emit("radio:station-removed", frequency).ok();
        self.emit(app);
    }

    fn sync_stations(&self, app: &AppHandle, station_states: Vec<StationState>) {
        {
            let mut stations = self.stations.write();
            stations.clear();

            for station_state in &station_states {
                if station_state.is_available
                    && let Some(frequency) = station_state.frequency
                {
                    stations.insert(frequency, RadioStation::from(station_state));
                }
            }

            {
                let mut receiving = self.receiving.write();
                *receiving = receiving
                    .iter()
                    .filter(|f| stations.contains_key(f))
                    .cloned()
                    .collect();
            }
        }

        let synced: Vec<RadioStation> = self.stations.read().values().cloned().collect();
        app.emit("radio:stations-synced", &synced).ok();
        self.emit(app);
    }

    fn stations(&self) -> Vec<RadioStation> {
        self.stations.read().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice_connected_state() -> TrackAudioState {
        let state = TrackAudioState::default();
        state.connected.store(true, Ordering::Relaxed);
        state.voice_connected.store(true, Ordering::Relaxed);
        state
    }

    #[test]
    fn clear_resets_ptt_latch() {
        let state = TrackAudioState::default();
        state.ptt_active.store(true, Ordering::Relaxed);

        state.clear();

        // A latch stuck at `true` makes every later press a silent no-op, so a disconnect (which
        // resets TrackAudio's own PTT state) has to drop it.
        assert!(!state.ptt_active.load(Ordering::Relaxed));
    }

    #[test]
    fn clear_resets_transmit_failure_streak() {
        let state = voice_connected_state();
        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD, Ordering::Relaxed);

        state.clear();

        assert_eq!(state.transmit_failures.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn radio_state_reports_error_only_once_failures_reach_threshold() {
        let state = voice_connected_state();
        assert_eq!(RadioState::from(&state), RadioState::VoiceConnected);

        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD - 1, Ordering::Relaxed);
        assert_eq!(RadioState::from(&state), RadioState::VoiceConnected);

        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD, Ordering::Relaxed);
        assert_eq!(RadioState::from(&state), RadioState::Error);
    }

    #[test]
    fn radio_state_recovers_when_failure_streak_is_reset() {
        let state = voice_connected_state();
        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD, Ordering::Relaxed);
        assert_eq!(RadioState::from(&state), RadioState::Error);

        state.transmit_failures.store(0, Ordering::Relaxed);
        assert_eq!(RadioState::from(&state), RadioState::VoiceConnected);
    }

    #[test]
    fn voice_connecting_resets_the_transmit_failure_streak() {
        let state = voice_connected_state();
        state.voice_connected.store(false, Ordering::Relaxed);
        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD * 4, Ordering::Relaxed);

        state.apply_voice_connected(true);

        assert_eq!(state.transmit_failures.load(Ordering::Relaxed), 0);
        assert_eq!(RadioState::from(&state), RadioState::VoiceConnected);
    }

    #[test]
    fn failure_streak_is_ignored_while_voice_is_disconnected() {
        let state = voice_connected_state();
        state.voice_connected.store(false, Ordering::Relaxed);
        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD, Ordering::Relaxed);

        assert_eq!(RadioState::from(&state), RadioState::Connected);
    }

    #[test]
    fn disconnected_takes_precedence_over_failure_streak() {
        let state = TrackAudioState::default();
        state
            .transmit_failures
            .store(TRANSMIT_FAILURE_THRESHOLD, Ordering::Relaxed);

        assert_eq!(RadioState::from(&state), RadioState::Disconnected);
    }
}
