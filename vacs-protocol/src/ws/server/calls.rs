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
    pub invited_participants: CallParticipants,
    pub joined_participants: CallParticipants,
    pub prio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallUpdate {
    pub call_id: CallId,
    pub invited_participants: CallParticipants,
    pub joined_participants: CallParticipants,
}

impl CallUpdate {
    pub fn all_participants(
        &self,
    ) -> std::iter::Chain<
        std::collections::hash_map::Iter<'_, ClientId, CallTarget>,
        std::collections::hash_map::Iter<'_, ClientId, CallTarget>,
    > {
        self.invited_participants
            .iter()
            .chain(self.joined_participants.iter())
    }
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

impl From<CallCancelled> for ServerMessage {
    fn from(value: CallCancelled) -> Self {
        Self::CallCancelled(value)
    }
}
