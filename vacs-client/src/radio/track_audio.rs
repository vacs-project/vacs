use crate::radio::{
    Frequency, Radio, RadioError, RadioState, RadioStation, StationStateUpdate, TransmissionState,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use trackaudio::messages::commands::SetStationState;
use trackaudio::messages::events::StationState;
use trackaudio::{
    ClientEvent, ConnectionState, TrackAudioClient, TrackAudioConfig, TrackAudioError,
};

#[derive(Clone)]
pub struct TrackAudioRadio {
    #[allow(dead_code)]
    app: AppHandle,
    client: TrackAudioClient,
    active: Arc<AtomicBool>,
    state: Arc<TrackAudioState>,
    cancellation_token: CancellationToken,
}

impl TrackAudioRadio {
    const TRANSMIT_TIMEOUT: Duration = Duration::from_millis(250);
    const VOICE_CONNECTED_STATE_TIMEOUT: Duration = Duration::from_millis(250);
    const STATION_STATES_TIMEOUT: Duration = Duration::from_millis(250);
    const STATION_STATE_TIMEOUT: Duration = Duration::from_millis(250);

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

        let active = Arc::new(AtomicBool::new(false));
        let state = Arc::new(TrackAudioState::default());

        {
            let app = app.clone();
            let client = client.clone();
            let token = cancellation_token.clone();
            let state = state.clone();

            tauri::async_runtime::spawn(async move {
                Self::events_task(app, client, token, state).await;
            });
        }

        let radio = Self {
            app,
            client,
            active,
            state,
            cancellation_token,
        };

        Ok(radio)
    }

    async fn events_task(
        app: AppHandle,
        client: TrackAudioClient,
        cancellation_token: CancellationToken,
        state: Arc<TrackAudioState>,
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
                        Ok(event) => Self::handle_event(event, &state, &app, &client).await,
                        Err(err) => {
                            log::error!("Error receiving TrackAudio event: {err}");
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
                log::trace!("TrackAudio transmission started");
                state.set_transmitting(app, true);
            }
            Event::TxEnd(_) => {
                log::trace!("TrackAudio transmission ended");
                state.set_transmitting(app, false);
            }
            Event::RxBegin(_) => {
                log::trace!("TrackAudio reception started");
                state.set_receiving(app, true);
            }
            Event::RxEnd(_) => {
                log::trace!("TrackAudio reception ended");
                state.set_receiving(app, false);
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
                state
                    .voice_connected
                    .store(voice_connected, Ordering::Relaxed);

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
            TransmissionState::Active if !self.active.swap(true, Ordering::Relaxed) => true,
            TransmissionState::Inactive if self.active.swap(false, Ordering::Relaxed) => false,
            _ => return Ok(()),
        };

        log::trace!("Setting transmission {state:?}, sending active {active}");

        self.client
            .api()
            .transmit(active, Some(Self::TRANSMIT_TIMEOUT))
            .await
            .map_err(|err| {
                if !matches!(err, TrackAudioError::Timeout) {
                    self.app.emit("radio:state", RadioState::Error).ok();
                }
                RadioError::Transmit(format!("Failed to transmit via TrackAudio: {err}"))
            })?;

        Ok(())
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
            .add_station(callsign, Some(Self::STATION_STATE_TIMEOUT))
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
}

impl Debug for TrackAudioRadio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackAudioRadio")
            .field("active", &self.active)
            .field("state", &self.state)
            .field("client", &self.client)
            .finish()
    }
}

impl Drop for TrackAudioRadio {
    fn drop(&mut self) {
        log::debug!("Dropping TrackAudioRadio");

        if self.active.load(Ordering::Relaxed)
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
struct TrackAudioState {
    connected: AtomicBool,
    voice_connected: AtomicBool,
    transmitting: AtomicBool,
    receiving: AtomicBool,
    stations: RwLock<HashMap<Frequency, RadioStation>>,
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

        // Priority: TX > RX > Idle
        if value.transmitting.load(Ordering::Relaxed) {
            return RadioState::TxActive;
        }

        if value.receiving.load(Ordering::Relaxed) {
            return RadioState::RxActive;
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
        self.receiving.store(false, Ordering::Relaxed);
        self.stations.write().clear();
    }

    fn set_transmitting(&self, app: &AppHandle, active: bool) {
        self.transmitting.store(active, Ordering::Relaxed);
        self.emit(app);
    }

    fn set_receiving(&self, app: &AppHandle, active: bool) {
        self.receiving.store(active, Ordering::Relaxed);
        self.emit(app);
    }

    fn set_voice_connected(&self, app: &AppHandle, connected: bool) {
        self.voice_connected.store(connected, Ordering::Relaxed);
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
                app.emit("radio:station-removed", frequency).ok();
            } else if let Some(existing) = stations.get_mut(&frequency) {
                if let Some(rx) = station_state.rx {
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
        }

        let synced: Vec<RadioStation> = self.stations.read().values().cloned().collect();
        app.emit("radio:stations-synced", &synced).ok();
        self.emit(app);
    }

    fn stations(&self) -> Vec<RadioStation> {
        self.stations.read().values().cloned().collect()
    }
}
