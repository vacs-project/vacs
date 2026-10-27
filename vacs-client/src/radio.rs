pub mod commands;
pub mod push_to_talk;
pub mod track_audio;

use crate::error::Error;
use crate::keybinds::{InputCode, KeybindsError, TransmitConfig, Trigger};
use crate::platform::Capabilities;
use crate::radio::push_to_talk::PushToTalkRadio;
use crate::radio::track_audio::TrackAudioRadio;
use keyboard_types::{Code, KeyState};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use thiserror::Error;
pub use trackaudio::Frequency;
use vacs_macros::Frontend;

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
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub enum RadioIntegration {
    AudioForVatsim,
    TrackAudio,
}

impl Default for RadioIntegration {
    fn default() -> Self {
        if Capabilities::get().keybind_emitter {
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
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(tag = "state", content = "data")]
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
    RxActive(HashSet<Frequency>),

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
pub trait Radio: Send + Sync + Debug + Any + 'static {
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

    async fn fast_couple(&self) -> Result<(), RadioError> {
        Err(RadioError::NotSupported)
    }

    #[cfg_attr(target_os = "macos", allow(unused))]
    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

pub type DynRadio = Arc<dyn Radio>;

pub type RadioHandle = Arc<RwLock<Option<DynRadio>>>;

#[derive(Debug, Clone, Serialize, Deserialize, Default, Frontend)]
#[cfg_attr(test, derive(schemars::JsonSchema), schemars(default))]
pub struct RadioConfig {
    /// Radio client vacs controls. Unset disables the integration.
    pub integration: Option<RadioIntegration>,
    /// Audio for VATSIM integration settings.
    #[frontend(nested)]
    pub audio_for_vatsim: Option<AudioForVatsimRadioConfig>,
    /// TrackAudio integration settings.
    #[frontend(nested)]
    pub track_audio: Option<TrackAudioRadioConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Frontend)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct AudioForVatsimRadioConfig {
    /// Key code vacs presses to transmit in Audio for VATSIM, for example `"F13"`.
    #[frontend(key)]
    #[cfg_attr(test, schemars(with = "Option<String>"))]
    pub emit: Option<Code>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Frontend)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct TrackAudioRadioConfig {
    /// Address of the TrackAudio SDK. Unset uses TrackAudio's default.
    pub endpoint: Option<String>,
}

impl RadioConfig {
    /// Create a radio integration instance based on the configured integration type.
    ///
    /// Returns `None` if the integration is not configured or if the emit key is not set.
    ///
    /// # Platform Limitation
    ///
    /// **Important**: AudioForVatsim Radio integration requires a functional `KeybindEmitter` to
    /// inject key presses into external applications. This works on Windows, macOS and Linux
    /// X11 (via XTest), but NOT on Wayland where the emitter is a no-op stub due to Wayland's
    /// security model.
    ///
    /// On Wayland, this method will successfully create a radio instance, but it will
    /// silently do nothing when `transmit()` is called.
    ///
    /// The TrackAudio integration is not affected by this platform limitation and is thus the
    /// default radio implementation for Linux.
    pub async fn radio(&self, app: AppHandle) -> Result<Option<DynRadio>, Error> {
        match self.integration {
            Some(RadioIntegration::AudioForVatsim) => {
                let Some(config) = self.audio_for_vatsim.as_ref() else {
                    return Ok(None);
                };
                let Some(emit) = config.emit else {
                    return Ok(None);
                };
                log::debug!("Initializing AudioForVatsim radio integration");
                let radio = PushToTalkRadio::new(app, emit).map_err(Error::from)?;
                Ok(Some(Arc::new(radio)))
            }
            Some(RadioIntegration::TrackAudio) => {
                let endpoint = self.track_audio.as_ref().and_then(|c| c.endpoint.as_ref());
                log::debug!("Initializing TrackAudio radio integration (endpoint: {endpoint:?})");
                let radio = Arc::new(
                    TrackAudioRadio::new(app.clone(), endpoint)
                        .await
                        .map_err(Error::from)?,
                );
                Ok(Some(radio))
            }
            _ => Ok(None),
        }
    }

    pub fn validate(&self, transmit_config: &TransmitConfig) -> Result<(), Error> {
        if self.integration == Some(RadioIntegration::AudioForVatsim)
            && let Some(afv_code) = self.audio_for_vatsim.as_ref().and_then(|c| c.emit)
            && transmit_config.active_radio_trigger(true)
                == Some(Trigger::Input(InputCode::Key(afv_code)))
        {
            return Err(KeybindsError::Other(
                "AFV emit key must be distinct from your radio integration push-to-talk key"
                    .to_string(),
            )
            .into());
        }
        Ok(())
    }
}
