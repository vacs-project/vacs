use std::collections::HashSet;

use crate::ws::client::ClientMessage;
use crate::ws::server::ServerMessage;
use crate::ws::shared::CallId;
use crate::{vatsim::ClientId, ws::shared::CallTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorReason {
    MalformedMessage,
    Internal(String),
    PeerConnection,
    UnexpectedMessage(String),
    RateLimited {
        targets: HashSet<CallTarget>,
        retry_after_secs: u64,
    },
    ClientNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub reason: ErrorReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<ClientId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<CallId>,
}

impl Error {
    pub fn new(reason: ErrorReason) -> Self {
        Self {
            reason,
            client_id: None,
            call_id: None,
        }
    }

    pub fn with_client_id(mut self, client_id: ClientId) -> Self {
        self.client_id = Some(client_id);
        self
    }

    pub fn with_call_id(mut self, call_id: CallId) -> Self {
        self.call_id = Some(call_id);
        self
    }
}

impl From<ErrorReason> for Error {
    fn from(reason: ErrorReason) -> Self {
        Self {
            reason,
            client_id: None,
            call_id: None,
        }
    }
}

impl From<Error> for ClientMessage {
    fn from(value: Error) -> Self {
        Self::Error(value)
    }
}

impl From<Error> for ServerMessage {
    fn from(value: Error) -> Self {
        Self::Error(value)
    }
}
