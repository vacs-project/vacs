use crate::metrics::{CallMetrics, ErrorMetrics};
use crate::ratelimit::CallInviteRejection;
use crate::state::AppState;
use crate::state::calls::{
    AcceptCallOutcome, CallTerminationOutcome, DropTargetOutcome, LinkReportOutcome, RingingTarget,
    StartCallError, UpdateCallAction, UpdateParticipants,
};
use crate::state::clients::session::ClientSession;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::{
    CallAccept, CallDropReason, CallDropTarget, CallInvite, CallReject, ClientMessage,
};
use vacs_protocol::ws::server::{CallCancelReason, CallInvitation, ServerMessage};
use vacs_protocol::ws::shared::{
    CallEnd, CallError, CallErrorReason, CallId, CallTarget, ErrorReason, WebrtcAnswer,
    WebrtcIceCandidate, WebrtcOffer,
};
use vacs_protocol::ws::{server, shared};

#[tracing::instrument(level = "trace", skip(state))]
pub async fn handle_application_message(
    state: &Arc<AppState>,
    client: &ClientSession,
    message: ClientMessage,
) -> ControlFlow<(), ()> {
    tracing::trace!("Handling application message");

    match message {
        ClientMessage::ListClients => {
            tracing::trace!("Returning list of clients");
            let clients = state.list_clients(Some(client.id())).await;
            if let Err(err) = client.send_message(server::ClientList { clients }).await {
                tracing::warn!(?err, "Failed to send client list");
            }
        }
        ClientMessage::ListStations => {
            tracing::trace!("Returning list of stations");
            let stations = state
                .clients
                .list_stations(client.active_profile(), client.position_id())
                .await;
            if let Err(err) = client.send_message(server::StationList { stations }).await {
                tracing::warn!(?err, "Failed to send station list");
            }
        }
        ClientMessage::CallInvite(call_invite) => {
            handle_call_invite(state, client, call_invite).await;
        }
        ClientMessage::CallAccept(call_accept) => {
            handle_call_accept(state, client, call_accept).await;
        }
        ClientMessage::CallReject(call_reject) => {
            handle_call_reject(state, client, call_reject).await;
        }
        ClientMessage::CallEnd(call_end) => {
            handle_call_end(state, client, call_end).await;
        }
        ClientMessage::CallError(call_error) => {
            handle_call_error(state, client, call_error).await;
        }
        ClientMessage::CallDropTarget(call_drop_target) => {
            handle_call_drop_target(state, client, call_drop_target).await;
        }
        ClientMessage::WebrtcOffer(webrtc_offer) => {
            handle_webrtc_offer(state, client, webrtc_offer).await;
        }
        ClientMessage::WebrtcAnswer(webrtc_answer) => {
            handle_webrtc_answer(state, client, webrtc_answer).await;
        }
        ClientMessage::WebrtcIceCandidate(webrtc_ice_candidate) => {
            handle_webrtc_ice_candidate(state, client, webrtc_ice_candidate).await;
        }
        ClientMessage::Logout | ClientMessage::Disconnect => return ControlFlow::Break(()),
        ClientMessage::Login(_) | ClientMessage::Error(_) => {}
    };
    ControlFlow::Continue(())
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_invite(state: &AppState, client: &ClientSession, invite: CallInvite) {
    tracing::trace!("Handling call invite");
    let caller_id = client.id();
    let call_id = &invite.call_id;

    if invite.source.client_id != *caller_id {
        tracing::debug!("Source client ID mismatch, rejecting call invite");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Source client ID mismatch"),
        )
        .await;
        return;
    }

    if invite.targets.is_empty() {
        tracing::debug!("Call invite has no targets, rejecting call invite");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("No targets specified"),
        )
        .await;
        return;
    }

    let mut not_found_targets = HashSet::new();
    let mut resolved_targets: Vec<(CallTarget, HashSet<ClientId>)> = Vec::new();

    CallMetrics::call_invite_targets(invite.targets.len());

    for target in &invite.targets {
        let target_clients: HashSet<ClientId> = match target {
            CallTarget::Client(client_id) => {
                if state.clients.is_client_connected(client_id).await {
                    HashSet::from([client_id.clone()])
                } else {
                    HashSet::new()
                }
            }
            CallTarget::Position(position_id) => {
                state.clients.clients_for_position(position_id).await
            }
            CallTarget::Station(station_id) => state.clients.clients_for_station(station_id).await,
        }
        .into_iter()
        .filter(|client_id| client_id != client.id())
        .collect();

        if target_clients.is_empty() {
            tracing::debug!("Call target has no clients, skipping target");
            not_found_targets.insert(target.clone());
            continue;
        }

        resolved_targets.push((target.clone(), target_clients));
    }

    if resolved_targets.is_empty() {
        tracing::trace!("No call target has clients, returning targets not found error");
        send_call_error(
            client,
            call_id,
            CallErrorReason::TargetsNotFound(not_found_targets),
            None,
        )
        .await;
        return;
    }

    // Checked on the resolved targets so unreachable or already present ones
    // do not count against the limit.
    let resolved_target_set: HashSet<CallTarget> = resolved_targets
        .iter()
        .map(|(target, _)| target.clone())
        .collect();
    if state
        .calls
        .invite_exceeds_max_conf_size(&invite.call_id, caller_id, &resolved_target_set)
    {
        tracing::debug!("Call invite would exceed max conf size, rejecting call invite");
        send_call_error(
            client,
            call_id,
            CallErrorReason::MaxConferenceSizeReached(invite.targets),
            None,
        )
        .await;
        return;
    }

    match state
        .rate_limiters()
        .check_call_invite(caller_id, resolved_targets.len())
    {
        Ok(()) => {}
        Err(CallInviteRejection::RateLimited(until)) => {
            tracing::debug!(?until, "Rate limit exceeded, rejecting call invite");
            let reason = ErrorReason::RateLimited {
                targets: invite.targets,
                retry_after_secs: until.as_secs(),
            };
            ErrorMetrics::error(&reason);
            client
                .send_error(shared::Error::from(reason).with_call_id(invite.call_id))
                .await;
            return;
        }
        Err(CallInviteRejection::TooManyTargets) => {
            tracing::debug!("Call invite has too many targets, rejecting call invite");
            send_call_error(
                client,
                call_id,
                CallErrorReason::Other,
                Some("Too many targets"),
            )
            .await;
            return;
        }
    }

    let mut invited_participants = HashMap::new();
    let mut joined_participants = HashMap::new();
    let mut all_target_participants = HashMap::new();

    for (target, target_clients) in &resolved_targets {
        match state
            .calls
            .attempt_call(call_id, client.id(), &invite.source, target, target_clients)
        {
            Ok((invited, joined)) => {
                invited_participants = invited;
                joined_participants = joined;
                all_target_participants.extend(
                    target_clients
                        .iter()
                        .cloned()
                        .map(|target_client| (target_client, target.clone())),
                );

                CallMetrics::call_invite(&invite.source, target, invite.prio);
            }
            Err(StartCallError::CallerBusy) => {
                tracing::debug!("Client already has an outgoing call, rejecting call invite");
                send_call_error(client, call_id, CallErrorReason::CallActive, None).await;
                return;
            }
            Err(StartCallError::NotParticipant) => {
                tracing::debug!("Client is not participant of call id, rejecting call invite");
                send_call_error(client, call_id, CallErrorReason::NotParticipant, None).await;
                return;
            }
            Err(StartCallError::AlreadyParticipant) => {
                tracing::debug!("Target or client is already a participant, rejecting call invite");
                send_call_error(
                    client,
                    call_id,
                    CallErrorReason::AlreadyParticipant(target.clone()),
                    None,
                )
                .await;
                continue;
            }
            Err(StartCallError::NotConferenceLeader) => {
                tracing::debug!("Caller is not conference leader, rejecting call invite");
                send_call_error(
                    client,
                    call_id,
                    CallErrorReason::NotConferenceLeader(target.clone()),
                    None,
                )
                .await;
                continue;
            }
        }
    }

    if !not_found_targets.is_empty() {
        tracing::trace!("Some call targets have no clients, returning targets not found error");
        send_call_error(
            client,
            call_id,
            CallErrorReason::TargetsNotFound(not_found_targets),
            None,
        )
        .await;

        if invited_participants.is_empty() && joined_participants.is_empty() {
            return;
        }
    }

    let mut failed_targets: HashSet<&CallTarget> = HashSet::new();

    let invited_targets: HashSet<CallTarget> = invited_participants.values().cloned().collect();
    let conference_leader = state
        .calls
        .active_call(call_id)
        .and_then(|active_call| active_call.conference_leader);

    for (callee_id, target) in &all_target_participants {
        tracing::trace!(?callee_id, "Sending call invite to target");

        let invitation = CallInvitation {
            call_id: invite.call_id,
            source: invite.source.clone(),
            target: target.clone(),
            invited_targets: invited_targets
                .iter()
                .filter(|invited_target| *invited_target != target)
                .cloned()
                .collect(),
            joined_participants: joined_participants.clone(),
            conference_leader: conference_leader.clone(),
            prio: invite.prio,
        };

        if let Err(err) = state.send_message(callee_id, invitation).await {
            tracing::warn!(?err, ?callee_id, "Failed to send call invite to target");
            match state.calls.call_error(call_id, callee_id) {
                CallTerminationOutcome::Continued => {}
                CallTerminationOutcome::TargetFailed(ringing_targets, _) => {
                    tracing::trace!(?target, "All clients for target failed, cancelling target");
                    failed_targets.insert(target);
                    cancel_failed_target(
                        state,
                        call_id,
                        ringing_targets,
                        CallCancelReason::Errored(CallErrorReason::CallFailure),
                    )
                    .await;
                }
                outcome => {
                    tracing::error!(
                        ?outcome,
                        ?callee_id,
                        "Unexpected termination outcome for failed invitation send"
                    );
                    failed_targets.insert(target);
                }
            }
        }
    }

    let update = UpdateParticipants {
        call_id: invite.call_id,
        invited_participants: invited_participants
            .iter()
            .filter(|(_, invited_target)| !failed_targets.contains(invited_target))
            .map(|(id, invited_target)| (id.clone(), invited_target.clone()))
            .collect(),
        joined_participants: joined_participants.clone(),
        conference_leader,
    };

    // Newly invited clients already received the full state via the invitation; on a fresh
    // call every recipient is newly invited, so no update goes out. When a target failed
    // mid-fan-out the sent invitations are stale, so everyone gets the corrected snapshot.
    for (participant_id, _) in invited_participants
        .iter()
        .chain(joined_participants.iter())
    {
        if failed_targets.is_empty() && all_target_participants.contains_key(participant_id) {
            continue;
        }

        tracing::trace!(?participant_id, "Sending call update to participant");
        send_call_update(state, participant_id, &update).await;
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_accept(state: &AppState, client: &ClientSession, accept: CallAccept) {
    tracing::trace!("Handling call acceptance");
    let answerer_id = client.id();
    let call_id = &accept.call_id;

    if accept.accepting_client_id != *answerer_id {
        tracing::debug!("Accepting client ID mismatch, rejecting call acceptance");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Accepting client ID mismatch"),
        )
        .await;
        return;
    }

    let (accepted_target, update) = match state.calls.accept_call(call_id, answerer_id) {
        AcceptCallOutcome::Accepted { target, update } => (target, update),
        AcceptCallOutcome::AcceptorBusy => {
            tracing::warn!("Accepting client has already an active call, rejecting call accept");
            send_call_error(client, call_id, CallErrorReason::CallActive, None).await;
            return;
        }
        AcceptCallOutcome::NotFound => {
            tracing::warn!("No ringing call for accepting client found, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
            return;
        }
    };

    tracing::trace!("Sending call update to all invited participants");
    for participant_id in update.invited_participants.keys() {
        if let Err(err) = state
            .send_message(
                participant_id,
                ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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

    tracing::trace!("Sending call update to all joined participants");

    for participant_id in update.joined_participants.keys() {
        if let Err(err) = state
            .send_message(
                participant_id,
                ServerMessage::CallUpdate(update.for_recipient(participant_id)),
            )
            .await
        {
            tracing::warn!(
                ?err,
                ?participant_id,
                "Failed to send call acceptance to participant"
            );

            let Some(actions) = state.calls.end_call(call_id, participant_id) else {
                tracing::error!(
                    ?participant_id,
                    "Tried to send a call acceptance message to a participant, which is not a participant anymore"
                );
                continue;
            };

            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        tracing::trace!(
                            "Cancelling ringing target during call accept, due to failure in sending call acceptance to a participant"
                        );
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );

                        for notified_client in ringing_target.notified_clients {
                            tracing::trace!(
                                ?notified_client,
                                "Sending call cancelled to notified client"
                            );
                            if let Err(err) = state
                                .send_message(&notified_client, cancelled.clone())
                                .await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?notified_client,
                                    "Failed to send call cancelled to notified client"
                                );
                            }
                        }
                    }
                    UpdateCallAction::DropParticipant(_, dropped_participant_id) => {
                        tracing::trace!(
                            "Dropping participant during call accept, due to failure in sending call acceptance to a participant"
                        );
                        if let Err(err) = state
                            .send_message(
                                &dropped_participant_id,
                                CallError {
                                    call_id: *call_id,
                                    reason: CallErrorReason::SignalingFailure(
                                        participant_id.clone(),
                                    ),
                                    message: None,
                                },
                            )
                            .await
                        {
                            tracing::warn!(
                                ?err,
                                ?participant_id,
                                "Failed to send call error to participant"
                            );
                        }

                        if let Err(err) = state
                            .send_message(
                                &dropped_participant_id,
                                CallEnd::new(*call_id, participant_id.clone()),
                            )
                            .await
                        {
                            tracing::warn!(
                                ?err,
                                ?dropped_participant_id,
                                "Failed to send call end to participant"
                            );
                        }
                    }
                    UpdateCallAction::UpdateParticipants(update) => {
                        tracing::trace!(
                            "Send call update to remaining participants during call accept, due to failure in sending call acceptance to a participant"
                        );
                        for (participant_id, _) in update.all_participants() {
                            if let Err(err) = state
                                .send_message(
                                    participant_id,
                                    ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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
    }

    if accepted_target.notified_clients.len() > 1 {
        let cancelled = server::CallCancelled::new(
            *call_id,
            HashSet::new(),
            CallCancelReason::AnsweredElsewhere(answerer_id.clone()),
        );

        for callee_id in accepted_target.notified_clients {
            if callee_id == *answerer_id {
                continue;
            }

            tracing::trace!(
                ?callee_id,
                "Sending call cancelled to other notified client"
            );
            if let Err(err) = state.send_message(&callee_id, cancelled.clone()).await {
                tracing::warn!(
                    ?err,
                    ?callee_id,
                    "Failed to send call cancelled to other notified client"
                );
            }
        }
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_reject(state: &AppState, client: &ClientSession, reject: CallReject) {
    tracing::trace!("Handling call rejection");
    let rejecter_id = client.id();
    let call_id = &reject.call_id;

    if reject.rejecting_client_id != *rejecter_id {
        tracing::debug!("Rejecting client ID mismatch, rejecting call rejection");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Rejecting client ID mismatch"),
        )
        .await;
        return;
    }

    match state.calls.reject_call(call_id, rejecter_id) {
        CallTerminationOutcome::CallNotFound => {
            tracing::warn!("No ringing call found, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
            return;
        }
        CallTerminationOutcome::ClientNotNotified => {
            tracing::warn!("Client was not notified of this call, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
            return;
        }
        CallTerminationOutcome::Continued => {}
        CallTerminationOutcome::TargetFailed(ringing_targets, update) => {
            tracing::trace!(
                "All notified clients either rejected or errored, call failed, sending call error to source client"
            );

            cancel_failed_target(
                state,
                call_id,
                ringing_targets,
                CallCancelReason::Rejected(reject.reason),
            )
            .await;

            tracing::trace!("Send call update to remaining participants during call reject");
            for (participant_id, _) in update.all_participants() {
                if let Err(err) = state
                    .send_message(
                        participant_id,
                        ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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
        CallTerminationOutcome::Changed(actions) => {
            tracing::error!(
                ?actions,
                "Ignoring unexpected update call actions after rejecting call"
            );
        }
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_end(state: &AppState, client: &ClientSession, end: CallEnd) {
    tracing::trace!("Handling call end");
    let ender_id = client.id();
    let call_id = &end.call_id;

    if end.ending_client_id != *ender_id {
        tracing::debug!("Ending client ID mismatch, rejecting call end");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Ending client ID mismatch"),
        )
        .await;
        return;
    }

    match state.calls.end_call(call_id, ender_id) {
        Some(actions) => {
            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        tracing::trace!("Ringing target found, canceling");
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );

                        for notified_client in ringing_target.notified_clients {
                            tracing::trace!(
                                ?notified_client,
                                "Sending call cancelled to notified client"
                            );
                            if let Err(err) = state
                                .send_message(&notified_client, cancelled.clone())
                                .await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?notified_client,
                                    "Failed to send call cancelled to notified client"
                                );
                            }
                        }
                    }
                    UpdateCallAction::DropParticipant(_, participant_id) => {
                        tracing::trace!("Dropping participant during call end");
                        if let Err(err) = state.send_message(&participant_id, end.clone()).await {
                            tracing::warn!(
                                ?err,
                                ?participant_id,
                                "Failed to send call end to peer"
                            );
                            send_call_error(
                                client,
                                call_id,
                                CallErrorReason::SignalingFailure(participant_id.clone()),
                                None,
                            )
                            .await;
                        }
                    }
                    UpdateCallAction::UpdateParticipants(update) => {
                        tracing::trace!("Updating all remaining participants during call end");
                        for (participant_id, _) in update.all_participants() {
                            if let Err(err) = state
                                .send_message(
                                    participant_id,
                                    ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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
        None => {
            tracing::trace!("No ringing or active call found, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallNotFound, None).await;
            return;
        }
    }
}

/// Handles a dead-link report. A single report is only recorded; once both
/// endpoints of the pair have reported, the later joiner is evicted: it
/// receives the reason naming the peer it could not reach followed by a
/// `CallEnd`, while the remaining participants get the regular leave fan-out.
#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_link_failure_report(
    state: &AppState,
    client: &ClientSession,
    call_id: &CallId,
    peer_id: ClientId,
) {
    match state
        .calls
        .report_link_failure(call_id, client.id(), &peer_id)
    {
        LinkReportOutcome::InvalidReport => {
            // No error reply: reports routinely race the reported peer's own
            // leave, and an error would make the reporter tear down a healthy
            // call. The reporter learns the roster changed via CallUpdate.
            tracing::debug!(
                ?peer_id,
                "Ignoring link failure report for a non-participant pair"
            );
        }
        LinkReportOutcome::Recorded => {
            tracing::debug!(
                ?peer_id,
                "Link failure recorded or already resolved, no eviction"
            );
        }
        LinkReportOutcome::Evicted {
            evicted,
            unreachable,
            actions,
        } => {
            tracing::info!(
                ?evicted,
                ?unreachable,
                "Both endpoints reported the link dead, evicting the later joiner"
            );

            if let Err(err) = state
                .send_message(
                    &evicted,
                    CallError {
                        call_id: *call_id,
                        reason: CallErrorReason::PeerConnectionFailed(unreachable.clone()),
                        message: None,
                    },
                )
                .await
            {
                tracing::warn!(?err, ?evicted, "Failed to send link eviction error");
            }
            if let Err(err) = state
                .send_message(&evicted, CallEnd::new(*call_id, evicted.clone()))
                .await
            {
                tracing::warn!(?err, ?evicted, "Failed to send link eviction call end");
            }

            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );

                        for notified_client in ringing_target.notified_clients {
                            if let Err(err) = state
                                .send_message(&notified_client, cancelled.clone())
                                .await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?notified_client,
                                    "Failed to send call cancelled to notified client"
                                );
                            }
                        }
                    }
                    UpdateCallAction::DropParticipant(_, participant_id) => {
                        if let Err(err) = state
                            .send_message(&participant_id, CallEnd::new(*call_id, evicted.clone()))
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
                        for (participant_id, _) in update.all_participants() {
                            if let Err(err) = state
                                .send_message(
                                    participant_id,
                                    ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_error(state: &AppState, client: &ClientSession, error: CallError) {
    tracing::trace!("Handling call error");
    let erroring_id = client.id();
    let call_id = &error.call_id;

    let reason = match &error.reason {
        CallErrorReason::WebrtcFailure(client_id)
        | CallErrorReason::AudioFailure(client_id)
        | CallErrorReason::SignalingFailure(client_id) => {
            if erroring_id != client_id {
                tracing::debug!("Erroring client ID mismatch, rejecting call error");
                send_call_error(
                    client,
                    call_id,
                    CallErrorReason::Other,
                    Some("Erroring client ID mismatch"),
                )
                .await;
                return;
            }
            error.reason
        }
        CallErrorReason::CallFailure | CallErrorReason::Other => error.reason,
        CallErrorReason::PeerConnectionFailed(peer_id) => {
            handle_link_failure_report(state, client, call_id, peer_id.clone()).await;
            return;
        }
        other => {
            tracing::error!(?other, "Receiving invalid call error reason, rejecting");
            return;
        }
    };

    match state.calls.call_error(call_id, erroring_id) {
        CallTerminationOutcome::CallNotFound => {
            tracing::warn!("No ringing call found, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        }
        CallTerminationOutcome::ClientNotNotified => {
            tracing::warn!("Client was not notified of this call, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        }
        CallTerminationOutcome::Continued => {}
        CallTerminationOutcome::TargetFailed(ringing_targets, update) => {
            tracing::trace!(
                "All notified clients either rejected or errored, call failed, sending call error to source client"
            );

            let Some(caller_id) = ringing_targets.first().map(|r| r.caller_id.clone()) else {
                tracing::error!(
                    "Call error resulted in a failed termination outcome, but ringing targets is empty"
                );
                return;
            };

            cancel_failed_target(
                state,
                call_id,
                ringing_targets,
                CallCancelReason::Errored(reason),
            )
            .await;

            tracing::trace!("Send call update to remaining participants during call error");
            for (participant_id, _) in update.all_participants_without_self(caller_id) {
                if let Err(err) = state
                    .send_message(
                        participant_id,
                        ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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
        CallTerminationOutcome::Changed(actions) => {
            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        tracing::trace!("Cancelling ringing target during call error");
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );

                        for notified_client in ringing_target.notified_clients {
                            tracing::trace!(
                                ?notified_client,
                                "Sending call cancelled to notified client"
                            );
                            if let Err(err) = state
                                .send_message(&notified_client, cancelled.clone())
                                .await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?notified_client,
                                    "Failed to send call cancelled to notified client"
                                );
                            }
                        }
                    }
                    UpdateCallAction::DropParticipant(_, client_id) => {
                        tracing::trace!(?client_id, "Dropping participant during call error");

                        if let Err(err) = state
                            .send_message(
                                &client_id,
                                CallError {
                                    call_id: *call_id,
                                    reason: reason.clone(),
                                    message: None,
                                },
                            )
                            .await
                        {
                            tracing::warn!(
                                ?err,
                                ?client_id,
                                "Failed to send call error to participant"
                            );
                        }

                        if let Err(err) = state
                            .send_message(&client_id, CallEnd::new(*call_id, erroring_id.clone()))
                            .await
                        {
                            tracing::warn!(
                                ?err,
                                ?client_id,
                                "Failed to send call end to participant"
                            );
                        }
                    }
                    UpdateCallAction::UpdateParticipants(updates) => {
                        for (client_id, _) in updates.all_participants() {
                            tracing::trace!(?client_id, "Updating participant during call error");

                            if let Err(err) = state
                                .send_message(
                                    client_id,
                                    CallError {
                                        call_id: *call_id,
                                        reason: reason.clone(),
                                        message: None,
                                    },
                                )
                                .await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?client_id,
                                    "Failed to send call error to participant"
                                );
                            }

                            if let Err(err) = state
                                .send_message(
                                    client_id,
                                    ServerMessage::CallUpdate(updates.for_recipient(client_id)),
                                )
                                .await
                            {
                                tracing::warn!(
                                    ?err,
                                    ?client_id,
                                    "Failed to send call update to participant"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_drop_target(
    state: &AppState,
    client: &ClientSession,
    drop_target: CallDropTarget,
) {
    tracing::trace!("Handling call drop target");
    let dropping_id = client.id();
    let call_id = &drop_target.call_id;

    match state.calls.drop_target(
        call_id,
        dropping_id,
        &drop_target.target,
        drop_target.reason,
    ) {
        DropTargetOutcome::CallNotFound => {
            tracing::debug!("No ringing or active call found, returning call error");
            send_call_error(client, call_id, CallErrorReason::CallNotFound, None).await;
        }
        DropTargetOutcome::NotPermitted => {
            tracing::debug!("Client may not drop this target, returning call error");
            send_call_error(
                client,
                call_id,
                CallErrorReason::NotConferenceLeader(drop_target.target),
                None,
            )
            .await;
        }
        DropTargetOutcome::Obsolete(update) => {
            tracing::debug!("Target drop is now obsolete, sending current call state");
            send_call_update(state, dropping_id, &update).await;
        }
        DropTargetOutcome::RingingTargetCancelled(ringing_target, update) => {
            tracing::trace!("Cancelling dropped ringing target");
            let cancelled = server::CallCancelled::new(
                *call_id,
                HashSet::from([ringing_target.target]),
                match drop_target.reason {
                    CallDropReason::Requested => CallCancelReason::CallerCancelled,
                    CallDropReason::AutoHangup => {
                        CallCancelReason::Errored(CallErrorReason::AutoHangup)
                    }
                },
            );

            for notified_client in ringing_target.notified_clients {
                if let Err(err) = state
                    .send_message(&notified_client, cancelled.clone())
                    .await
                {
                    tracing::warn!(
                        ?err,
                        ?notified_client,
                        "Failed to send call cancelled to notified client"
                    );
                }
            }

            broadcast_call_update(state, &update, dropping_id).await;
        }
        DropTargetOutcome::ParticipantDropped(dropped_client_id, update) => {
            tracing::trace!(?dropped_client_id, "Dropping participant from conference");

            if let Err(err) = state
                .send_message(
                    &dropped_client_id,
                    CallEnd::new(*call_id, dropping_id.clone()),
                )
                .await
            {
                tracing::warn!(
                    ?err,
                    ?dropped_client_id,
                    "Failed to send call end to dropped participant"
                );
            }

            broadcast_call_update(state, &update, dropping_id).await;
        }
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_webrtc_offer(state: &AppState, client: &ClientSession, offer: WebrtcOffer) {
    tracing::trace!("Handling WebRTC offer");
    let client_id = client.id();
    let call_id = &offer.call_id;

    if offer.from_client_id != *client_id {
        tracing::debug!("Source client ID mismatch, rejecting WebRTC offer");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Source client ID mismatch"),
        )
        .await;
        return;
    }

    if !state.calls.has_active_call(call_id, client_id) {
        tracing::debug!("No active call found for WebRTC offer, returning call error");
        send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        return;
    }

    if !state.calls.has_active_call(call_id, &offer.to_client_id) {
        tracing::debug!("Recipient is not a call participant, dropping WebRTC offer");
        return;
    }

    if let Err(err) = state.send_message(&offer.to_client_id, offer.clone()).await {
        tracing::warn!(?err, "Failed to send WebRTC offer to peer");
        send_call_error(
            client,
            call_id,
            CallErrorReason::SignalingFailure(offer.to_client_id),
            None,
        )
        .await;
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_webrtc_answer(state: &AppState, client: &ClientSession, answer: WebrtcAnswer) {
    tracing::trace!("Handling WebRTC answer");
    let client_id = client.id();
    let call_id = &answer.call_id;

    if answer.from_client_id != *client_id {
        tracing::debug!("Source client ID mismatch, rejecting WebRTC answer");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Source client ID mismatch"),
        )
        .await;
        return;
    }

    if !state.calls.has_active_call(call_id, client_id) {
        tracing::debug!("No active call found for WebRTC answer, returning call error");
        send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        return;
    }

    if !state.calls.has_active_call(call_id, &answer.to_client_id) {
        tracing::debug!("Recipient is not a call participant, dropping WebRTC answer");
        return;
    }

    if let Err(err) = state
        .send_message(&answer.to_client_id, answer.clone())
        .await
    {
        tracing::warn!(?err, "Failed to send WebRTC answer to peer");
        send_call_error(
            client,
            call_id,
            CallErrorReason::SignalingFailure(answer.to_client_id),
            None,
        )
        .await;
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_webrtc_ice_candidate(
    state: &AppState,
    client: &ClientSession,
    ice_candidate: WebrtcIceCandidate,
) {
    tracing::trace!("Handling WebRTC ice candidate");
    let client_id = client.id();
    let call_id = &ice_candidate.call_id;

    if ice_candidate.from_client_id != *client_id {
        tracing::debug!("Source client ID mismatch, rejecting WebRTC ice candidate");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Source client ID mismatch"),
        )
        .await;
        return;
    }

    if !state.calls.has_active_call(call_id, client_id) {
        tracing::debug!("No active call found for WebRTC ice candidate, returning call error");
        send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        return;
    }

    if !state
        .calls
        .has_active_call(call_id, &ice_candidate.to_client_id)
    {
        tracing::debug!("Recipient is not a call participant, dropping WebRTC ice candidate");
        return;
    }

    if let Err(err) = state
        .send_message(&ice_candidate.to_client_id, ice_candidate.clone())
        .await
    {
        tracing::warn!(?err, "Failed to send WebRTC ice candidate to peer");
        send_call_error(
            client,
            call_id,
            CallErrorReason::SignalingFailure(ice_candidate.to_client_id),
            None,
        )
        .await;
    }
}

/// Notifies the caller that a ringing target failed as a whole (every notified client
/// rejected, errored, or was unreachable).
async fn cancel_failed_target(
    state: &AppState,
    call_id: &CallId,
    ringing_targets: Vec<RingingTarget>,
    reason: CallCancelReason,
) {
    let Some(caller_id) = ringing_targets.first().map(|r| r.caller_id.clone()) else {
        tracing::error!("Target failed, but ringing targets is empty");
        return;
    };

    let targets = ringing_targets.into_iter().map(|r| r.target).collect();

    if let Err(err) = state
        .send_message(
            &caller_id,
            server::CallCancelled::new(*call_id, targets, reason),
        )
        .await
    {
        tracing::warn!(?err, "Failed to send call cancellation to source client");
    }
}

/// Sends the authoritative membership snapshot to every participant, plus the
/// dropping client itself: a caller that has not joined the call is listed in
/// neither half of the snapshot, and would otherwise never learn that the call
/// it started shrank or ended.
async fn broadcast_call_update(
    state: &AppState,
    update: &UpdateParticipants,
    dropping_id: &ClientId,
) {
    let mut recipients: HashSet<&ClientId> = update.all_participants().map(|(id, _)| id).collect();
    recipients.insert(dropping_id);

    for participant_id in recipients {
        send_call_update(state, participant_id, update).await;
    }
}

async fn send_call_update(
    state: &AppState,
    participant_id: &ClientId,
    update: &UpdateParticipants,
) {
    if let Err(err) = state
        .send_message(
            participant_id,
            ServerMessage::CallUpdate(update.for_recipient(participant_id)),
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

async fn send_call_error(
    client: &ClientSession,
    call_id: &CallId,
    reason: CallErrorReason,
    message: Option<&str>,
) {
    CallMetrics::call_error(&reason);
    if let Err(err) = client
        .send_message(CallError {
            call_id: *call_id,
            reason,
            message: message.map(|m| m.to_string()),
        })
        .await
    {
        tracing::warn!(?err, "Failed to send call error message");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::test_util::{TestSetup, create_client_info};
    use pretty_assertions::{assert_eq, assert_matches};
    use test_log::test;
    use vacs_protocol::vatsim::ClientId;
    use vacs_protocol::ws::server::{self, ServerMessage};

    #[test(tokio::test)]
    async fn handle_application_message_list_clients_without_self() {
        let mut setup = TestSetup::new();
        setup.register_client(create_client_info(1)).await;

        let control_flow = handle_application_message(
            &setup.app_state,
            &setup.session,
            ClientMessage::ListClients,
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));

        let message = setup.rx.recv().await.expect("No message received");
        assert_matches!(
            message,
            ServerMessage::ClientList(server::ClientList { clients }) if clients.is_empty()
        );
    }

    #[test(tokio::test)]
    async fn handle_application_message_list_stations() {
        let mut setup = TestSetup::new();
        setup.register_client(create_client_info(1)).await;

        let control_flow = handle_application_message(
            &setup.app_state,
            &setup.session,
            ClientMessage::ListStations,
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));

        let message = setup.rx.recv().await.expect("No message received");
        assert_matches!(
            message,
            ServerMessage::StationList(server::StationList { stations }) if stations.is_empty()
        );
    }

    #[test(tokio::test)]
    async fn handle_application_message_list_clients() {
        let mut setup = TestSetup::new();
        setup.register_client(create_client_info(1)).await;
        let client_2 = create_client_info(2);
        setup.register_client(client_2.clone()).await;

        let control_flow = handle_application_message(
            &setup.app_state,
            &setup.session,
            ClientMessage::ListClients,
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));

        let message = setup.rx.recv().await.expect("No message received");
        assert_matches!(
            message,
            ServerMessage::ClientList(server::ClientList { clients }) if clients == vec![client_2]
        );
    }

    #[test(tokio::test)]
    async fn handle_application_message_logout() {
        let setup = TestSetup::new();
        setup.register_client(create_client_info(1)).await;

        let control_flow =
            handle_application_message(&setup.app_state, &setup.session, ClientMessage::Logout)
                .await;
        assert_eq!(control_flow, ControlFlow::Break(()));
    }

    #[test(tokio::test)]
    async fn handle_application_message_call_offer() {
        let setup = TestSetup::new();

        let control_flow = handle_application_message(
            &setup.app_state,
            &setup.session,
            ClientMessage::WebrtcOffer(WebrtcOffer {
                call_id: CallId::new(),
                from_client_id: ClientId::from("client1"),
                to_client_id: ClientId::from("client2"),
                sdp: "sdp1".to_string(),
            }),
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));
    }

    #[test(tokio::test)]
    async fn handle_application_message_unknown() {
        let setup = TestSetup::new();

        let control_flow = handle_application_message(
            &setup.app_state,
            &setup.session,
            ClientMessage::Error(vacs_protocol::ws::shared::Error::new(
                ErrorReason::Internal("test".to_string()),
            )),
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));
    }
}
