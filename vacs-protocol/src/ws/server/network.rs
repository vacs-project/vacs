use crate::profile::{ActiveProfile, Profile};
use crate::vatsim::{ClientId, PositionId, StationChange, StationId};
use crate::ws::server::ServerMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "activeProfile")]
pub enum SessionProfile {
    Unchanged,
    Changed(ActiveProfile<Profile>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub id: ClientId,
    pub display_name: String,
    pub frequency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<PositionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub client: ClientInfo,
    pub profile: SessionProfile,
    #[serde(default)]
    pub default_call_sources: Vec<StationId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationInfo {
    pub id: StationId,
    pub own: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConnected {
    pub client: ClientInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDisconnected {
    pub client_id: ClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientList {
    pub clients: Vec<ClientInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationList {
    pub stations: Vec<StationInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationChanges {
    pub changes: Vec<StationChange>,
}

impl std::fmt::Display for SessionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionProfile::Unchanged => write!(f, "Unchanged"),
            SessionProfile::Changed(profile) => write!(f, "Changed({profile})"),
        }
    }
}

impl From<ActiveProfile<Profile>> for SessionProfile {
    fn from(value: ActiveProfile<Profile>) -> Self {
        Self::Changed(value)
    }
}

impl From<ClientInfo> for ServerMessage {
    fn from(value: ClientInfo) -> Self {
        Self::ClientInfo(value)
    }
}

impl From<SessionInfo> for ServerMessage {
    fn from(value: SessionInfo) -> Self {
        Self::SessionInfo(value)
    }
}

impl From<ClientInfo> for ClientConnected {
    fn from(client: ClientInfo) -> Self {
        Self { client }
    }
}

impl From<ClientConnected> for ServerMessage {
    fn from(value: ClientConnected) -> Self {
        Self::ClientConnected(value)
    }
}

impl From<ClientId> for ClientDisconnected {
    fn from(client_id: ClientId) -> Self {
        Self { client_id }
    }
}

impl From<ClientDisconnected> for ServerMessage {
    fn from(value: ClientDisconnected) -> Self {
        Self::ClientDisconnected(value)
    }
}

impl From<Vec<ClientInfo>> for ClientList {
    fn from(clients: Vec<ClientInfo>) -> Self {
        Self { clients }
    }
}

impl From<ClientList> for ServerMessage {
    fn from(value: ClientList) -> Self {
        Self::ClientList(value)
    }
}

impl From<Vec<ClientInfo>> for ServerMessage {
    fn from(value: Vec<ClientInfo>) -> Self {
        Self::ClientList(value.into())
    }
}

impl From<Vec<StationInfo>> for StationList {
    fn from(stations: Vec<StationInfo>) -> Self {
        Self { stations }
    }
}

impl From<StationList> for ServerMessage {
    fn from(value: StationList) -> Self {
        Self::StationList(value)
    }
}

impl From<Vec<StationInfo>> for ServerMessage {
    fn from(value: Vec<StationInfo>) -> Self {
        Self::StationList(value.into())
    }
}

impl From<Vec<StationChange>> for StationChanges {
    fn from(changes: Vec<StationChange>) -> Self {
        Self { changes }
    }
}

impl From<StationChanges> for ServerMessage {
    fn from(value: StationChanges) -> Self {
        Self::StationChanges(value)
    }
}

impl From<Vec<StationChange>> for ServerMessage {
    fn from(value: Vec<StationChange>) -> Self {
        Self::StationChanges(value.into())
    }
}
