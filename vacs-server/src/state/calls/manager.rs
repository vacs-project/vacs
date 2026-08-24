use crate::metrics::ErrorMetrics;
use crate::metrics::guards::CallAttemptOutcome;
use crate::state::AppState;
use crate::state::calls::{
    ActiveCall, ActiveCallEntry, RingingCallEntry, RingingTarget, RingingTargetEntry,
    UpdateCallAction, UpdateParticipants,
};
use parking_lot::RwLock;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use tracing::instrument;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::CallDropReason;
use vacs_protocol::ws::server::CallCancelReason;
use vacs_protocol::ws::server::{self, ServerMessage};
use vacs_protocol::ws::shared::{
    CallEnd, CallErrorReason, CallId, CallParticipants, CallSource, CallTarget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartCallError {
    CallerBusy,
    AlreadyParticipant,
    NotConferenceLeader,
    NotParticipant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropTargetOutcome {
    CallNotFound,
    NotPermitted,
    /// The drop is obsolete as the target dropped by an auto-hangup is
    /// already an active participant in the call and will thus not be
    /// removed automatically. The dropping client receives the current
    /// call state as an `CallUpdate`, just to make sure its local state
    /// is correct.
    Obsolete(UpdateParticipants),
    RingingTargetCancelled(RingingTarget, UpdateParticipants),
    ParticipantDropped(ClientId, UpdateParticipants),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTerminationOutcome {
    CallNotFound,
    ClientNotNotified,
    Continued,
    TargetFailed(Vec<RingingTarget>, UpdateParticipants),
    Changed(Vec<UpdateCallAction>),
}

#[derive(Debug)]
pub enum AcceptCallOutcome {
    Accepted {
        target: Box<RingingTarget>,
        update: UpdateParticipants,
    },
    /// The accepting client already participates in another active call.
    AcceptorBusy,
    NotFound,
}

/// Lock order: when holding more than one lock, acquire them in field order —
/// `ringing_calls` → `active_calls` → `client_incoming_calls` / `client_active_calls`.
/// Never acquire a lock while holding one that comes later in this order.
pub struct CallManager {
    ringing_calls: RwLock<HashMap<CallId, RingingCallEntry>>,
    active_calls: RwLock<HashMap<CallId, ActiveCallEntry>>,
    client_incoming_calls: RwLock<HashMap<ClientId, HashMap<CallId, CallTarget>>>,
    client_active_calls: RwLock<HashMap<ClientId, CallId>>,
    max_conf_size: usize,
}

impl Default for CallManager {
    fn default() -> Self {
        CallManager::new(8)
    }
}

impl std::fmt::Debug for CallManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallStateManager")
            .field("ringing_calls", &self.ringing_calls.read().len())
            .field("active_calls", &self.active_calls.read().len())
            .finish()
    }
}

impl CallManager {
    pub fn new(max_conf_size: usize) -> Self {
        Self {
            ringing_calls: RwLock::new(HashMap::new()),
            active_calls: RwLock::new(HashMap::new()),
            client_incoming_calls: RwLock::new(HashMap::new()),
            client_active_calls: RwLock::new(HashMap::new()),
            max_conf_size,
        }
    }

    pub fn has_active_call(&self, call_id: &CallId, client_id: &ClientId) -> bool {
        self.active_calls
            .read()
            .get(call_id)
            .is_some_and(|active| active.involves(client_id))
    }

    pub fn active_call(&self, call_id: &CallId) -> Option<ActiveCall> {
        self.active_calls.read().get(call_id).map(Into::into)
    }

    pub fn invite_exceeds_max_conf_size(
        &self,
        call_id: &CallId,
        caller_id: &ClientId,
        targets: &HashSet<CallTarget>,
    ) -> bool {
        let ringing_calls = self.ringing_calls.read();
        let ringing = ringing_calls.get(call_id);
        let ringing_len = ringing.map(|call| call.targets.len()).unwrap_or_default();

        let active_calls = self.active_calls.read();
        let active = active_calls.get(call_id);
        let joined_len = active
            .map(|call| call.participants.len())
            .unwrap_or_default();
        let caller_joined = active.is_some_and(|call| call.participants.contains_key(caller_id));

        // Only targets not already ringing or joined can grow the call.
        let new_targets = targets
            .iter()
            .filter(|target| {
                !ringing.is_some_and(|call| call.targets.contains_key(*target))
                    && !active.is_some_and(|call| {
                        call.participants.values().any(|joined| joined == *target)
                    })
            })
            .count();

        joined_len + ringing_len + new_targets + usize::from(!caller_joined) > self.max_conf_size
    }

    #[instrument(level = "trace", skip(self))]
    pub fn attempt_call(
        &self,
        call_id: &CallId,
        caller_id: &ClientId,
        source: &CallSource,
        target: &CallTarget,
        notified_clients: &HashSet<ClientId>,
    ) -> Result<(CallParticipants, CallParticipants), StartCallError> /* (invited, joined) */ {
        self.validate_call_attempt(call_id, caller_id, target, notified_clients)?;

        let ringing_participants: CallParticipants =
            match self.ringing_calls.write().entry(*call_id) {
                Entry::Occupied(mut e) => {
                    let ringing_call_entry = e.get_mut();

                    ringing_call_entry.add_target(
                        source.clone(),
                        target.clone(),
                        notified_clients.clone(),
                    );

                    ringing_call_entry.invited_participants()
                }
                Entry::Vacant(e) => {
                    e.insert_entry(RingingCallEntry::new(
                        *call_id,
                        caller_id.clone(),
                        source.clone(),
                        target.clone(),
                        notified_clients.clone(),
                    ));
                    notified_clients
                        .iter()
                        .map(|client| (client.clone(), target.clone()))
                        .collect()
                }
            };
        self.client_active_calls
            .write()
            .insert(caller_id.clone(), *call_id);

        let mut client_incoming_calls = self.client_incoming_calls.write();
        for client_id in notified_clients {
            if let Some(old_target) = client_incoming_calls
                .entry(client_id.clone())
                .or_default()
                .insert(*call_id, target.clone())
            {
                tracing::error!(
                    ?client_id,
                    ?old_target,
                    "Callee already has an incoming call for this call id, but with a different target"
                );
            }
        }

        let joined_participants = self
            .active_calls
            .read()
            .get(call_id)
            .map(|active_call| active_call.participants.clone())
            .unwrap_or_default();

        Ok((ringing_participants, joined_participants))
    }

    fn mark_client_within_ringing_calls<F>(
        &self,
        call_id: &CallId,
        client_id: &ClientId,
        mark_fn: F,
        outcome: CallAttemptOutcome,
    ) -> CallTerminationOutcome
    where
        F: Fn(&mut RingingTargetEntry, &ClientId),
    {
        self.remove_client_incoming_call(call_id, client_id);

        let mut ringing_calls = self.ringing_calls.write();
        match ringing_calls.entry(*call_id) {
            Entry::Occupied(mut entry) => {
                if entry.get().caller_id == *client_id {
                    return CallTerminationOutcome::ClientNotNotified;
                }

                let mut marked_once = false;
                let mut terminated_targets = Vec::new();

                let ringing_call_entry = entry.get_mut();
                ringing_call_entry
                    .targets
                    .retain(|target, ringing_target_entry| {
                        if !ringing_target_entry.has_notified_client(client_id)
                            || ringing_target_entry.has_failed_client(client_id)
                        {
                            return true;
                        }

                        mark_fn(ringing_target_entry, client_id);
                        marked_once = true;

                        if ringing_target_entry.all_rejected_or_errored() {
                            terminated_targets.push(ringing_target_entry.complete(
                                outcome.clone(),
                                call_id,
                                &ringing_call_entry.caller_id,
                                ringing_target_entry.source.clone(),
                                target,
                            ));
                            false
                        } else {
                            true
                        }
                    });

                if !marked_once {
                    return CallTerminationOutcome::ClientNotNotified;
                }

                let invited_participants = ringing_call_entry.invited_participants();

                if ringing_call_entry.targets.is_empty() {
                    let caller_id = ringing_call_entry.caller_id.clone();

                    entry.remove();
                    drop(ringing_calls);

                    if !self
                        .active_calls
                        .read()
                        .values()
                        .any(|active_call_entry| active_call_entry.involves(&caller_id))
                    {
                        self.client_active_calls.write().remove(&caller_id);
                    }
                } else {
                    drop(ringing_calls);
                }

                if !terminated_targets.is_empty() {
                    if terminated_targets.len() > 1 {
                        tracing::error!("Rejecting call has multiple targets within the same call");
                    }

                    let joined_participants = self
                        .active_calls
                        .read()
                        .get(call_id)
                        .map(|active_call| active_call.participants.clone())
                        .unwrap_or_default();

                    let update = UpdateParticipants {
                        call_id: *call_id,
                        invited_participants,
                        joined_participants,
                    };

                    return CallTerminationOutcome::TargetFailed(terminated_targets, update);
                }

                CallTerminationOutcome::Continued
            }
            Entry::Vacant(_) => CallTerminationOutcome::CallNotFound,
        }
    }

    pub fn reject_call(
        &self,
        call_id: &CallId,
        rejecting_client_id: &ClientId,
    ) -> CallTerminationOutcome {
        self.mark_client_within_ringing_calls(
            call_id,
            rejecting_client_id,
            RingingTargetEntry::mark_rejected,
            CallAttemptOutcome::Rejected,
        )
    }

    pub fn call_error(
        &self,
        call_id: &CallId,
        erroring_client_id: &ClientId,
    ) -> CallTerminationOutcome {
        let outcome = self.mark_client_within_ringing_calls(
            call_id,
            erroring_client_id,
            RingingTargetEntry::mark_errored,
            CallAttemptOutcome::Error(CallErrorReason::CallFailure),
        );

        match outcome {
            CallTerminationOutcome::Continued | CallTerminationOutcome::TargetFailed(_, _) => {
                outcome
            }
            _ => match self.end_call(call_id, erroring_client_id) {
                None => CallTerminationOutcome::CallNotFound,
                Some(actions) => CallTerminationOutcome::Changed(actions),
            },
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn accept_call(
        &self,
        call_id: &CallId,
        accepting_client_id: &ClientId,
    ) -> AcceptCallOutcome {
        // Held across the whole accept: every teardown path acquires
        // `ringing_calls` before `active_calls`, so this guard keeps a
        // concurrent teardown from interleaving between the ringing and
        // active updates below.
        let mut ringing_calls = self.ringing_calls.write();

        // Under the same guard, so concurrent accepts serialize.
        if self
            .client_active_calls
            .read()
            .contains_key(accepting_client_id)
        {
            return AcceptCallOutcome::AcceptorBusy;
        }

        let accepted = {
            match ringing_calls.entry(*call_id) {
                Entry::Occupied(mut entry) => {
                    let ringing_call = entry.get_mut();

                    let mut ringing_target: Option<RingingTarget> = None;

                    ringing_call.targets.retain(|target, ringing_target_entry| {
                        if ringing_target_entry.has_notified_client(accepting_client_id) {
                            let mut new_ringing_target = ringing_target_entry.complete(
                                CallAttemptOutcome::Accepted,
                                call_id,
                                &ringing_call.caller_id,
                                ringing_target_entry.source.clone(),
                                target,
                            );

                            if let Some(old) = ringing_target.take() {
                                tracing::error!(?old.target, ?target, "Accepting client id was notified in multiple ringing target entries");
                                new_ringing_target
                                    .notified_clients
                                    .extend(old.notified_clients);

                                if new_ringing_target.target > old.target {
                                    new_ringing_target.target = old.target;
                                }
                            }
                            ringing_target = Some(new_ringing_target);

                            return false;
                        }
                        true
                    });

                    let invited_participants = ringing_call.invited_participants();

                    if ringing_call.targets.is_empty() {
                        entry.remove();
                    }

                    ringing_target.map(|ringing_target| (ringing_target, invited_participants))
                }
                _ => None,
            }
        };
        let Some((ringing_target, invited_participants)) = accepted else {
            return AcceptCallOutcome::NotFound;
        };

        {
            let mut client_incoming_calls = self.client_incoming_calls.write();
            for callee_id in &ringing_target.notified_clients {
                if let Some(calls) = client_incoming_calls.get_mut(callee_id) {
                    calls.remove(call_id);
                    if calls.is_empty() {
                        client_incoming_calls.remove(callee_id);
                    }
                }
            }
        }

        let mut active_calls = self.active_calls.write();
        let joined_participants: CallParticipants = match active_calls.entry(*call_id) {
            Entry::Occupied(mut entry) => {
                let active_call = entry.get_mut();

                active_call
                    .participants
                    .insert(accepting_client_id.clone(), ringing_target.target.clone());

                active_call
                    .guard
                    .record_participants(active_call.participants.len());

                if active_call.conference_leader.is_none() {
                    if active_call.participants.len() > 3 {
                        tracing::warn!(
                            "Call was already a conference without a leader before client joined, setting leader"
                        );
                    }

                    // Deliberate: leadership goes to whoever invited the participant that
                    // turned the call into a conference, not to the original caller
                    active_call.conference_leader = Some(ringing_target.caller_id.clone());
                }

                let participants = active_call.participants.clone();

                drop(active_calls);

                self.client_active_calls
                    .write()
                    .insert(accepting_client_id.clone(), *call_id);

                participants
            }
            Entry::Vacant(entry) => {
                let participants = HashMap::from([
                    (accepting_client_id.clone(), ringing_target.target.clone()),
                    (
                        ringing_target.caller_id.clone(),
                        ringing_target.source.clone().into(),
                    ),
                ]);

                let active = ActiveCallEntry::new(*call_id, None, participants.clone());

                entry.insert(active);

                drop(active_calls);

                // attempt_call already registered the caller; re-inserting it
                // would resurrect an entry a concurrent teardown just removed.
                self.client_active_calls
                    .write()
                    .insert(accepting_client_id.clone(), *call_id);

                participants
            }
        };

        let update = UpdateParticipants {
            call_id: *call_id,
            invited_participants,
            joined_participants,
        };

        AcceptCallOutcome::Accepted {
            target: Box::new(ringing_target),
            update,
        }
    }

    /// Removes a single target from a call without ending it: a ringing target
    /// may be dropped by the client that invited it, a joined participant only
    /// by the conference leader.
    #[instrument(level = "trace", skip(self))]
    pub fn drop_target(
        &self,
        call_id: &CallId,
        dropping_client_id: &ClientId,
        target: &CallTarget,
        reason: CallDropReason,
    ) -> DropTargetOutcome {
        if let Some(outcome) = self.drop_ringing_target(call_id, dropping_client_id, target, reason)
        {
            return outcome;
        }

        self.drop_joined_participant(call_id, dropping_client_id, target, reason)
    }

    /// Returns `None` when the target does not ring within this call, so that
    /// the drop can be retried against the joined participants.
    fn drop_ringing_target(
        &self,
        call_id: &CallId,
        dropping_client_id: &ClientId,
        target: &CallTarget,
        reason: CallDropReason,
    ) -> Option<DropTargetOutcome> {
        let (ringing_target, invited_participants, caller_id, ringing_ended) = {
            let mut ringing_calls = self.ringing_calls.write();

            let ringing_call = ringing_calls.get_mut(call_id)?;
            if ringing_call.targets.get(target)?.source.client_id != *dropping_client_id {
                tracing::debug!("Dropping client did not invite this target");
                return Some(DropTargetOutcome::NotPermitted);
            }

            let caller_id = ringing_call.caller_id.clone();
            let mut ringing_target_entry = ringing_call.targets.remove(target)?;
            let source = ringing_target_entry.source.clone();
            let ringing_target =
                ringing_target_entry.complete(reason.into(), call_id, &caller_id, source, target);

            let invited_participants = ringing_call.invited_participants();
            let ringing_ended = ringing_call.targets.is_empty();
            if ringing_ended {
                ringing_calls.remove(call_id);
            }

            (
                ringing_target,
                invited_participants,
                caller_id,
                ringing_ended,
            )
        };

        for callee_id in &ringing_target.notified_clients {
            self.remove_client_incoming_call(call_id, callee_id);
        }

        let joined_participants = self
            .active_calls
            .read()
            .get(call_id)
            .map(|active_call| active_call.participants.clone())
            .unwrap_or_default();

        if ringing_ended && !joined_participants.contains_key(&caller_id) {
            let mut client_active_calls = self.client_active_calls.write();
            if client_active_calls
                .get(&caller_id)
                .is_some_and(|id| id == call_id)
            {
                client_active_calls.remove(&caller_id);
            }
        }

        Some(DropTargetOutcome::RingingTargetCancelled(
            ringing_target,
            UpdateParticipants {
                call_id: *call_id,
                invited_participants,
                joined_participants,
            },
        ))
    }

    fn drop_joined_participant(
        &self,
        call_id: &CallId,
        dropping_client_id: &ClientId,
        target: &CallTarget,
        reason: CallDropReason,
    ) -> DropTargetOutcome {
        // Decided while the active call is locked. The resulting snapshot has to be
        // taken afterwards, because reading the ringing calls while holding the active
        // calls lock inverts the lock order.
        let dropped_client_id = {
            let mut active_calls = self.active_calls.write();

            match active_calls.get_mut(call_id) {
                None => None,
                Some(active_call) if !active_call.involves(dropping_client_id) => {
                    tracing::debug!("Dropping client does not participate in this call");
                    return DropTargetOutcome::NotPermitted;
                }
                Some(active_call) => {
                    let dropped_client_id = active_call.participants.iter().find_map(
                        |(client_id, participating_target)| {
                            if participating_target == target {
                                Some(client_id.clone())
                            } else {
                                None
                            }
                        },
                    );

                    match dropped_client_id {
                        None => None,
                        // A target that answered while the invitation was timing out stays in
                        // the call: an expired timer must never remove a joined participant.
                        Some(_) if reason == CallDropReason::AutoHangup => None,
                        Some(dropped_client_id) if dropped_client_id == *dropping_client_id => {
                            tracing::debug!("Dropping client tried to drop itself");
                            return DropTargetOutcome::NotPermitted;
                        }
                        Some(_) if active_call.participants.len() <= 2 => {
                            tracing::debug!(
                                ?dropped_client_id,
                                "Call is not a conference, nothing to drop from"
                            );
                            return DropTargetOutcome::NotPermitted;
                        }
                        Some(_)
                            if active_call.conference_leader.as_ref()
                                != Some(dropping_client_id) =>
                        {
                            tracing::debug!(
                                ?dropped_client_id,
                                "Dropping client is not conference leader"
                            );
                            return DropTargetOutcome::NotPermitted;
                        }
                        Some(dropped_client_id) => {
                            active_call.participants.remove(&dropped_client_id);

                            if active_call.participants.len() <= 2 {
                                tracing::debug!(
                                    ?dropped_client_id,
                                    "Dropping participant downgraded conference to regular call"
                                );
                                active_call.conference_leader = None;
                            }

                            Some(dropped_client_id)
                        }
                    }
                }
            }
        };

        let invited_participants = self
            .ringing_calls
            .read()
            .get(call_id)
            .map(RingingCallEntry::invited_participants)
            .unwrap_or_default();

        let joined_participants = self
            .active_calls
            .read()
            .get(call_id)
            .map(|active_call| active_call.participants.clone())
            .unwrap_or_default();

        let update = UpdateParticipants {
            call_id: *call_id,
            invited_participants,
            joined_participants,
        };

        let Some(dropped_client_id) = dropped_client_id else {
            return if update.invited_participants.is_empty()
                && update.joined_participants.is_empty()
            {
                tracing::warn!(
                    "Call with dropped target has no invited or joined participants anymore"
                );
                DropTargetOutcome::CallNotFound
            } else {
                DropTargetOutcome::Obsolete(update)
            };
        };

        {
            let mut client_active_calls = self.client_active_calls.write();
            if client_active_calls
                .get(&dropped_client_id)
                .is_some_and(|id| id == call_id)
            {
                client_active_calls.remove(&dropped_client_id);
            }
        }

        DropTargetOutcome::ParticipantDropped(dropped_client_id, update)
    }

    pub fn end_call(
        &self,
        call_id: &CallId,
        ending_client_id: &ClientId,
    ) -> Option<Vec<UpdateCallAction>> {
        let ringing = {
            let mut ringing_calls = self.ringing_calls.write();
            match ringing_calls.entry(*call_id) {
                Entry::Occupied(entry) if entry.get().caller_id == *ending_client_id => {
                    Some(entry.remove())
                }
                _ => None,
            }
        };

        let ringing_actions: Option<Vec<UpdateCallAction>> = ringing.map(|ringing| {
            self.cleanup_client_incoming_calls(&ringing);
            self.client_active_calls.write().remove(ending_client_id);
            ringing
                .complete_all_targets(CallAttemptOutcome::Cancelled)
                .into_iter()
                .map(UpdateCallAction::CancelRingingTarget)
                .collect()
        });

        let mut active_calls = self.active_calls.write();
        let active_actions = match active_calls.entry(*call_id) {
            Entry::Occupied(mut entry) if entry.get().involves(ending_client_id) => {
                let active_call = entry.get_mut();

                let participants_without_self =
                    active_call.participants_without_self(ending_client_id);

                if participants_without_self.is_empty() {
                    tracing::error!(
                        "Ending client has active call, which has no other participant than self"
                    );
                    ErrorMetrics::peer_not_found();

                    entry.remove();
                    drop(active_calls);

                    let actions =
                        self.cancel_pending_invitations(call_id, CallAttemptOutcome::Cancelled);

                    {
                        let mut client_active_calls = self.client_active_calls.write();
                        if client_active_calls
                            .get(ending_client_id)
                            .is_some_and(|c| c == call_id)
                        {
                            client_active_calls.remove(ending_client_id);
                        }
                    }

                    Some(actions)
                } else if active_call.conference_leader.as_ref() == Some(ending_client_id)
                    || participants_without_self.len() <= 1
                {
                    entry.remove();
                    drop(active_calls);

                    {
                        let mut client_active_calls = self.client_active_calls.write();
                        for participant_id in participants_without_self.keys() {
                            if client_active_calls
                                .get(participant_id)
                                .is_some_and(|c| c == call_id)
                            {
                                client_active_calls.remove(participant_id);
                            }
                        }

                        if client_active_calls
                            .get(ending_client_id)
                            .is_some_and(|c| c == call_id)
                        {
                            client_active_calls.remove(ending_client_id);
                        }
                    }

                    let mut actions: Vec<UpdateCallAction> = participants_without_self
                        .into_keys()
                        .map(|participant_id| {
                            UpdateCallAction::DropParticipant(*call_id, participant_id)
                        })
                        .collect();
                    actions.extend(
                        self.cancel_pending_invitations(call_id, CallAttemptOutcome::Cancelled),
                    );

                    Some(actions)
                } else {
                    if active_call.participants.remove(ending_client_id).is_none() {
                        tracing::error!(
                            "Tried to remove ending participant from active call, but no entry found; sending update anyway..."
                        );
                    }

                    if active_call.participants.len() <= 2 {
                        active_call.conference_leader = None;
                    }

                    let joined_participants = active_call.participants.clone();

                    drop(active_calls);

                    {
                        let mut client_active_calls = self.client_active_calls.write();
                        if client_active_calls
                            .get(ending_client_id)
                            .is_some_and(|c| c == call_id)
                        {
                            client_active_calls.remove(ending_client_id);
                        }
                    }

                    let invited_participants = self
                        .ringing_calls
                        .read()
                        .get(call_id)
                        .map(|ringing_call| ringing_call.invited_participants())
                        .unwrap_or_default();

                    let update = UpdateParticipants {
                        call_id: *call_id,
                        invited_participants,
                        joined_participants,
                    };

                    Some(Vec::from([UpdateCallAction::UpdateParticipants(update)]))
                }
            }
            _ => None,
        };

        match (active_actions, ringing_actions) {
            (Some(mut active), Some(ringing)) => {
                active.extend(ringing);
                Some(active)
            }
            (active, ringing) => active.or(ringing),
        }
    }

    #[instrument(level = "trace", skip(self, state))]
    pub async fn cleanup_client_calls(&self, state: &AppState, client_id: &ClientId) {
        tracing::trace!("Cleaning up client calls");

        let mut actions = Vec::new();

        let active_or_outgoing_call_id = self.client_active_calls.write().remove(client_id);
        if let Some(active_or_ringing_call_id) = active_or_outgoing_call_id {
            let mut has_active_or_outgoing_call = false;

            {
                let mut ringing_calls = self.ringing_calls.write();
                match ringing_calls.entry(active_or_ringing_call_id) {
                    Entry::Occupied(entry) if entry.get().caller_id == *client_id => {
                        has_active_or_outgoing_call = true;

                        let ringing_call = entry.remove();
                        drop(ringing_calls);

                        {
                            let mut client_incoming_calls = self.client_incoming_calls.write();
                            for callee_id in ringing_call
                                .targets
                                .values()
                                .flat_map(|e| &e.notified_clients)
                            {
                                if let Some(calls) = client_incoming_calls.get_mut(callee_id) {
                                    calls.remove(&active_or_ringing_call_id);
                                    if calls.is_empty() {
                                        client_incoming_calls.remove(callee_id);
                                    }
                                }
                            }
                        }

                        tracing::trace!(
                            ?active_or_ringing_call_id,
                            "Aborting outgoing ringing call"
                        );
                        actions.extend(
                            ringing_call
                                .complete_all_targets(CallAttemptOutcome::Aborted)
                                .into_iter()
                                .map(UpdateCallAction::CancelRingingTarget),
                        );
                    }
                    _ => {}
                }
            }

            {
                let mut active_calls = self.active_calls.write();
                match active_calls.entry(active_or_ringing_call_id) {
                    Entry::Occupied(mut entry) => {
                        has_active_or_outgoing_call = true;

                        let active_call = entry.get_mut();

                        if active_call.participants.contains_key(client_id) {
                            let participants_without_self =
                                active_call.participants_without_self(client_id);

                            if participants_without_self.is_empty() {
                                tracing::error!(
                                    "Disconnecting client has active call, which has no other participant than self"
                                );
                                ErrorMetrics::peer_not_found();
                                entry.remove();
                            } else if active_call.conference_leader.as_ref() == Some(client_id)
                                || participants_without_self.len() <= 1
                            {
                                entry.remove();
                                drop(active_calls);

                                {
                                    let mut client_active_calls = self.client_active_calls.write();
                                    for participant_id in participants_without_self.keys() {
                                        if client_active_calls
                                            .get(participant_id)
                                            .is_some_and(|c| *c == active_or_ringing_call_id)
                                        {
                                            client_active_calls.remove(participant_id);
                                        }
                                    }
                                }

                                actions.extend(participants_without_self.into_keys().map(
                                    |participant_id| {
                                        UpdateCallAction::DropParticipant(
                                            active_or_ringing_call_id,
                                            participant_id,
                                        )
                                    },
                                ));
                                actions.extend(self.cancel_pending_invitations(
                                    &active_or_ringing_call_id,
                                    CallAttemptOutcome::Aborted,
                                ));
                            } else {
                                if active_call.participants.remove(client_id).is_none() {
                                    tracing::error!(
                                        "Tried to remove disconnecting participant from active call, but no entry found; sending update anyway..."
                                    );
                                }

                                if active_call.participants.len() <= 2 {
                                    active_call.conference_leader = None;
                                }

                                let joined_participants = active_call.participants.clone();

                                drop(active_calls);

                                self.client_active_calls.write().remove(client_id);

                                let invited_participants = self
                                    .ringing_calls
                                    .read()
                                    .get(&active_or_ringing_call_id)
                                    .map(|ringing_call| ringing_call.invited_participants())
                                    .unwrap_or_default();

                                let update = UpdateParticipants {
                                    call_id: active_or_ringing_call_id,
                                    invited_participants,
                                    joined_participants,
                                };

                                actions.push(UpdateCallAction::UpdateParticipants(update));
                            }
                        } else {
                            tracing::error!(
                                "Client has active call, but does not participate in that call"
                            );
                        }
                    }
                    Entry::Vacant(_) => {}
                }
            }

            if !has_active_or_outgoing_call {
                tracing::error!(
                    "Client has active call, but call was not found in ringing or active"
                );
            }
        }

        let incoming_call_ids = self.client_incoming_calls.write().remove(client_id);
        if let Some(incoming_call_ids) = incoming_call_ids {
            let mut ringing_calls = self.ringing_calls.write();

            for (call_id, call_target) in incoming_call_ids {
                match ringing_calls.entry(call_id) {
                    Entry::Occupied(mut entry) => {
                        let ringing_call = entry.get_mut();

                        match ringing_call.targets.entry(call_target.clone()) {
                            Entry::Occupied(mut entry) => {
                                let ringing_target = entry.get_mut();

                                ringing_target.notified_clients.remove(client_id);
                                ringing_target.rejected_clients.remove(client_id);
                                ringing_target.errored_clients.remove(client_id);

                                tracing::trace!(
                                    ?call_id,
                                    ?ringing_target,
                                    "Removing client from incoming ringing call"
                                );

                                if ringing_target.all_rejected_or_errored() {
                                    tracing::trace!(
                                        ?call_id,
                                        "Aborting incoming ringing call target"
                                    );

                                    if let Some(active_call) =
                                        self.active_calls.read().get(&call_id)
                                    {
                                        entry.remove();

                                        actions.push(UpdateCallAction::UpdateParticipants(
                                            UpdateParticipants {
                                                call_id,
                                                invited_participants: ringing_call
                                                    .invited_participants(),
                                                joined_participants: active_call
                                                    .participants
                                                    .clone(),
                                            },
                                        ));
                                    } else {
                                        actions.push(UpdateCallAction::CancelRingingTarget(
                                            ringing_target.complete(
                                                CallAttemptOutcome::Aborted,
                                                &ringing_call.call_id,
                                                &ringing_call.caller_id,
                                                ringing_target.source.clone(),
                                                &call_target,
                                            ),
                                        ));

                                        entry.remove();
                                    }
                                }
                            }
                            Entry::Vacant(_) => {
                                tracing::error!(
                                    ?call_id,
                                    ?call_target,
                                    "Client has incoming call but no related ringing target"
                                );
                            }
                        }

                        if ringing_call.all_targets_failed() {
                            if !self.active_calls.read().values().any(|active_call_entry| {
                                active_call_entry.involves(&ringing_call.caller_id)
                            }) {
                                self.client_active_calls
                                    .write()
                                    .remove(&ringing_call.caller_id);
                            }

                            entry.remove();
                        }
                    }
                    Entry::Vacant(_) => {
                        tracing::error!(
                            ?call_id,
                            ?call_target,
                            "Client has incoming call but no related ringing call"
                        );
                    }
                }
            }
        }

        for action in actions {
            match action {
                UpdateCallAction::CancelRingingTarget(ringing_target) => {
                    if ringing_target.caller_id == *client_id {
                        let cancelled = server::CallCancelled::new(
                            ringing_target.call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );
                        for callee_id in ringing_target.notified_clients {
                            tracing::trace!(
                                ?callee_id,
                                "Sending call cancelled to notified client"
                            );
                            if let Err(err) =
                                state.send_message(&callee_id, cancelled.clone()).await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?callee_id,
                                    "Failed to send call cancelled to notified client"
                                );
                            }
                        }
                    } else {
                        tracing::trace!(
                            "Ringing target failed or was torn down with the call, sending call cancelled to source and notified clients"
                        );
                        let cancelled = server::CallCancelled::new(
                            ringing_target.call_id,
                            HashSet::from([ringing_target.target.clone()]),
                            CallCancelReason::Disconnected,
                        );
                        if let Err(err) = state
                            .send_message(&ringing_target.caller_id, cancelled.clone())
                            .await
                        {
                            tracing::warn!(?err, "Failed to send call cancelled to source client");
                        }
                        for callee_id in ringing_target.notified_clients {
                            tracing::trace!(
                                ?callee_id,
                                "Sending call cancelled to notified client"
                            );
                            if let Err(err) =
                                state.send_message(&callee_id, cancelled.clone()).await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?callee_id,
                                    "Failed to send call cancelled to notified client"
                                );
                            }
                        }
                    }
                }
                UpdateCallAction::DropParticipant(call_id, participant_id) => {
                    tracing::trace!(?participant_id, "Sending call end to participant");
                    if let Err(err) = state
                        .send_message(
                            &participant_id,
                            CallEnd::new(call_id, participant_id.clone()),
                        )
                        .await
                    {
                        tracing::warn!(
                            ?err,
                            ?participant_id,
                            "Failed to send call end to participant"
                        );
                    }
                }
                UpdateCallAction::UpdateParticipants(update) => {
                    tracing::trace!("Sending call update to all participants");
                    for (participant_id, _) in update.all_participants() {
                        if let Err(err) = state
                            .send_message(
                                participant_id,
                                ServerMessage::CallUpdate((&update).into()),
                            )
                            .await
                        {
                            tracing::warn!(
                                ?err,
                                ?participant_id,
                                "Failed to send call update to participant"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Removes the call's ringing entry regardless of who created it and returns
    /// cancellation actions for all still pending targets, so that pending
    /// invitations cannot outlive a fully torn down call.
    fn cancel_pending_invitations(
        &self,
        call_id: &CallId,
        outcome: CallAttemptOutcome,
    ) -> Vec<UpdateCallAction> {
        let Some(ringing) = self.ringing_calls.write().remove(call_id) else {
            return Vec::new();
        };

        self.cleanup_client_incoming_calls(&ringing);
        ringing
            .complete_all_targets(outcome)
            .into_iter()
            .map(UpdateCallAction::CancelRingingTarget)
            .collect()
    }

    fn remove_client_incoming_call(&self, call_id: &CallId, client_id: &ClientId) {
        let mut client_incoming_calls = self.client_incoming_calls.write();
        if let Some(calls) = client_incoming_calls.get_mut(client_id) {
            calls.remove(call_id);
            if calls.is_empty() {
                client_incoming_calls.remove(client_id);
            }
        }
    }

    fn cleanup_client_incoming_calls(&self, ringing_call: &RingingCallEntry) {
        let mut client_incoming_calls = self.client_incoming_calls.write();
        for callee_id in ringing_call
            .targets
            .values()
            .flat_map(|e| &e.notified_clients)
        {
            if let Some(calls) = client_incoming_calls.get_mut(callee_id) {
                calls.remove(&ringing_call.call_id);
                if calls.is_empty() {
                    client_incoming_calls.remove(callee_id);
                }
            }
        }
    }

    fn validate_call_attempt(
        &self,
        call_id: &CallId,
        caller_id: &ClientId,
        target: &CallTarget,
        notified_clients: &HashSet<ClientId>,
    ) -> Result<(), StartCallError> {
        let participating_call_id = self.client_active_calls.read().get(caller_id).copied();
        if let Some(participating_call_id) = participating_call_id {
            if &participating_call_id != call_id {
                tracing::warn!(
                    ?participating_call_id,
                    "Caller is already participating in a different call"
                );
                return Err(StartCallError::CallerBusy);
            }

            {
                let active_calls = self.active_calls.read();

                if let Some(active_call) = active_calls.get(call_id) {
                    if let Some(conference_leader) = active_call.conference_leader.as_ref()
                        && conference_leader != caller_id
                    {
                        tracing::warn!("Caller is not conference leader of this call");
                        return Err(StartCallError::NotConferenceLeader);
                    }

                    if active_call.participants.iter().any(
                        |(participant_id, participating_target)| {
                            if participating_target == target {
                                tracing::warn!("Target is already participating in this call");
                                return true;
                            }

                            notified_clients.iter().any(|callee| {
                                if participant_id == callee {
                                    tracing::warn!(
                                        ?callee,
                                        "Callee is already participating in this call"
                                    );
                                    true
                                } else {
                                    false
                                }
                            })
                        },
                    ) {
                        return Err(StartCallError::AlreadyParticipant);
                    }
                }

                let active_targets = active_calls
                    .get(call_id)
                    .map(|active_call_entry| {
                        active_call_entry
                            .participants
                            .values()
                            .collect::<HashSet<&CallTarget>>()
                    })
                    .unwrap_or_default();

                if active_targets.contains(target) {
                    tracing::warn!("Target is already participating in this call");
                    return Err(StartCallError::AlreadyParticipant);
                }
            }

            {
                let ringing_calls = self.ringing_calls.read();

                if let Some(ringing_call) = ringing_calls.get(call_id) {
                    if &ringing_call.caller_id != caller_id {
                        tracing::warn!("Caller is not previous caller of that call");
                        return Err(StartCallError::NotConferenceLeader);
                    }

                    if ringing_call.targets.contains_key(target) {
                        tracing::warn!("Caller has already rung this target");
                        return Err(StartCallError::AlreadyParticipant);
                    }

                    if let Some(callee) = ringing_call.targets.values().find_map(|ringing_target| {
                        notified_clients
                            .iter()
                            .find_map(|callee| ringing_target.notified_clients.get(callee))
                    }) {
                        tracing::warn!(?callee, "Caller has already rung this client");
                        return Err(StartCallError::AlreadyParticipant);
                    }
                }
            }
        } else {
            if self.active_calls.read().contains_key(call_id) {
                tracing::warn!("Caller has no reference to active call id from the invite");
                return Err(StartCallError::NotParticipant);
            }

            if self.ringing_calls.read().contains_key(call_id) {
                tracing::warn!("Caller has no reference to ringing call id from the invite");
                return Err(StartCallError::NotParticipant);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vacs_protocol::vatsim::StationId;

    fn source(client_id: &ClientId) -> CallSource {
        CallSource {
            client_id: client_id.clone(),
            position_id: None,
            station_id: None,
        }
    }

    /// A target covered by multiple clients only fails once EVERY notified client
    /// rejected or errored; a single client's failure must yield `Continued`, not
    /// `TargetFailed`. The invite fan-out handler relies on this distinction to
    /// decide whether a target drops out of the CallUpdate snapshot.
    #[test]
    fn call_error_is_per_client_not_per_target() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee1 = ClientId::from("callee1");
        let callee2 = ClientId::from("callee2");
        let target = CallTarget::Station(StationId::from("EDDF_TWR"));

        manager
            .attempt_call(
                &call_id,
                &caller,
                &source(&caller),
                &target,
                &HashSet::from([callee1.clone(), callee2.clone()]),
            )
            .expect("call attempt should succeed");

        assert_eq!(
            manager.call_error(&call_id, &callee1),
            CallTerminationOutcome::Continued,
            "the target must keep ringing while another notified client remains"
        );

        match manager.call_error(&call_id, &callee2) {
            CallTerminationOutcome::TargetFailed(ringing_targets, update) => {
                assert_eq!(
                    ringing_targets
                        .iter()
                        .map(|ringing_target| &ringing_target.target)
                        .collect::<Vec<_>>(),
                    vec![&target],
                    "the failed target must be reported"
                );
                assert!(
                    update.invited_participants.is_empty(),
                    "no invited participants must remain after the last client errored"
                );
            }
            outcome => {
                panic!("expected TargetFailed after the last client errored, got {outcome:?}")
            }
        }
    }

    /// Rings `callee` for `caller` within `call_id` and lets it accept, which
    /// either creates the active call or joins the existing one.
    fn join(manager: &CallManager, call_id: &CallId, caller: &ClientId, callee: &ClientId) {
        manager
            .attempt_call(
                call_id,
                caller,
                &source(caller),
                &CallTarget::Client(callee.clone()),
                &HashSet::from([callee.clone()]),
            )
            .expect("call attempt should succeed");

        assert!(
            matches!(
                manager.accept_call(call_id, callee),
                AcceptCallOutcome::Accepted { .. }
            ),
            "call should be accepted"
        );
    }

    /// The size check counts the caller, joined participants, ringing targets
    /// and only the genuinely new targets of the invite.
    #[test]
    fn max_conf_size_counts_new_targets_and_the_caller() {
        let manager = CallManager::new(3);
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");

        assert!(
            !manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([
                    CallTarget::Client(ClientId::from("a")),
                    CallTarget::Client(ClientId::from("b")),
                ]),
            ),
            "caller plus two targets must fit a limit of three"
        );
        assert!(
            manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([
                    CallTarget::Client(ClientId::from("a")),
                    CallTarget::Client(ClientId::from("b")),
                    CallTarget::Client(ClientId::from("c")),
                ]),
            ),
            "caller plus three targets must exceed a limit of three"
        );

        join(&manager, &call_id, &caller, &callee);

        assert!(
            !manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([CallTarget::Client(callee.clone())]),
            ),
            "a target that already joined must not count against the limit"
        );
        assert!(
            !manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([CallTarget::Client(ClientId::from("third"))]),
            ),
            "two joined plus one new target must fit a limit of three"
        );
        assert!(
            manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([
                    CallTarget::Client(ClientId::from("third")),
                    CallTarget::Client(ClientId::from("fourth")),
                ]),
            ),
            "two joined plus two new targets must exceed a limit of three"
        );
    }

    /// The caller of a purely ringing call is counted even though it has not
    /// joined yet, and already ringing targets are not counted twice.
    #[test]
    fn max_conf_size_counts_the_unjoined_caller_and_dedups_ringing_targets() {
        let manager = CallManager::new(3);
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");
        let target = CallTarget::Client(callee.clone());

        manager
            .attempt_call(
                &call_id,
                &caller,
                &source(&caller),
                &target,
                &HashSet::from([callee]),
            )
            .expect("call attempt should succeed");

        assert!(
            !manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([target.clone(), CallTarget::Client(ClientId::from("third"))]),
            ),
            "the already ringing target must not count twice"
        );
        assert!(
            manager.invite_exceeds_max_conf_size(
                &call_id,
                &caller,
                &HashSet::from([
                    CallTarget::Client(ClientId::from("third")),
                    CallTarget::Client(ClientId::from("fourth")),
                ]),
            ),
            "the unjoined caller plus one ringing plus two new targets must exceed a limit of three"
        );
    }

    /// A client that already participates in an active call must not be able
    /// to accept another one, however it was rung.
    #[test]
    fn accepting_while_in_another_call_is_rejected() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");

        join(&manager, &call_id, &caller, &callee);

        let other_call_id = CallId::new();
        let other_caller = ClientId::from("other-caller");
        manager
            .attempt_call(
                &other_call_id,
                &other_caller,
                &source(&other_caller),
                &CallTarget::Client(callee.clone()),
                &HashSet::from([callee.clone()]),
            )
            .expect("ringing a busy client is allowed");

        assert!(
            matches!(
                manager.accept_call(&other_call_id, &callee),
                AcceptCallOutcome::AcceptorBusy
            ),
            "a busy client's accept must be rejected"
        );
        assert!(
            manager.active_call(&other_call_id).is_none(),
            "the rejected accept must not create an active call"
        );
    }

    /// A late accept for a call the caller already cancelled must fail
    /// without creating an active call, and must leave both clients free to
    /// start new calls.
    #[test]
    fn late_accept_after_cancel_does_not_resurrect_the_call() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");

        manager
            .attempt_call(
                &call_id,
                &caller,
                &source(&caller),
                &CallTarget::Client(callee.clone()),
                &HashSet::from([callee.clone()]),
            )
            .expect("call attempt should succeed");
        manager
            .end_call(&call_id, &caller)
            .expect("cancelling the ringing call must produce actions");

        assert!(
            matches!(
                manager.accept_call(&call_id, &callee),
                AcceptCallOutcome::NotFound
            ),
            "the late accept must not find the cancelled call"
        );
        assert!(
            manager.active_call(&call_id).is_none(),
            "the late accept must not create an active call"
        );

        join(&manager, &CallId::new(), &caller, &callee);
    }

    /// Only the client that sent an invitation may cancel it, even though every
    /// participant of a two party call may invite while there is no leader.
    #[test]
    fn dropping_a_ringing_target_requires_the_inviter() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");
        let invited = ClientId::from("invited");

        join(&manager, &call_id, &caller, &callee);

        manager
            .attempt_call(
                &call_id,
                &callee,
                &source(&callee),
                &CallTarget::Client(invited.clone()),
                &HashSet::from([invited.clone()]),
            )
            .expect("participant should be able to invite into a two party call");

        assert_eq!(
            manager.drop_target(
                &call_id,
                &caller,
                &CallTarget::Client(invited),
                CallDropReason::Requested,
            ),
            DropTargetOutcome::NotPermitted,
            "a client must not cancel an invitation it did not send"
        );
    }

    /// Dropping the last ringing target of a call nobody joined ends the call,
    /// so the caller must not stay busy afterwards.
    #[test]
    fn dropping_the_last_ringing_target_frees_the_caller() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");

        manager
            .attempt_call(
                &call_id,
                &caller,
                &source(&caller),
                &CallTarget::Client(callee.clone()),
                &HashSet::from([callee.clone()]),
            )
            .expect("call attempt should succeed");

        match manager.drop_target(
            &call_id,
            &caller,
            &CallTarget::Client(callee),
            CallDropReason::AutoHangup,
        ) {
            DropTargetOutcome::RingingTargetCancelled(_, update) => {
                assert!(
                    update.invited_participants.is_empty() && update.joined_participants.is_empty(),
                    "the call must be over once its last ringing target is dropped"
                );
            }
            outcome => panic!("expected the ringing target to be cancelled, got {outcome:?}"),
        }

        let other = ClientId::from("other");
        manager
            .attempt_call(
                &CallId::new(),
                &caller,
                &source(&caller),
                &CallTarget::Client(other.clone()),
                &HashSet::from([other]),
            )
            .expect("caller should be free after dropping its last ringing target");
    }

    /// An invitation timer that expires while the target answers must never
    /// remove the target from the call it just joined.
    #[test]
    fn auto_hangup_never_drops_a_joined_participant() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");
        let invited = ClientId::from("invited");

        join(&manager, &call_id, &caller, &callee);
        join(&manager, &call_id, &caller, &invited);

        match manager.drop_target(
            &call_id,
            &caller,
            &CallTarget::Client(invited.clone()),
            CallDropReason::AutoHangup,
        ) {
            DropTargetOutcome::Obsolete(update) => {
                assert!(
                    update.joined_participants.contains_key(&invited),
                    "the answering client must stay in the call"
                );
            }
            outcome => panic!("expected the auto hangup to be obsolete, got {outcome:?}"),
        }
    }

    #[test]
    fn dropping_a_participant_requires_the_conference_leader() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");
        let invited = ClientId::from("invited");

        join(&manager, &call_id, &caller, &callee);
        join(&manager, &call_id, &caller, &invited);

        assert_eq!(
            manager.drop_target(
                &call_id,
                &callee,
                &CallTarget::Client(invited.clone()),
                CallDropReason::Requested,
            ),
            DropTargetOutcome::NotPermitted,
            "a participant that does not lead the conference must not drop anyone"
        );

        match manager.drop_target(
            &call_id,
            &caller,
            &CallTarget::Client(invited.clone()),
            CallDropReason::Requested,
        ) {
            DropTargetOutcome::ParticipantDropped(dropped_client_id, update) => {
                assert_eq!(
                    dropped_client_id, invited,
                    "the wrong participant was dropped"
                );
                assert_eq!(
                    update.joined_participants.keys().collect::<HashSet<_>>(),
                    HashSet::from([&caller, &callee]),
                    "the remaining participants must stay in the call"
                );
            }
            outcome => panic!("expected the participant to be dropped, got {outcome:?}"),
        }

        assert!(
            manager
                .active_call(&call_id)
                .expect("call should still be active")
                .conference_leader
                .is_none(),
            "a call of two must not keep a conference leader"
        );
    }

    /// Dropping the only other participant of a two party call would leave a
    /// call of one; leaving is [`CallManager::end_call`]'s job.
    #[test]
    fn dropping_a_participant_of_a_two_party_call_is_not_permitted() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee = ClientId::from("callee");

        join(&manager, &call_id, &caller, &callee);

        assert_eq!(
            manager.drop_target(
                &call_id,
                &caller,
                &CallTarget::Client(callee),
                CallDropReason::Requested,
            ),
            DropTargetOutcome::NotPermitted,
            "a two party call has no participant to drop"
        );
    }

    /// A failed client only takes down its own target; other ringing targets of the
    /// same call are unaffected.
    #[test]
    fn call_error_leaves_other_targets_ringing() {
        let manager = CallManager::default();
        let call_id = CallId::new();
        let caller = ClientId::from("caller");
        let callee1 = ClientId::from("callee1");
        let callee2 = ClientId::from("callee2");
        let target1 = CallTarget::Client(callee1.clone());
        let target2 = CallTarget::Client(callee2.clone());

        for (target, callee) in [(&target1, &callee1), (&target2, &callee2)] {
            manager
                .attempt_call(
                    &call_id,
                    &caller,
                    &source(&caller),
                    target,
                    &HashSet::from([callee.clone()]),
                )
                .expect("call attempt should succeed");
        }

        match manager.call_error(&call_id, &callee1) {
            CallTerminationOutcome::TargetFailed(ringing_targets, update) => {
                assert_eq!(
                    ringing_targets
                        .iter()
                        .map(|ringing_target| &ringing_target.target)
                        .collect::<Vec<_>>(),
                    vec![&target1],
                    "only the errored client's target must fail"
                );
                assert_eq!(
                    update.invited_participants,
                    CallParticipants::from([(callee2.clone(), target2.clone())]),
                    "the other target must keep ringing"
                );
            }
            outcome => {
                panic!("expected TargetFailed for the errored client's target, got {outcome:?}")
            }
        }
    }
}
