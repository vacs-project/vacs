use crate::app::state::AppState;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::keybinds::KeybindsError;
use crate::radio::RadioError;
use serde::Serialize;
use serde_json::Value;
use std::fmt::{Debug, Display, Formatter};
use tauri::{AppHandle, Emitter, Manager};
use thiserror::Error;
use vacs_signaling::error::{SignalingError, SignalingRuntimeError};
use vacs_signaling::protocol::ws::server::{DisconnectReason, LoginFailureReason};
use vacs_signaling::protocol::ws::shared::{CallErrorReason, CallId, ErrorReason};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unauthorized")]
    Unauthorized,
    #[error(transparent)]
    AudioDevice(#[from] Box<vacs_audio::error::AudioError>),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Signaling error: {0}")]
    Signaling(#[from] Box<SignalingError>),
    #[error("HTTP error: {0}")]
    Reqwest(#[from] Box<reqwest::Error>),
    #[error("WebRTC error: {0}")]
    Webrtc(#[from] Box<vacs_webrtc::error::WebrtcError>),
    #[error("Keybinds error: {0}")]
    Keybinds(#[from] Box<KeybindsError>),
    #[error("Radio error: {0}")]
    Radio(#[from] Box<RadioError>),
    #[error("Capability {0} not available on your platform")]
    CapabilityNotAvailable(String),
    #[error(transparent)]
    Other(#[from] Box<anyhow::Error>),
}

impl From<vacs_audio::error::AudioError> for Error {
    fn from(err: vacs_audio::error::AudioError) -> Self {
        Error::AudioDevice(Box::new(err))
    }
}

impl From<SignalingError> for Error {
    fn from(err: SignalingError) -> Self {
        Error::Signaling(Box::new(err))
    }
}

impl From<SignalingRuntimeError> for Error {
    fn from(err: SignalingRuntimeError) -> Self {
        Error::Signaling(Box::new(err.into()))
    }
}

impl From<KeybindsError> for Error {
    fn from(err: KeybindsError) -> Self {
        Error::Keybinds(Box::new(err))
    }
}

impl From<RadioError> for Error {
    fn from(err: RadioError) -> Self {
        Error::Radio(Box::new(err))
    }
}

impl From<vacs_webrtc::error::WebrtcError> for Error {
    fn from(err: vacs_webrtc::error::WebrtcError) -> Self {
        Error::Webrtc(Box::new(err))
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Reqwest(Box::new(err))
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::Other(Box::new(err))
    }
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        FrontendError::from(self).serialize(serializer)
    }
}

#[async_trait::async_trait]
pub trait HandleUnauthorizedExt<R: Send> {
    async fn handle_unauthorized(self, app: &AppHandle) -> Result<R, Error>;
}

#[async_trait::async_trait]
impl<R: Send> HandleUnauthorizedExt<R> for Result<R, Error> {
    async fn handle_unauthorized(self, app: &AppHandle) -> Result<R, Error> {
        match self {
            Ok(val) => Ok(val),
            Err(Error::Unauthorized) => {
                log::info!("Not authenticated");

                app.state::<AppState>().lock().await.set_client_id(None);
                app.emit("auth:unauthenticated", Value::Null).ok();
                Err(Error::Unauthorized)
            }
            Err(err) => Err(err),
        }
    }
}

pub trait LogErrExt<R> {
    #[allow(dead_code)]
    fn log_err(self) -> Result<R, Error>;
}

impl<R> LogErrExt<R> for Result<R, Error> {
    fn log_err(self) -> Result<R, Error> {
        match self {
            Ok(val) => Ok(val),
            Err(err) => {
                log::error!("{err:?}");
                Err(err)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendError {
    pub title: String,
    pub detail: String,
    pub is_non_critical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u16>,
}

impl FrontendError {
    pub fn new(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            is_non_critical: false,
            timeout_ms: None,
        }
    }

    pub fn non_critical(mut self) -> Self {
        self.is_non_critical = true;
        self
    }

    pub fn timeout(mut self, timeout_ms: u16) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn new_with_timeout(
        title: impl Into<String>,
        detail: impl Into<String>,
        timeout_ms: u16,
    ) -> Self {
        Self::new(title, detail).with_timeout(timeout_ms)
    }

    pub fn with_timeout(mut self, timeout_ms: u16) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

impl From<Error> for FrontendError {
    fn from(err: Error) -> Self {
        FrontendError::from(&err)
    }
}

impl From<&Error> for FrontendError {
    fn from(err: &Error) -> Self {
        match err {
            Error::Unauthorized => FrontendError::new_with_timeout(
                "Unauthorized",
                "Your authentication expired. Please log in again.",
                5000,
            ),
            Error::AudioDevice(err) => FrontendError::new("Audio device error", err.to_string()),
            Error::Reqwest(err) => FrontendError::new("HTTP error", err.to_string()),
            Error::Network(err) => FrontendError::new("Network error", err),
            Error::Signaling(err) => {
                FrontendError::new("Signaling error", format_signaling_error(err))
            }
            Error::Webrtc(err) => FrontendError::new("WebRTC error", err.to_string()),
            Error::Keybinds(err) => FrontendError::new("Keybinds error", err.to_string()),
            Error::Radio(err) => FrontendError::new("Radio error", err.to_string()),
            Error::CapabilityNotAvailable(capability) => FrontendError::new(
                "Not implemented",
                format!("{capability} functionality is not available on your platform"),
            ),
            Error::Other(err) => FrontendError::new("Error", err.to_string()),
        }
    }
}

fn format_signaling_error(err: &SignalingError) -> String {
    match err {
        SignalingError::LoginError(reason) => match reason {
            LoginFailureReason::Unauthorized => "Login failed: Unauthorized.",
            LoginFailureReason::DuplicateId => {
                "Login failed: Another client with your CID is already connected."
            }
            LoginFailureReason::InvalidCredentials => "Login failed: Invalid credentials.",
            LoginFailureReason::NoActiveVatsimConnection => {
                "Login failed: No active VATSIM connection. Wait a few seconds after connecting to VATSIM and try again."
            }
            LoginFailureReason::AmbiguousVatsimPosition(_) => {
                "Login failed: Multiple VATSIM positions matched your current position. Please select the correct position manually."
            }
            LoginFailureReason::InvalidVatsimPosition => {
                "Login failed: Selected VATSIM position is not covered by your active VATSIM connection. Wait a few seconds after connecting to VATSIM and try again."
            }
            LoginFailureReason::Timeout => {
                "Login failed: Login did not complete in time. Please try again."
            }
            LoginFailureReason::IncompatibleProtocolVersion => {
                "Login failed: Incompatible protocol version. Please check your client version."
            }
        }
        .to_string(),
        SignalingError::Runtime(runtime_err) => match runtime_err {
            SignalingRuntimeError::ServerError(reason) => match reason {
                ErrorReason::MalformedMessage => "Server error: Malformed message".to_string(),
                ErrorReason::Internal(msg) => format!("Internal server error: {msg}"),
                ErrorReason::PeerConnection => "Server error: Peer connection error.".to_string(),
                ErrorReason::UnexpectedMessage(msg) => {
                    format!("Server error: unexpected message: {msg}")
                },
                ErrorReason::RateLimited {retry_after_secs} => {
                    format!("Server error: Rate limited. Retry after {retry_after_secs}.")
                },
                ErrorReason::ClientNotFound => {
                    "Server error: Client not found.".to_string()
                }
            },
            SignalingRuntimeError::Disconnected(reason) => match reason {
                None => "Disconnected",
                Some(DisconnectReason::Terminated) => "Disconnected: Your connection was terminated by another client.",
                Some(DisconnectReason::NoActiveVatsimConnection) => "Disconnected: No active VATSIM connection was found.",
                Some(DisconnectReason::AmbiguousVatsimPosition(_)) => {
                    "Disconnected: Multiple VATSIM positions matched your current position. Please select the correct position manually."
                }
            }.to_string(),
            _ => runtime_err.to_string(),
        },
        _ => err.to_string(),
    }
}

impl From<Error> for CallErrorReason {
    fn from(err: Error) -> Self {
        match err {
            Error::AudioDevice(_) => CallErrorReason::AudioFailure,
            Error::Webrtc(err) => match err.as_ref() {
                vacs_webrtc::error::WebrtcError::CallActive => CallErrorReason::CallFailure,
                vacs_webrtc::error::WebrtcError::NoCallActive => CallErrorReason::CallFailure,
                _ => CallErrorReason::WebrtcFailure,
            },
            _ => CallErrorReason::Other,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallError {
    call_id: CallId,
    reason: String,
}

impl CallError {
    pub fn new(call_id: CallId, is_local: bool, reason: CallErrorReason) -> Self {
        Self {
            call_id,
            reason: format!(
                "{} {}",
                if is_local { "Local" } else { "Remote" },
                match reason {
                    CallErrorReason::WebrtcFailure => "Connection failure",
                    CallErrorReason::AudioFailure => "Audio failure",
                    CallErrorReason::CallFailure => "Call failure",
                    CallErrorReason::CallActive => "Call already active",
                    CallErrorReason::SignalingFailure => "Target not reachable",
                    CallErrorReason::AutoHangup => "Target did not answer",
                    CallErrorReason::Other => "Unknown failure",
                    CallErrorReason::TargetNotFound => "Call target not found",
                }
            ),
        }
    }
}

#[derive(Debug)]
pub enum StartupError {
    Audio,
    Config,
    Keybinds,
    Other,
}

impl Display for StartupError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StartupError::Audio => "No suitable output audio device found. Check your logs for further details.",
            StartupError::Config => "Failed to load configuration. Check your config files for errors or logs for further details.",
            StartupError::Keybinds => "Failed to register keybinds according to your transmit mode. Check your config files or logs for further details.",
            StartupError::Other => "A fatal error occurred during startup. Check your logs for further details.",
        })
    }
}

pub trait StartupErrorExt<T> {
    fn map_startup_err(self, error: StartupError) -> Result<T, StartupError>;
}

impl<T, E: Debug> StartupErrorExt<T> for Result<T, E> {
    fn map_startup_err(self, error: StartupError) -> Result<T, StartupError> {
        match self {
            Ok(val) => Ok(val),
            Err(err) => {
                log::error!("{err:?}");
                Err(error)
            }
        }
    }
}
