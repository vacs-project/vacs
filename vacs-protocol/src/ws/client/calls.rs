use std::collections::HashSet;

use crate::ws::client::ClientMessage;
use crate::ws::shared::{CallId, CallTarget};
use crate::{vatsim::ClientId, ws::shared::CallSource};
use serde::{Deserialize, Serialize};

/// Starts a new call or invites further targets into an existing one.
///
/// Both operations use the same message: a `call_id` the server does not know
/// starts a fresh call, while the id of a call the sender participates in
/// grows that call into (or within) a conference, subject to the
/// authorization rules on
/// [`CallInvitation::conference_leader`](crate::ws::server::CallInvitation::conference_leader).
///
/// A successful fresh-call invite is not acknowledged directly: the caller
/// learns of progress through `CallUpdate`/`CallCancelled`/`CallError`
/// messages. A conference-grow invite is followed immediately by a
/// `CallUpdate` to the existing participants, the inviter included; a caller
/// adding targets to its own still-ringing fresh call gets no `CallUpdate`
/// until someone accepts and tracks that batch locally. Failures come back as
/// `CallError`, except a rate-limited invite, which is answered with an
/// `Error` carrying `ErrorReason::RateLimited` and the affected targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallInvite {
    pub call_id: CallId,
    /// The identity the sender places the call as. Its `client_id` must be
    /// the sender's own.
    pub source: CallSource,
    /// The targets to invite. Must not be empty; targets already ringing or
    /// joined in the call are rejected.
    pub targets: HashSet<CallTarget>,
    /// Marks this invite's targets as priority: their invitations ring as
    /// priority calls. The server keeps no per-call priority, so inviting
    /// with `prio` does not change how the call is presented to participants
    /// already in it.
    pub prio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallAccept {
    pub call_id: CallId,
    pub accepting_client_id: ClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallRejectReason {
    Busy,
    /// Forward compatibility: a reason this protocol version does not know.
    /// The rejection itself must still be honored.
    #[serde(untagged)]
    Unknown(crate::ws::shared::UnknownReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallReject {
    pub call_id: CallId,
    pub rejecting_client_id: ClientId,
    pub reason: CallRejectReason,
}

/// Why a target is being dropped from a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallDropReason {
    /// The dropping client acted deliberately: it cancelled an invitation it
    /// sent, or, as conference leader, removed a participant from the call.
    Requested,
    /// The invitation timed out without being answered; only ever cancels a
    /// ringing invitation. A target that answered while the time was expiring
    /// is never removed from the call by it.
    AutoHangup,
}

/// Removes a single target from a call without ending it.
///
/// Applies to a target that is still ringing, which may be dropped by the
/// client that invited it, and to a joined participant, which may only be
/// dropped by the conference leader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallDropTarget {
    pub call_id: CallId,
    pub target: CallTarget,
    pub reason: CallDropReason,
}

impl From<CallInvite> for ClientMessage {
    fn from(value: CallInvite) -> Self {
        Self::CallInvite(value)
    }
}

impl From<CallAccept> for ClientMessage {
    fn from(value: CallAccept) -> Self {
        Self::CallAccept(value)
    }
}

impl From<CallReject> for ClientMessage {
    fn from(value: CallReject) -> Self {
        Self::CallReject(value)
    }
}

impl From<CallDropTarget> for ClientMessage {
    fn from(value: CallDropTarget) -> Self {
        Self::CallDropTarget(value)
    }
}
