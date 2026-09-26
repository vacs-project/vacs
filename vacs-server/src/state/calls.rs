mod manager;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub use manager::*;
use vacs_protocol::ws::server::CallUpdate;

use crate::metrics::CallMetrics;
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
    /// Monotonic join sequence per participant; decides which member of a
    /// dead link is evicted (the later joiner).
    join_order: HashMap<ClientId, u64>,
    next_join_seq: u64,
    /// Dead-link reports keyed by the normalized pair, holding which of the
    /// two endpoints reported so far and when the pair was last reported.
    link_reports: HashMap<(ClientId, ClientId), LinkReport>,
    guard: CallGuard,
}

/// A stale half-report must not let a later re-failure evict on a single
/// fresh report: reports older than this are discarded when the pair is
/// reported again. Genuine confirmations arrive within one retry interval.
const LINK_REPORT_TTL: Duration = Duration::from_secs(90);

#[derive(Debug)]
struct LinkReport {
    reporters: HashSet<ClientId>,
    refreshed_at: Instant,
    /// Start of the current report window; reset when an expired report is
    /// discarded. Measures how long a pair stays half-reported.
    first_reported_at: Instant,
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
            join_order: HashMap::new(),
            next_join_seq: 0,
            link_reports: HashMap::new(),
            guard: CallGuard::new(),
        }
    }

    pub fn record_join(&mut self, client_id: &ClientId) {
        self.join_order
            .insert(client_id.clone(), self.next_join_seq);
        self.next_join_seq += 1;
    }

    /// Forgets everything tied to a leaving participant: its join sequence
    /// and every link report involving it, so half-reported links cannot
    /// outlive an endpoint.
    pub fn forget_participant(&mut self, client_id: &ClientId) {
        self.join_order.remove(client_id);
        self.link_reports
            .retain(|(a, b), _| a != client_id && b != client_id);
    }

    fn link_key(a: &ClientId, b: &ClientId) -> (ClientId, ClientId) {
        if a <= b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        }
    }

    /// Records a dead-link report and returns whether both endpoints of the
    /// pair have now reported it.
    pub fn record_link_report(&mut self, reporter: &ClientId, peer: &ClientId) -> bool {
        self.record_link_report_at(reporter, peer, Instant::now())
    }

    fn record_link_report_at(
        &mut self,
        reporter: &ClientId,
        peer: &ClientId,
        now: Instant,
    ) -> bool {
        let key = Self::link_key(reporter, peer);
        let report = self.link_reports.entry(key).or_insert_with(|| LinkReport {
            reporters: HashSet::new(),
            refreshed_at: now,
            first_reported_at: now,
        });

        if now.duration_since(report.refreshed_at) >= LINK_REPORT_TTL {
            report.reporters.clear();
            report.first_reported_at = now;
            CallMetrics::link_report_expired();
        }
        report.refreshed_at = now;
        let repeat = !report.reporters.insert(reporter.clone());

        let confirmed = report.reporters.contains(peer);
        if confirmed {
            CallMetrics::link_confirmation_delay(
                now.duration_since(report.first_reported_at).as_secs_f64(),
            );
            CallMetrics::link_report("confirmed");
        } else if repeat {
            CallMetrics::link_report("refreshed");
        } else {
            CallMetrics::link_report("recorded");
        }
        confirmed
    }

    /// The member of the pair that joined the call later.
    pub fn later_joiner(&self, a: &ClientId, b: &ClientId) -> ClientId {
        let seq = |id: &ClientId| self.join_order.get(id).copied().unwrap_or(u64::MAX);
        if seq(a) >= seq(b) {
            a.clone()
        } else {
            b.clone()
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
    pub conference_leader: Option<ClientId>,
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

impl UpdateParticipants {
    /// Builds the wire update for one recipient. `invited_targets` never
    /// contains the recipient's own target: while ringing, that identity is
    /// carried by the invitation's `target` field, and once joined it lives in
    /// `joined_participants` under the recipient's client id.
    pub fn for_recipient(&self, recipient_id: &ClientId) -> CallUpdate {
        let own_target = self.invited_participants.get(recipient_id);
        CallUpdate {
            call_id: self.call_id,
            invited_targets: self
                .invited_participants
                .values()
                .filter(|target| Some(*target) != own_target)
                .cloned()
                .collect(),
            joined_participants: self.joined_participants.clone(),
            conference_leader: self.conference_leader.clone(),
        }
    }
}
