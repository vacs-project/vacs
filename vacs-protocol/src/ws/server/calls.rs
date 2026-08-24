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
    ///
    /// Invite authorization: once the call is a conference, only the leader
    /// may invite; in addition, while any target is still ringing, only the
    /// client that opened the currently ringing invite batch may add to it
    /// (for the first batch that is the original caller). With nothing
    /// ringing and no leader yet, any participant may invite, and whoever
    /// grows the call into a conference becomes its leader. Only the leader
    /// may drop joined participants. Leadership never transfers: when the
    /// leader leaves or disconnects, the whole call ends for everyone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// `joined_participants` under its client id.
    ///
    /// Empty-update semantics depend on the recipient's phase. For a
    /// still-ringing recipient, an empty set together with empty
    /// `joined_participants` is a live state (it may be the only party still
    /// being rung) and its invitation only ends via an explicit
    /// [`CallCancelled`]. For a caller that has not joined the call, the same
    /// empty update is the call-over signal: the caller is listed in neither
    /// half and would otherwise never learn its call ended.
    pub invited_targets: HashSet<CallTarget>,
    pub joined_participants: CallParticipants,
    /// The current conference leader, if the call is a conference. `None` for
    /// regular calls, including a conference that shrank back to two
    /// participants. See [`CallInvitation::conference_leader`] for the
    /// authorization rules leadership implies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Forward compatibility: a reason this protocol version does not know.
    /// The cancellation of the listed `targets` must still be honored.
    #[serde(untagged)]
    Unknown(crate::ws::shared::UnknownReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallCancelled {
    pub call_id: CallId,
    /// The invitations this cancellation affects. Cancellations only ever
    /// concern ringing invitations; joined participants are torn down via
    /// `CallEnd`.
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
