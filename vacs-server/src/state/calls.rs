mod manager;
use std::collections::{HashMap, HashSet};

pub use manager::*;
use vacs_protocol::ws::server::CallUpdate;

use crate::metrics::guards::{CallAttemptGuard, CallAttemptOutcome, CallGuard};
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::shared::{CallId, CallParticipants, CallSource, CallTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingingTarget {
    pub call_id: CallId,
    pub caller_id: ClientId,
    pub source: CallSource,
    pub target: CallTarget,
    pub notified_clients: HashSet<ClientId>,
}

#[derive(Debug)]
pub struct RingingTargetEntry {
    source: CallSource,
    notified_clients: HashSet<ClientId>,
    rejected_clients: HashSet<ClientId>,
    errored_clients: HashSet<ClientId>,
    guard: CallAttemptGuard,
}

impl RingingTargetEntry {
    pub fn new(source: CallSource, notified_clients: HashSet<ClientId>) -> Self {
        Self {
            source,
            notified_clients,
            rejected_clients: HashSet::new(),
            errored_clients: HashSet::new(),
            guard: CallAttemptGuard::new(),
        }
    }

    pub fn has_notified_client(&self, client_id: &ClientId) -> bool {
        self.notified_clients.contains(client_id)
    }

    pub fn has_failed_client(&self, client_id: &ClientId) -> bool {
        self.rejected_clients.contains(client_id) || self.errored_clients.contains(client_id)
    }

    pub fn mark_rejected(&mut self, client_id: &ClientId) {
        if !self.notified_clients.remove(client_id) {
            return;
        }
        self.rejected_clients.insert(client_id.clone());
    }

    pub fn mark_errored(&mut self, client_id: &ClientId) {
        if !self.notified_clients.remove(client_id) {
            return;
        }
        self.errored_clients.insert(client_id.clone());
    }

    pub fn set_outcome(&mut self, outcome: CallAttemptOutcome) {
        self.guard.set_outcome(outcome);
    }

    pub fn complete(
        &mut self,
        outcome: CallAttemptOutcome,
        call_id: &CallId,
        caller_id: &ClientId,
        source: CallSource,
        target: &CallTarget,
    ) -> RingingTarget {
        self.set_outcome(outcome);
        RingingTarget {
            call_id: *call_id,
            caller_id: caller_id.clone(),
            source,
            target: target.clone(),
            notified_clients: self.notified_clients.clone(),
        }
    }

    fn all_rejected_or_errored(&self) -> bool {
        self.notified_clients.is_empty()
    }
}

#[derive(Debug)]
struct RingingCallEntry {
    call_id: CallId,
    caller_id: ClientId,
    targets: HashMap<CallTarget, RingingTargetEntry>,
}

#[derive(Debug, Clone)]
pub struct ActiveCall {
    pub call_id: CallId,
    pub conference_leader: Option<ClientId>,
    pub participants: CallParticipants,
}

#[derive(Debug)]
struct ActiveCallEntry {
    call_id: CallId,
    conference_leader: Option<ClientId>,
    participants: CallParticipants,
    guard: CallGuard,
}

impl RingingCallEntry {
    pub fn new(
        call_id: CallId,
        caller_id: ClientId,
        source: CallSource,
        target: CallTarget,
        notified_clients: HashSet<ClientId>,
    ) -> Self {
        Self {
            call_id,
            caller_id,
            targets: HashMap::from([(target, RingingTargetEntry::new(source, notified_clients))]),
        }
    }

    pub fn add_target(
        &mut self,
        source: CallSource,
        target: CallTarget,
        notified_clients: HashSet<ClientId>,
    ) {
        self.targets
            .insert(target, RingingTargetEntry::new(source, notified_clients));
    }

    pub fn complete_all_targets(self, outcome: CallAttemptOutcome) -> Vec<RingingTarget> {
        self.targets
            .into_iter()
            .map(|(target, mut entry)| {
                entry.complete(
                    outcome.clone(),
                    &self.call_id,
                    &self.caller_id,
                    entry.source.clone(),
                    &target,
                )
            })
            .collect()
    }

    pub fn all_targets_failed(&self) -> bool {
        self.targets
            .values()
            .all(|entry| entry.all_rejected_or_errored())
    }

    pub fn invited_participants(&self) -> CallParticipants {
        self.targets
            .iter()
            .flat_map(|(target, ringing_target_entry)| {
                ringing_target_entry
                    .notified_clients
                    .iter()
                    .map(|client| (client.clone(), target.clone()))
            })
            .collect()
    }
}

impl ActiveCall {
    pub fn participants_without_self(&self, client_id: &ClientId) -> CallParticipants {
        self.participants
            .iter()
            .filter_map(|(participant_client_id, as_station_id)| {
                if participant_client_id != client_id {
                    Some((participant_client_id.clone(), as_station_id.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn involves(&self, client_id: &ClientId) -> bool {
        self.participants.contains_key(client_id)
    }
}

impl ActiveCallEntry {
    pub fn new(
        call_id: CallId,
        conference_leader: Option<ClientId>,
        participants: CallParticipants,
    ) -> Self {
        Self {
            call_id,
            conference_leader,
            participants,
            guard: CallGuard::new(),
        }
    }

    pub fn participants_without_self(&self, client_id: &ClientId) -> CallParticipants {
        self.participants
            .iter()
            .filter_map(|(participant_client_id, as_station_id)| {
                if participant_client_id != client_id {
                    Some((participant_client_id.clone(), as_station_id.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn involves(&self, client_id: &ClientId) -> bool {
        self.participants.contains_key(client_id)
    }
}

impl From<ActiveCallEntry> for ActiveCall {
    fn from(entry: ActiveCallEntry) -> Self {
        Self {
            call_id: entry.call_id,
            conference_leader: entry.conference_leader,
            participants: entry.participants,
        }
    }
}

impl From<&ActiveCallEntry> for ActiveCall {
    fn from(entry: &ActiveCallEntry) -> Self {
        Self {
            call_id: entry.call_id,
            conference_leader: entry.conference_leader.clone(),
            participants: entry.participants.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCallAction {
    CancelRingingTarget(RingingTarget),
    DropParticipant(CallId, ClientId),
    UpdateParticipants(UpdateParticipants),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateParticipants {
    pub call_id: CallId,
    pub invited_participants: CallParticipants,
    pub joined_participants: CallParticipants,
}

impl UpdateParticipants {
    pub fn all_participants(&self) -> impl Iterator<Item = (&ClientId, &CallTarget)> {
        self.invited_participants
            .iter()
            .chain(self.joined_participants.iter())
    }

    pub fn all_participants_without_self(
        &self,
        client_id: ClientId,
    ) -> impl Iterator<Item = (&ClientId, &CallTarget)> {
        self.all_participants()
            .filter(move |(id, _)| **id != client_id)
    }
}

impl From<&UpdateParticipants> for CallUpdate {
    fn from(value: &UpdateParticipants) -> Self {
        Self {
            call_id: value.call_id,
            invited_targets: value.invited_participants.values().cloned().collect(),
            joined_participants: value.joined_participants.clone(),
        }
    }
}
