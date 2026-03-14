use std::fmt::{Display, Formatter};
use std::ops::Add;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio_tungstenite::tungstenite;
use vacs_protocol::ws::server::{DisconnectReason, LoginFailureReason};
use vacs_protocol::ws::shared::ErrorReason;

#[derive(Debug, Error)]
pub enum SignalingError {
    #[error("login failed: {0:?}")]
    LoginError(LoginFailureReason),
    #[error("transport error: {0}")]
    Transport(#[from] Box<tungstenite::error::Error>),
    #[error("signaling protocol error: {0}")]
    ProtocolError(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("runtime error: {0:?}")]
    Runtime(SignalingRuntimeError),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Error)]
pub enum SignalingRuntimeError {
    #[error("disconnected: {0:?}")]
    Disconnected(Option<DisconnectReason>),
    #[error("reconnect failed: {0:?}")]
    ReconnectFailed(ReconnectFailureReason),
    #[error("aborting automatic reconnect due to rapid failures")]
    ReconnectSuppressed(UntilInstant),
    #[error("server error: {0:?}")]
    ServerError(ErrorReason),
    #[error("transport error: {0:?}")]
    Transport(TransportFailureReason),
    #[error("serialization error: {0}")]
    SerializationError(String),
    #[error("rate limited for {0}")]
    RateLimited(UntilInstant),
}

impl SignalingRuntimeError {
    pub fn can_reconnect(&self) -> bool {
        matches!(self, SignalingRuntimeError::Disconnected(reason) if reason.is_none())
            || matches!(
                self,
                SignalingRuntimeError::ServerError(_)
                    | SignalingRuntimeError::Transport(_)
                    | SignalingRuntimeError::SerializationError(_)
            )
    }

    /// Whether this error indicates a connection loss where the server may not have detected
    /// the disconnect yet. In such cases, the existing session should be terminated before
    /// reconnecting to avoid a duplicate client ID rejection.
    pub fn needs_session_terminate(&self) -> bool {
        matches!(
            self,
            SignalingRuntimeError::Disconnected(None) | SignalingRuntimeError::Transport(_)
        )
    }

    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            SignalingRuntimeError::Disconnected(_)
                | SignalingRuntimeError::ReconnectFailed(_)
                | SignalingRuntimeError::ReconnectSuppressed(_)
                | SignalingRuntimeError::ServerError(_)
                | SignalingRuntimeError::Transport(_)
        )
    }
}

impl From<SignalingRuntimeError> for SignalingError {
    fn from(value: SignalingRuntimeError) -> Self {
        SignalingError::Runtime(value)
    }
}

#[derive(Debug, Clone)]
pub enum ReconnectFailureReason {
    Connection,
    Login(LoginFailureReason),
    Other(String),
}

#[derive(Debug, Clone)]
pub enum TransportFailureReason {
    Send(String),
    Receive(String),
    Close(String),
}

impl From<SignalingError> for ReconnectFailureReason {
    fn from(value: SignalingError) -> ReconnectFailureReason {
        match value {
            SignalingError::LoginError(reason) => ReconnectFailureReason::Login(reason),
            SignalingError::Transport(_) => ReconnectFailureReason::Connection,
            SignalingError::ProtocolError(reason) => ReconnectFailureReason::Other(reason),
            SignalingError::Timeout(reason) => ReconnectFailureReason::Other(reason),
            SignalingError::Runtime(error) => match error {
                SignalingRuntimeError::Disconnected(_)
                | SignalingRuntimeError::ServerError(_)
                | SignalingRuntimeError::SerializationError(_) => {
                    ReconnectFailureReason::Connection
                }
                _ => {
                    unreachable!("SignalingRuntimeError is not valid as ReconnectFailureReason");
                }
            },
            SignalingError::Other(reason) => ReconnectFailureReason::Other(reason),
        }
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct UntilInstant(pub Instant);

impl Display for UntilInstant {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.0.checked_duration_since(Instant::now()) {
            Some(dur) => write!(f, "{:.0?}", dur),
            None => write!(f, "0s"),
        }
    }
}

impl From<Instant> for UntilInstant {
    fn from(value: Instant) -> UntilInstant {
        UntilInstant(value)
    }
}

impl From<u64> for UntilInstant {
    fn from(value: u64) -> UntilInstant {
        UntilInstant(Instant::now().add(Duration::from_secs(value)))
    }
}
