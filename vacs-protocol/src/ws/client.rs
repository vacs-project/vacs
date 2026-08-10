pub mod auth;
pub mod calls;

pub use auth::*;
pub use calls::*;

use crate::ws::shared::{CallEnd, CallError, Error, WebrtcAnswer, WebrtcIceCandidate, WebrtcOffer};
use serde::{Deserialize, Serialize};

// TODO: add CallDropTarget

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ClientMessage {
    Login(Login),
    Logout,
    CallInvite(CallInvite),
    CallAccept(CallAccept),
    CallEnd(CallEnd),
    CallReject(CallReject),
    CallError(CallError),
    WebrtcOffer(WebrtcOffer),
    WebrtcAnswer(WebrtcAnswer),
    WebrtcIceCandidate(WebrtcIceCandidate),
    ListClients,
    ListStations,
    Disconnect,
    Error(Error),
}

impl ClientMessage {
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
            ClientMessage::Login(_) => "Login",
            ClientMessage::Logout => "Logout",
            ClientMessage::CallInvite(_) => "CallInvite",
            ClientMessage::CallAccept(_) => "CallAccept",
            ClientMessage::CallEnd(_) => "CallEnd",
            ClientMessage::CallReject(_) => "CallReject",
            ClientMessage::CallError(_) => "CallError",
            ClientMessage::WebrtcOffer(_) => "WebrtcOffer",
            ClientMessage::WebrtcAnswer(_) => "WebrtcAnswer",
            ClientMessage::WebrtcIceCandidate(_) => "WebrtcIceCandidate",
            ClientMessage::ListClients => "ListClients",
            ClientMessage::ListStations => "ListStations",
            ClientMessage::Disconnect => "Disconnect",
            ClientMessage::Error(_) => "Error",
        }
    }
}
