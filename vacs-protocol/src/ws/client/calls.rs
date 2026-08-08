use std::collections::HashSet;

use crate::ws::client::ClientMessage;
use crate::ws::shared::{CallId, CallTarget};
use crate::{vatsim::ClientId, ws::shared::CallSource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallInvite {
    pub call_id: CallId,
    pub source: CallSource,
    pub targets: HashSet<CallTarget>,
    pub prio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallRejectReason {
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallReject {
    pub call_id: CallId,
    pub rejecting_client_id: ClientId,
    pub reason: CallRejectReason,
}

impl From<CallInvite> for ClientMessage {
    fn from(value: CallInvite) -> Self {
        Self::CallInvite(value)
    }
}

impl From<CallReject> for ClientMessage {
    fn from(value: CallReject) -> Self {
        Self::CallReject(value)
    }
}
