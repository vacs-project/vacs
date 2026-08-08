use std::collections::HashMap;

use crate::vatsim::{ClientId, PositionId, StationId};
use crate::ws::client::ClientMessage;
use crate::ws::server::ServerMessage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
#[serde(transparent)]
pub struct CallId(Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSource {
    pub client_id: ClientId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<PositionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_id: Option<StationId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallTarget {
    Client(ClientId),
    Position(PositionId),
    Station(StationId),
}

impl PartialOrd for CallTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CallTarget {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (CallTarget::Station(station_id), CallTarget::Station(other_station_id)) => {
                station_id.cmp(other_station_id)
            }
            (CallTarget::Station(_), CallTarget::Position(_))
            | (CallTarget::Station(_), CallTarget::Client(_))
            | (CallTarget::Position(_), CallTarget::Client(_)) => std::cmp::Ordering::Less,
            (CallTarget::Client(client_id), CallTarget::Client(other_client_id)) => {
                client_id.cmp(other_client_id)
            }
            (CallTarget::Client(_), CallTarget::Position(_))
            | (CallTarget::Client(_), CallTarget::Station(_))
            | (CallTarget::Position(_), CallTarget::Station(_)) => std::cmp::Ordering::Greater,
            (CallTarget::Position(position_id), CallTarget::Position(other_position_id)) => {
                position_id.cmp(other_position_id)
            }
        }
    }
}

impl From<CallSource> for CallTarget {
    fn from(value: CallSource) -> Self {
        if let Some(station_id) = value.station_id {
            CallTarget::Station(station_id)
        } else if let Some(position_id) = value.position_id {
            CallTarget::Position(position_id)
        } else {
            CallTarget::Client(value.client_id)
        }
    }
}

pub type CallParticipants = HashMap<ClientId, CallTarget>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallErrorReason {
    TargetNotFound,
    AlreadyParticipant,
    CallNotFound,
    CallActive,
    WebrtcFailure,
    AudioFailure,
    CallFailure,
    SignalingFailure,
    AutoHangup,
    NotConferenceLeader,
    NotParticipant,
    Other,
}

// CallInvite: CallId, Target, Source, Invited/Pending_Participants, Joined/Active_Participants
// CallUpdate: CallId, Invited/Pending_Participants, Joined/Active_Participants -> sent WHENEVER a calls participants are updated, to every participant which did not reject/error

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallAccept {
    pub call_id: CallId,
    pub accepting_client_id: ClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallEnd {
    pub call_id: CallId,
    pub ending_client_id: ClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallError {
    pub call_id: CallId,
    pub reason: CallErrorReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl CallId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl std::fmt::Display for CallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for CallId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl std::str::FromStr for CallId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::try_parse(s)?))
    }
}

impl TryFrom<String> for CallId {
    type Error = uuid::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<&str> for CallId {
    type Error = uuid::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl AsRef<Uuid> for CallId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::borrow::Borrow<Uuid> for CallId {
    fn borrow(&self) -> &Uuid {
        &self.0
    }
}

impl From<ClientId> for CallSource {
    fn from(value: ClientId) -> Self {
        Self {
            client_id: value,
            position_id: None,
            station_id: None,
        }
    }
}

impl CallSource {
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            position_id: None,
            station_id: None,
        }
    }

    pub fn with_position(mut self, position_id: PositionId) -> Self {
        self.position_id = Some(position_id);
        self
    }

    pub fn with_station(mut self, station_id: StationId) -> Self {
        self.station_id = Some(station_id);
        self
    }
}

impl CallEnd {
    pub fn new(call_id: CallId, ending_client_id: ClientId) -> Self {
        Self {
            call_id,
            ending_client_id,
        }
    }
}

impl From<ClientId> for CallTarget {
    fn from(value: ClientId) -> Self {
        Self::Client(value)
    }
}

impl From<PositionId> for CallTarget {
    fn from(value: PositionId) -> Self {
        Self::Position(value)
    }
}

impl From<StationId> for CallTarget {
    fn from(value: StationId) -> Self {
        Self::Station(value)
    }
}

impl From<CallAccept> for ClientMessage {
    fn from(value: CallAccept) -> Self {
        Self::CallAccept(value)
    }
}

impl From<CallAccept> for ServerMessage {
    fn from(value: CallAccept) -> Self {
        Self::CallAccept(value)
    }
}

impl From<CallEnd> for ClientMessage {
    fn from(value: CallEnd) -> Self {
        Self::CallEnd(value)
    }
}

impl From<CallEnd> for ServerMessage {
    fn from(value: CallEnd) -> Self {
        Self::CallEnd(value)
    }
}

impl From<CallError> for ClientMessage {
    fn from(value: CallError) -> Self {
        Self::CallError(value)
    }
}

impl From<CallError> for ServerMessage {
    fn from(value: CallError) -> Self {
        Self::CallError(value)
    }
}
