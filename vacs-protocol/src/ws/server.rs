pub mod auth;
pub mod calls;
pub mod network;

pub use auth::*;
pub use calls::*;
pub use network::*;

use crate::ws::shared::{CallEnd, CallError, Error, WebrtcAnswer, WebrtcIceCandidate, WebrtcOffer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ServerMessage {
    LoginFailure(LoginFailure),
    CallInvitation(CallInvitation),
    CallEnd(CallEnd),
    CallUpdate(CallUpdate),
    CallCancelled(CallCancelled),
    CallError(CallError),
    WebrtcOffer(WebrtcOffer),
    WebrtcAnswer(WebrtcAnswer),
    WebrtcIceCandidate(WebrtcIceCandidate),
    ClientInfo(ClientInfo),
    SessionInfo(SessionInfo),
    ClientConnected(ClientConnected),
    ClientDisconnected(ClientDisconnected),
    ClientList(ClientList),
    StationList(StationList),
    StationChanges(StationChanges),
    Disconnected(Disconnected),
    Error(Error),
}

impl ServerMessage {
    pub fn serialize(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn into_json(self) -> serde_json::Result<String> {
        self.serialize()
    }

    pub fn deserialize(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }

    pub const fn variant(&self) -> &'static str {
        match self {
            ServerMessage::LoginFailure(_) => "LoginFailure",
            ServerMessage::CallInvitation(_) => "CallInvite",
            ServerMessage::CallEnd(_) => "CallEnd",
            ServerMessage::CallUpdate(_) => "CallUpdate",
            ServerMessage::CallCancelled(_) => "CallCancelled",
            ServerMessage::CallError(_) => "CallError",
            ServerMessage::WebrtcOffer(_) => "WebrtcOffer",
            ServerMessage::WebrtcAnswer(_) => "WebrtcAnswer",
            ServerMessage::WebrtcIceCandidate(_) => "WebrtcIceCandidate",
            ServerMessage::ClientInfo(_) => "ClientInfo",
            ServerMessage::SessionInfo(_) => "SessionInfo",
            ServerMessage::ClientConnected(_) => "ClientConnected",
            ServerMessage::ClientDisconnected(_) => "ClientDisconnected",
            ServerMessage::ClientList(_) => "ClientList",
            ServerMessage::StationList(_) => "StationList",
            ServerMessage::StationChanges(_) => "StationChanges",
            ServerMessage::Disconnected(_) => "Disconnected",
            ServerMessage::Error(_) => "Error",
        }
    }
}
