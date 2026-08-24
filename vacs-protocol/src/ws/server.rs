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
    /// Catch-all for message types unknown to this protocol version.
    ///
    /// Never sent by a server; any unrecognized `type` tag deserializes to this variant so
    /// clients can skip additive messages from newer servers instead of failing the
    /// connection. Malformed payloads of known message types still fail deserialization.
    #[serde(other)]
    Unknown,
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
            ServerMessage::CallInvitation(_) => "CallInvitation",
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
            ServerMessage::Unknown => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_message_type_deserializes_to_unknown() {
        let msg = ServerMessage::deserialize(r#"{"type":"someFutureMessage","value":42}"#)
            .expect("unknown message types must deserialize");
        assert_eq!(msg, ServerMessage::Unknown);
    }

    #[test]
    fn malformed_known_message_type_still_fails() {
        assert!(ServerMessage::deserialize(r#"{"type":"callEnd"}"#).is_err());
    }

    #[test]
    fn missing_type_tag_fails() {
        assert!(ServerMessage::deserialize(r#"{"value":42}"#).is_err());
    }

    /// A newer server adding a reason variant must not make the enclosing
    /// message undeserializable; the reason falls back to `Unknown`.
    #[test]
    fn unknown_reason_variants_deserialize_to_unknown() {
        let msg =
            ServerMessage::deserialize(r#"{"type":"loginFailure","reason":"someFutureReason"}"#)
                .expect("unknown unit reason must fall back");
        let ServerMessage::LoginFailure(failure) = msg else {
            panic!("expected a login failure");
        };
        assert!(matches!(
            failure.reason,
            crate::ws::server::LoginFailureReason::Unknown(_)
        ));

        let msg = ServerMessage::deserialize(
            r#"{"type":"callCancelled","callId":"00000000-0000-0000-0000-000000000000","targets":[],"reason":{"someFutureReason":{"detail":1}}}"#,
        )
        .expect("unknown structured reason must fall back");
        let ServerMessage::CallCancelled(cancelled) = msg else {
            panic!("expected a call cancelled");
        };
        assert!(matches!(
            cancelled.reason,
            crate::ws::server::CallCancelReason::Unknown(_)
        ));

        let msg = ServerMessage::deserialize(
            r#"{"type":"callError","callId":"00000000-0000-0000-0000-000000000000","reason":"someFutureReason"}"#,
        )
        .expect("unknown call error reason must fall back");
        let ServerMessage::CallError(error) = msg else {
            panic!("expected a call error");
        };
        assert!(matches!(
            error.reason,
            crate::ws::shared::CallErrorReason::Unknown(_)
        ));
    }

    /// Known reasons must keep deserializing to their real variants, not be
    /// swallowed by the untagged fallback.
    #[test]
    fn known_reason_variants_still_deserialize() {
        let msg = ServerMessage::deserialize(r#"{"type":"loginFailure","reason":"unauthorized"}"#)
            .expect("known reason must deserialize");
        let ServerMessage::LoginFailure(failure) = msg else {
            panic!("expected a login failure");
        };
        assert_eq!(
            failure.reason,
            crate::ws::server::LoginFailureReason::Unauthorized
        );
    }
}
