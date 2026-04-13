pub mod commands;
pub mod push_to_talk;
pub mod track_audio;

use crate::platform::Capabilities;
use keyboard_types::KeyState;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::Arc;
use tauri::Emitter;
use thiserror::Error;
pub use trackaudio::Frequency;

#[derive(Debug, Clone, Error)]
pub enum RadioError {
    #[error("Radio integration error: {0}")]
    Integration(String),
    #[error("Radio transmit error: {0}")]
    Transmit(String),
    #[error("Operation not supported by this radio integration")]
    NotSupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum RadioIntegration {
    AudioForVatsim,
    TrackAudio,
}

impl Default for RadioIntegration {
    fn default() -> Self {
        if Capabilities::default().keybind_emitter {
            RadioIntegration::AudioForVatsim
        } else {
            RadioIntegration::TrackAudio
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransmissionState {
    Active,
    Inactive,
}

impl From<TransmissionState> for KeyState {
    fn from(value: TransmissionState) -> Self {
        match value {
            TransmissionState::Active => KeyState::Down,
            TransmissionState::Inactive => KeyState::Up,
        }
    }
}

impl From<KeyState> for TransmissionState {
    fn from(value: KeyState) -> Self {
        match value {
            KeyState::Down => TransmissionState::Active,
            KeyState::Up => TransmissionState::Inactive,
        }
    }
}

/// Radio state representing the current operational status of the chosen radio integration.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq, Hash)]
pub enum RadioState {
    #[default]
    /// No radio integration configured.
    NotConfigured,

    /// Radio configured but not connected to backend.
    /// This includes initial connection attempts, reconnection attempts, and disconnected states.
    Disconnected,

    /// Connected to a radio backend, but the backend itself is not connected to VATSIM voice server.
    Connected,

    /// Connected to a radio backend, which is connected to the VATSIM voice server.
    VoiceConnected,

    /// Connected to a radio backend and monitoring at least one frequency (RX ready).
    RxIdle,

    /// Connected and receiving transmission from others.
    RxActive,

    /// Connected and actively transmitting.
    /// May or may not be receiving simultaneously (TX takes priority).
    TxActive,

    /// Fatal connection error or client error event.
    Error,
}

impl RadioState {
    pub fn emit(&self, app: &tauri::AppHandle) {
        app.emit("radio:state", self).ok();
    }
}

/// A radio station with its current state, owned by vacs.
///
/// This is the vacs-canonical station representation, decoupled from any specific
/// radio backend (e.g. TrackAudio). Backend-specific types are converted into this.
#[derive(Debug, Clone, Serialize)]
pub struct RadioStation {
    pub callsign: Option<String>,
    pub frequency: Frequency,
    pub rx: bool,
    pub tx: bool,
    /// Read-only cross-coupling state computed by the radio backend (e.g. AFV-Native).
    /// Not user-controllable. See [`xca`](Self::xca) for the user-settable variant.
    pub xc: bool,
    /// User-controllable "cross-couple across" mode. This is the only cross-coupling
    /// field that can be set via [`StationStateUpdate`].
    pub xca: bool,
    pub headset: bool,
    pub output_muted: bool,
    pub is_available: bool,
}

/// Partial update for a station's state. Only provided (`Some`) fields are changed.
///
/// Note: `xc` is intentionally absent here. It is read-only state computed by the
/// radio backend (e.g. AFV-Native). Only `xca` (cross-couple across) is user-controllable.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StationStateUpdate {
    pub rx: Option<bool>,
    pub tx: Option<bool>,
    pub xca: Option<bool>,
    pub headset: Option<bool>,
    pub output_muted: Option<bool>,
}

#[async_trait::async_trait]
pub trait Radio: Send + Sync + Debug + 'static {
    async fn transmit(&self, state: TransmissionState) -> Result<(), RadioError>;
    async fn reconnect(&self) -> Result<(), RadioError> {
        Ok(())
    }

    fn state(&self) -> RadioState;

    async fn add_station(&self, _callsign: &str) -> Result<RadioStation, RadioError> {
        Err(RadioError::NotSupported)
    }

    async fn set_station_state(
        &self,
        _frequency: Frequency,
        _update: StationStateUpdate,
    ) -> Result<RadioStation, RadioError> {
        Err(RadioError::NotSupported)
    }

    async fn get_stations(&self) -> Result<Vec<RadioStation>, RadioError> {
        Err(RadioError::NotSupported)
    }
}

pub type DynRadio = Arc<dyn Radio>;
