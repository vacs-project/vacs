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
    /// The party that placed the call.
    pub source: CallSource,
    /// The target this recipient is being invited as. This is the recipient's
    /// own identity in the call and never appears in `invited_targets`.
    pub target: CallTarget,
    /// The other targets still being invited into the call. Never contains the
    /// recipient's own `target`.
    pub invited_targets: HashSet<CallTarget>,
    pub joined_participants: CallParticipants,
    /// The current conference leader, if the call already is a conference.
    /// Only the leader may invite further targets into a conference or drop
    /// joined participants from it.
    #[serde(default)]
    pub conference_leader: Option<ClientId>,
    pub prio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallUpdate {
    pub call_id: CallId,
    /// The targets still being invited into the call. Never contains the
    /// recipient's own target: a still-ringing recipient keeps its identity
    /// from the invitation's `target`, a joined recipient finds itself in
    /// `joined_participants` under its client id. An empty set together with
    /// empty `joined_participants` does NOT mean the call ended; termination
    /// is always signalled explicitly.
    pub invited_targets: HashSet<CallTarget>,
    pub joined_participants: CallParticipants,
    /// The current conference leader, if the call is a conference. `None` for
    /// regular calls, including a conference that shrank back to two
    /// participants.
    #[serde(default)]
    pub conference_leader: Option<ClientId>,
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
