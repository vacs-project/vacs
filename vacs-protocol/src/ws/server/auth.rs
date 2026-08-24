use crate::vatsim::PositionId;
use crate::ws::server::ServerMessage;
use crate::ws::shared::UnknownReason;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginFailureReason {
    Unauthorized,
    DuplicateId,
    InvalidCredentials,
    NoActiveVatsimConnection,
    AmbiguousVatsimPosition(Vec<PositionId>),
    InvalidVatsimPosition,
    Timeout,
    IncompatibleProtocolVersion,
    /// Forward compatibility: a reason this protocol version does not know.
    /// Treat as a generic login failure.
    #[serde(untagged)]
    Unknown(UnknownReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisconnectReason {
    Terminated,
    NoActiveVatsimConnection,
    AmbiguousVatsimPosition(Vec<PositionId>),
    /// Forward compatibility: a reason this protocol version does not know.
    /// Treat as a generic disconnect.
    #[serde(untagged)]
    Unknown(UnknownReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginFailure {
    pub reason: LoginFailureReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disconnected {
    pub reason: DisconnectReason,
}

impl From<LoginFailureReason> for LoginFailure {
    fn from(reason: LoginFailureReason) -> Self {
        Self { reason }
    }
}

impl From<LoginFailure> for ServerMessage {
    fn from(value: LoginFailure) -> Self {
        Self::LoginFailure(value)
    }
}

impl From<LoginFailureReason> for ServerMessage {
    fn from(value: LoginFailureReason) -> Self {
        Self::LoginFailure(value.into())
    }
}

impl From<DisconnectReason> for Disconnected {
    fn from(reason: DisconnectReason) -> Self {
        Self { reason }
    }
}

impl From<Disconnected> for ServerMessage {
    fn from(value: Disconnected) -> Self {
        Self::Disconnected(value)
    }
}

impl From<DisconnectReason> for ServerMessage {
    fn from(value: DisconnectReason) -> Self {
        Self::Disconnected(value.into())
    }
}
