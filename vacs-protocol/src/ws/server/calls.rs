use std::collections::HashSet;

use crate::vatsim::ClientId;
use crate::ws::client::CallRejectReason;
use crate::ws::server::ServerMessage;
use crate::ws::shared::{CallErrorReason, CallId, CallParticipants, CallSource, CallTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallInvitation {
    pub call_id: CallId,
    pub source: CallSource,
    pub target: CallTarget,
    pub invited_targets: HashSet<CallTarget>,
    pub joined_participants: CallParticipants,
    pub prio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallAcceptance {
    pub call_id: CallId,
    pub target: CallTarget,
    pub accepting_client_id: ClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallUpdate {
    pub call_id: CallId,
    pub invited_targets: HashSet<CallTarget>,
    pub joined_participants: CallParticipants,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallCancelReason {
    AnsweredElsewhere(ClientId),
    CallerCancelled,
    Disconnected,
    Errored(CallErrorReason),
    Rejected(CallRejectReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallCancelled {
    pub call_id: CallId,
    pub targets: HashSet<CallTarget>,
    pub reason: CallCancelReason,
}

impl CallCancelled {
    pub fn new(call_id: CallId, targets: HashSet<CallTarget>, reason: CallCancelReason) -> Self {
        Self {
            call_id,
            targets,
            reason,
        }
    }
}

impl From<CallInvitation> for ServerMessage {
    fn from(value: CallInvitation) -> Self {
        Self::CallInvitation(value)
    }
}

impl From<CallAcceptance> for ServerMessage {
    fn from(value: CallAcceptance) -> Self {
        Self::CallAcceptance(value)
    }
}

impl From<CallCancelled> for ServerMessage {
    fn from(value: CallCancelled) -> Self {
        Self::CallCancelled(value)
    }
}
