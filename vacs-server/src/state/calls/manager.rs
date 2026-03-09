use crate::metrics::ErrorMetrics;
use crate::metrics::guards::CallAttemptOutcome;
use crate::state::AppState;
use crate::state::calls::{ActiveCall, ActiveCallEntry, RingingCall, RingingCallEntry};
use parking_lot::RwLock;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use tracing::instrument;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::server;
use vacs_protocol::ws::server::CallCancelReason;
use vacs_protocol::ws::shared::{CallEnd, CallId, CallTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartCallError {
    CallerBusy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTerminationOutcome {
    CallNotFound,
    ClientNotNotified,
    Continued,
    Failed(RingingCall),
}

pub struct CallManager {
    ringing_calls: RwLock<HashMap<CallId, RingingCallEntry>>,
    active_calls: RwLock<HashMap<CallId, ActiveCallEntry>>,
    client_incoming_calls: RwLock<HashMap<ClientId, HashSet<CallId>>>,
    client_outgoing_calls: RwLock<HashMap<ClientId, CallId>>,
    client_active_calls: RwLock<HashMap<ClientId, CallId>>,
}

impl Default for CallManager {
    fn default() -> Self {
        CallManager::new()
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
    pub fn new() -> Self {
        Self {
            ringing_calls: RwLock::new(HashMap::new()),
            active_calls: RwLock::new(HashMap::new()),
            client_incoming_calls: RwLock::new(HashMap::new()),
            client_outgoing_calls: RwLock::new(HashMap::new()),
            client_active_calls: RwLock::new(HashMap::new()),
        }
    }

    pub fn has_outgoing_call(&self, client_id: &ClientId) -> bool {
        self.client_outgoing_calls.read().contains_key(client_id)
    }

    pub fn has_active_call(&self, call_id: &CallId, client_id: &ClientId) -> bool {
        self.active_calls
            .read()
            .get(call_id)
            .is_some_and(|active| active.involves(client_id))
    }

    pub fn ringing_call(&self, call_id: &CallId) -> Option<RingingCall> {
        self.ringing_calls.read().get(call_id).map(Into::into)
    }

    pub fn active_call(&self, call_id: &CallId) -> Option<ActiveCall> {
        self.active_calls.read().get(call_id).map(Into::into)
    }

    pub fn start_call_attempt(
        &self,
        call_id: &CallId,
        caller_id: &ClientId,
        target: &CallTarget,
        notified_clients: &HashSet<ClientId>,
    ) -> Result<(), StartCallError> {
        if self.has_outgoing_call(caller_id) {
            tracing::warn!("Client already has outgoing call");
            return Err(StartCallError::CallerBusy);
        }

        let ringing = RingingCallEntry::new(
            *call_id,
            caller_id.clone(),
            target.clone(),
            notified_clients.clone(),
        );

        self.ringing_calls.write().insert(*call_id, ringing);
        self.client_outgoing_calls
            .write()
            .insert(caller_id.clone(), *call_id);

        let mut client_incoming_calls = self.client_incoming_calls.write();
        for client_id in notified_clients {
            client_incoming_calls
                .entry(client_id.clone())
                .or_default()
                .insert(*call_id);
        }

        Ok(())
    }

    pub fn reject_call(
        &self,
        call_id: &CallId,
        rejecting_client_id: &ClientId,
    ) -> CallTerminationOutcome {
        self.remove_client_incoming_call(call_id, rejecting_client_id);

        let mut ringing_calls = self.ringing_calls.write();
        match ringing_calls.entry(*call_id) {
            Entry::Occupied(mut entry) => {
                if !entry.get().has_notified_client(rejecting_client_id) {
                    return CallTerminationOutcome::ClientNotNotified;
                }

                if entry.get_mut().mark_rejected(rejecting_client_id) {
                    let ringing = entry.remove();
                    drop(ringing_calls);
                    self.cleanup_ringing_call(&ringing);
                    CallTerminationOutcome::Failed(ringing.complete(CallAttemptOutcome::Rejected))
                } else {
                    CallTerminationOutcome::Continued
                }
            }
            Entry::Vacant(_) => CallTerminationOutcome::CallNotFound,
        }
    }

    pub fn call_error(
        &self,
        call_id: &CallId,
        erroring_client_id: &ClientId,
    ) -> CallTerminationOutcome {
        self.remove_client_incoming_call(call_id, erroring_client_id);

        let mut ringing_calls = self.ringing_calls.write();
        match ringing_calls.entry(*call_id) {
            Entry::Occupied(mut entry) => {
                if !entry.get().has_notified_client(erroring_client_id) {
                    return CallTerminationOutcome::ClientNotNotified;
                }

                if entry.get_mut().mark_errored(erroring_client_id) {
                    let ringing = entry.remove();
                    drop(ringing_calls);
                    self.cleanup_ringing_call(&ringing);
                    // TODO: should we allow passing strict error reason here?
                    CallTerminationOutcome::Failed(ringing.complete(CallAttemptOutcome::Error(
                        vacs_protocol::ws::shared::CallErrorReason::CallFailure,
                    )))
                } else {
                    CallTerminationOutcome::Continued
                }
            }
            Entry::Vacant(_) => CallTerminationOutcome::CallNotFound,
        }
    }

    pub fn accept_call(
        &self,
        call_id: &CallId,
        accepting_client_id: &ClientId,
    ) -> Option<RingingCall> {
        let ringing = {
            let mut ringing_calls = self.ringing_calls.write();
            match ringing_calls.entry(*call_id) {
                Entry::Occupied(entry) if entry.get().has_notified_client(accepting_client_id) => {
                    Some(entry.remove())
                }
                _ => None,
            }
        }?;

        self.cleanup_ringing_call(&ringing);

        let active = ActiveCallEntry::new(
            *call_id,
            ringing.caller_id.clone(),
            accepting_client_id.clone(),
        );

        self.active_calls.write().insert(*call_id, active);
        {
            let mut client_active_calls = self.client_active_calls.write();
            client_active_calls.insert(ringing.caller_id.clone(), *call_id);
            client_active_calls.insert(accepting_client_id.clone(), *call_id);
        }

        Some(ringing.complete(CallAttemptOutcome::Accepted))
    }

    pub fn cancel_ringing_call(
        &self,
        call_id: &CallId,
        cancelling_client_id: &ClientId,
        outcome: CallAttemptOutcome,
    ) -> Option<RingingCall> {
        let ringing = {
            let mut ringing_calls = self.ringing_calls.write();
            match ringing_calls.entry(*call_id) {
                Entry::Occupied(entry) if entry.get().involves(cancelling_client_id) => {
                    Some(entry.remove())
                }
                _ => None,
            }
        }?;

        self.cleanup_ringing_call(&ringing);

        Some(ringing.complete(outcome))
    }

    pub fn end_ringing_call(
        &self,
        call_id: &CallId,
        cancelling_client_id: &ClientId,
    ) -> Option<RingingCall> {
        let ringing = {
            let mut ringing_calls = self.ringing_calls.write();
            match ringing_calls.entry(*call_id) {
                Entry::Occupied(entry) if entry.get().caller_id == *cancelling_client_id => {
                    Some(entry.remove())
                }
                _ => None,
            }
        }?;

        self.cleanup_ringing_call(&ringing);

        Some(ringing.complete(CallAttemptOutcome::Cancelled))
    }

    pub fn end_active_call(
        &self,
        call_id: &CallId,
        ending_client_id: &ClientId,
    ) -> Option<ActiveCall> {
        let active = {
            let mut active_calls = self.active_calls.write();
            match active_calls.entry(*call_id) {
                Entry::Occupied(entry) if entry.get().involves(ending_client_id) => {
                    Some(entry.remove())
                }
                _ => None,
            }
        }?;

        {
            let mut client_active_calls = self.client_active_calls.write();
            client_active_calls.remove(&active.caller_id);
            client_active_calls.remove(&active.callee_id);
        }

        Some(ActiveCall::from(active))
    }

    #[instrument(level = "trace", skip(self, state))]
    pub async fn cleanup_client_calls(&self, state: &AppState, client_id: &ClientId) {
        tracing::trace!("Cleaning up client calls");

        let mut cleaned_ringing_calls: Vec<RingingCall> = Vec::new();
        let mut cleaned_active_call: Option<ActiveCall> = None;

        let outgoing_call_id = { self.client_outgoing_calls.write().remove(client_id) };
        if let Some(outgoing_call_id) = outgoing_call_id {
            let ringing = { self.ringing_calls.write().remove(&outgoing_call_id) };
            if let Some(ringing) = ringing {
                {
                    let mut client_incoming_calls = self.client_incoming_calls.write();
                    for callee_id in ringing.notified_clients.iter() {
                        if let Some(calls) = client_incoming_calls.get_mut(callee_id) {
                            calls.remove(&outgoing_call_id);
                            if calls.is_empty() {
                                client_incoming_calls.remove(callee_id);
                            }
                        }
                    }
                }

                tracing::trace!(?outgoing_call_id, "Aborting outgoing ringing call");
                cleaned_ringing_calls.push(ringing.complete(CallAttemptOutcome::Aborted)); // TODO other outcome?
            }
        }

        let incoming_call_ids = { self.client_incoming_calls.write().remove(client_id) };
        if let Some(incoming_call_ids) = incoming_call_ids {
            let mut ringing_calls = self.ringing_calls.write();
            let mut removed_call_ids = Vec::new();

            for call_id in incoming_call_ids {
                if let Some(ringing) = ringing_calls.get_mut(&call_id) {
                    ringing.notified_clients.remove(client_id);
                    ringing.rejected_clients.remove(client_id);
                    ringing.errored_clients.remove(client_id);

                    tracing::trace!(
                        ?call_id,
                        ?ringing,
                        "Removing client from incoming ringing call"
                    );

                    if ringing.all_rejected_or_errored() {
                        tracing::trace!(?call_id, "Aborting incoming ringing call");
                        ringing.set_outcome(CallAttemptOutcome::Aborted); // TODO other outcome?
                        cleaned_ringing_calls.push(ringing.to_ringing_call());
                        removed_call_ids.push(call_id);
                    }
                }
            }

            for call_id in removed_call_ids {
                ringing_calls.remove(&call_id);
            }
        }

        let active_call_id = { self.client_active_calls.write().remove(client_id) };
        if let Some(active_call_id) = active_call_id {
            let active = { self.active_calls.write().remove(&active_call_id) };
            if let Some(active) = active
                && let Some(peer_id) = active.peer(client_id)
            {
                {
                    let mut client_active_calls = self.client_active_calls.write();
                    if client_active_calls
                        .get(peer_id)
                        .is_some_and(|c| *c == active_call_id)
                    {
                        client_active_calls.remove(peer_id);
                    }
                }

                cleaned_active_call = Some(ActiveCall::from(active));
            }
        }

        for ringing in cleaned_ringing_calls {
            self.client_outgoing_calls
                .write()
                .remove(&ringing.caller_id);

            if ringing.caller_id == *client_id {
                let cancelled =
                    server::CallCancelled::new(ringing.call_id, CallCancelReason::CallerCancelled);
                for callee_id in ringing.notified_clients {
                    tracing::trace!(?callee_id, "Sending call cancelled to notified client");
                    if let Err(err) = state.send_message(&callee_id, cancelled.clone()).await {
                        tracing::warn!(
                            ?err,
                            ?callee_id,
                            "Failed to send call cancelled to notified client"
                        );
                    }
                }
            } else {
                tracing::trace!(
                    "All notified clients either rejected or errored, call failed, sending call error to source client"
                );
                // TODO send CallCancelled to all notified, just in case?
                if let Err(err) = state
                    .send_message(
                        &ringing.caller_id,
                        server::CallCancelled::new(ringing.call_id, CallCancelReason::Disconnected),
                    )
                    .await
                {
                    tracing::warn!(?err, "Failed to send call error to source client");
                }
            }
        }

        if let Some(active) = cleaned_active_call
            && let Some(peer_id) = active.peer(client_id)
        {
            tracing::trace!(?peer_id, "Sending call end to peer");
            if let Err(err) = state
                .send_message(peer_id, CallEnd::new(active.call_id, peer_id.clone()))
                .await
            {
                tracing::warn!(?err, ?peer_id, "Failed to send call end to peer");
            }
        } else {
            ErrorMetrics::peer_not_found();
            tracing::warn!("No peer found for active call");
        }
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

    fn cleanup_ringing_call(&self, ringing: &RingingCallEntry) {
        self.client_outgoing_calls
            .write()
            .remove(&ringing.caller_id);

        let mut client_incoming_calls = self.client_incoming_calls.write();
        for callee_id in ringing.notified_clients.iter() {
            if let Some(calls) = client_incoming_calls.get_mut(callee_id) {
                calls.remove(&ringing.call_id);
                if calls.is_empty() {
                    client_incoming_calls.remove(callee_id);
                }
            }
        }
    }
}
