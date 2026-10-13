use crate::metrics::labels::AsMetricLabel;
use crate::metrics::{CallMetrics, ErrorMetrics};
use crate::ratelimit::CallInviteRejection;
use crate::state::AppState;
use crate::state::calls::{
    AcceptCallOutcome, CallTerminationOutcome, DropTargetOutcome, LinkReportOutcome, RingingTarget,
    StartCallError, UpdateCallAction, UpdateParticipants, log_call_teardown, log_target_cancelled,
    peer_ids_without,
};
use crate::state::clients::session::ClientSession;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;
use tracing::Instrument;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::{
    CallAccept, CallDropReason, CallDropTarget, CallInvite, CallReject, CallRejectReason,
    ClientMessage,
};
use vacs_protocol::ws::server::{CallCancelReason, CallInvitation, ServerMessage};
use vacs_protocol::ws::shared::{
    CallEnd, CallError, CallErrorReason, CallId, CallTarget, ErrorReason, WebrtcAnswer,
    WebrtcIceCandidate, WebrtcOffer,
};
use vacs_protocol::ws::{server, shared};

pub async fn handle_application_message(
    state: &Arc<AppState>,
    client: &ClientSession,
    message: ClientMessage,
) -> ControlFlow<(), ()> {
    let kind = message.as_metric_label();
    tracing::trace!(kind, "Received message");

    let party_span = |call_id: &CallId, is_party: bool| {
        let span = tracing::info_span!("call_message", kind, call_id = tracing::field::Empty);
        if is_party {
            span.record("call_id", tracing::field::display(call_id));
        }
        span
    };
    let call_span =
        |call_id: &CallId| party_span(call_id, state.calls.is_party(call_id, client.id()));

    match message {
        ClientMessage::ListClients => {
            let clients = state.list_clients(Some(client.id())).await;
            if let Err(err) = client.send_message(server::ClientList { clients }).await {
                tracing::debug!(%err, "Failed to send client list");
            }
        }
        ClientMessage::ListStations => {
            let stations = state
                .clients
                .list_stations(client.active_profile(), client.position_id())
                .await;
            if let Err(err) = client.send_message(server::StationList { stations }).await {
                tracing::debug!(%err, "Failed to send station list");
            }
        }
        ClientMessage::CallInvite(call_invite) => {
            let is_party = state.calls.is_party(&call_invite.call_id, client.id());
            let span = party_span(&call_invite.call_id, is_party);
            handle_call_invite(state, client, call_invite, is_party)
                .instrument(span)
                .await;
        }
        ClientMessage::CallAccept(call_accept) => {
            let span = call_span(&call_accept.call_id);
            handle_call_accept(state, client, call_accept)
                .instrument(span)
                .await;
        }
        ClientMessage::CallReject(call_reject) => {
            let span = call_span(&call_reject.call_id);
            handle_call_reject(state, client, call_reject)
                .instrument(span)
                .await;
        }
        ClientMessage::CallEnd(call_end) => {
            let span = call_span(&call_end.call_id);
            handle_call_end(state, client, call_end)
                .instrument(span)
                .await;
        }
        ClientMessage::CallError(call_error) => {
            let span = call_span(&call_error.call_id);
            handle_call_error(state, client, call_error)
                .instrument(span)
                .await;
        }
        ClientMessage::CallDropTarget(call_drop_target) => {
            let span = call_span(&call_drop_target.call_id);
            handle_call_drop_target(state, client, call_drop_target)
                .instrument(span)
                .await;
        }
        ClientMessage::WebrtcOffer(webrtc_offer) => {
            let span = call_span(&webrtc_offer.call_id);
            handle_webrtc_offer(state, client, webrtc_offer)
                .instrument(span)
                .await;
        }
        ClientMessage::WebrtcAnswer(webrtc_answer) => {
            let span = call_span(&webrtc_answer.call_id);
            handle_webrtc_answer(state, client, webrtc_answer)
                .instrument(span)
                .await;
        }
        ClientMessage::WebrtcIceCandidate(webrtc_ice_candidate) => {
            let span = call_span(&webrtc_ice_candidate.call_id);
            handle_webrtc_ice_candidate(state, client, webrtc_ice_candidate)
                .instrument(span)
                .await;
        }
        ClientMessage::Logout | ClientMessage::Disconnect => return ControlFlow::Break(()),
        ClientMessage::Login(_) | ClientMessage::Error(_) => {}
    };
    ControlFlow::Continue(())
}

/// Puts a call the invite just created on the `call_message` span. The dispatcher records calls
/// that already exist.
fn record_call_id(call_id: &CallId) {
    tracing::Span::current().record("call_id", tracing::field::display(call_id));
}

fn invite_rejected(reason: &str) {
    tracing::info!(reason, "Call invite rejected");
}

fn message_rejected(reason: &str) {
    tracing::info!(reason, "Call message rejected");
}

fn reject_reason_label(reason: &CallRejectReason) -> &'static str {
    match reason {
        CallRejectReason::Busy => "busy",
        CallRejectReason::Unknown(_) => "unknown",
    }
}

fn drop_reason_label(reason: CallDropReason) -> &'static str {
    match reason {
        CallDropReason::Requested => "requested",
        CallDropReason::AutoHangup => "auto_hangup",
    }
}

async fn handle_call_invite(
    state: &AppState,
    client: &ClientSession,
    invite: CallInvite,
    mut call_recorded: bool,
) {
    let caller_id = client.id();
    let call_id = &invite.call_id;

    if invite.source.client_id != *caller_id {
        invite_rejected("client_id_mismatch");
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
        invite_rejected("no_targets");
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
            tracing::debug!(
                call_target = target.as_metric_label(),
                "Call target has no clients, skipping target"
            );
            not_found_targets.insert(target.clone());
            continue;
        }

        resolved_targets.push((target.clone(), target_clients));
    }

    if resolved_targets.is_empty() {
        let reason = CallErrorReason::TargetsNotFound(not_found_targets);
        invite_rejected(reason.as_metric_label());
        send_call_error(client, call_id, reason, None).await;
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
        let reason = CallErrorReason::MaxConferenceSizeReached(invite.targets);
        invite_rejected(reason.as_metric_label());
        send_call_error(client, call_id, reason, None).await;
        return;
    }

    match state
        .rate_limiters()
        .check_call_invite(caller_id, resolved_targets.len())
    {
        Ok(()) => {}
        Err(CallInviteRejection::RateLimited(until)) => {
            let reason = ErrorReason::RateLimited {
                targets: invite.targets,
                retry_after_secs: until.as_secs(),
            };
            tracing::info!(
                reason = reason.as_metric_label(),
                retry_after_secs = until.as_secs(),
                "Call invite rejected"
            );
            ErrorMetrics::error(&reason);
            client
                .send_error(shared::Error::from(reason).with_call_id(invite.call_id))
                .await;
            return;
        }
        Err(CallInviteRejection::TooManyTargets) => {
            invite_rejected("too_many_targets");
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
                if !call_recorded {
                    record_call_id(call_id);
                    call_recorded = true;
                }
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
                let reason = CallErrorReason::CallActive;
                invite_rejected(reason.as_metric_label());
                send_call_error(client, call_id, reason, None).await;
                return;
            }
            Err(StartCallError::NotParticipant) => {
                let reason = CallErrorReason::NotParticipant;
                invite_rejected(reason.as_metric_label());
                send_call_error(client, call_id, reason, None).await;
                return;
            }
            Err(StartCallError::AlreadyParticipant) => {
                let reason = CallErrorReason::AlreadyParticipant(target.clone());
                tracing::info!(
                    reason = reason.as_metric_label(),
                    call_target = target.as_metric_label(),
                    "Call invite rejected"
                );
                send_call_error(client, call_id, reason, None).await;
                continue;
            }
            Err(StartCallError::NotConferenceLeader) => {
                let reason = CallErrorReason::NotConferenceLeader(target.clone());
                tracing::info!(
                    reason = reason.as_metric_label(),
                    call_target = target.as_metric_label(),
                    "Call invite rejected"
                );
                send_call_error(client, call_id, reason, None).await;
                continue;
            }
        }
    }

    if !not_found_targets.is_empty() {
        let reason = CallErrorReason::TargetsNotFound(not_found_targets);
        invite_rejected(reason.as_metric_label());
        send_call_error(client, call_id, reason, None).await;

        if invited_participants.is_empty() && joined_participants.is_empty() {
            return;
        }
    }

    if !all_target_participants.is_empty() {
        tracing::info!(
            peer_ids = %crate::logging::peer_ids(all_target_participants.keys()),
            prio = invite.prio,
            "Call invite sent"
        );
    }

    let mut failed_targets: HashSet<&CallTarget> = HashSet::new();

    let invited_targets: HashSet<CallTarget> = invited_participants.values().cloned().collect();
    let conference_leader = state
        .calls
        .active_call(call_id)
        .and_then(|active_call| active_call.conference_leader);

    for (callee_id, target) in &all_target_participants {
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
            tracing::debug!(
                peer_ids = %callee_id,
                reason = err.reason.as_metric_label(),
                "Failed to send call invite to target"
            );
            match state.calls.call_error(call_id, callee_id) {
                CallTerminationOutcome::Continued => {}
                CallTerminationOutcome::TargetFailed(ringing_targets, _) => {
                    for ringing_target in &ringing_targets {
                        log_target_cancelled(caller_id, ringing_target, "signaling_failure");
                    }
                    failed_targets.insert(target);
                    cancel_failed_target(
                        state,
                        call_id,
                        ringing_targets,
                        CallCancelReason::Errored(CallErrorReason::CallFailure),
                    )
                    .await;
                }
                _ => {
                    tracing::error!(
                        peer_ids = %callee_id,
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

        send_call_update(state, participant_id, &update).await;
    }
}

async fn handle_call_accept(state: &AppState, client: &ClientSession, accept: CallAccept) {
    let answerer_id = client.id();
    let call_id = &accept.call_id;

    if accept.accepting_client_id != *answerer_id {
        message_rejected("client_id_mismatch");
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
            send_call_error(client, call_id, CallErrorReason::CallActive, None).await;

            let outcome = state.calls.fail_ringing_recipient(
                call_id,
                answerer_id,
                CallErrorReason::CallActive,
            );
            message_rejected(CallErrorReason::CallActive.as_metric_label());

            if let CallTerminationOutcome::TargetFailed(ringing_targets, update) = outcome {
                for ringing_target in &ringing_targets {
                    log_target_cancelled(
                        answerer_id,
                        ringing_target,
                        CallErrorReason::CallActive.as_metric_label(),
                    );
                }
                fail_ringing_targets(
                    state,
                    call_id,
                    ringing_targets,
                    update,
                    CallErrorReason::CallActive,
                )
                .await;
            }
            return;
        }
        AcceptCallOutcome::NotFound => {
            tracing::debug!(reason = "call_not_found", "Call message rejected");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
            return;
        }
    };

    tracing::info!(
        peer_ids = peer_ids_without(update.joined_participants.keys(), answerer_id).as_deref(),
        "Call accepted"
    );

    for participant_id in update.invited_participants.keys() {
        send_call_update(state, participant_id, &update).await;
    }

    for participant_id in update.joined_participants.keys() {
        if let Err(err) = state
            .send_message(
                participant_id,
                ServerMessage::CallUpdate(update.for_recipient(participant_id)),
            )
            .await
        {
            tracing::debug!(
                peer_ids = %participant_id,
                reason = err.reason.as_metric_label(),
                "Failed to send call acceptance to participant"
            );

            let duration = state.calls.call_duration(call_id);
            let Some(actions) = state.calls.end_call(call_id, participant_id) else {
                tracing::error!(
                    peer_ids = %participant_id,
                    "Tried to send a call acceptance message to a participant, which is not a participant anymore"
                );
                continue;
            };
            log_call_teardown(
                answerer_id,
                participant_id,
                &actions,
                duration,
                "signaling_failure",
            );

            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );
                        send_call_cancelled(state, ringing_target.notified_clients, &cancelled)
                            .await;
                    }
                    UpdateCallAction::DropParticipant(_, dropped_participant_id) => {
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
                            tracing::debug!(
                                peer_ids = %dropped_participant_id,
                                reason = err.reason.as_metric_label(),
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
                            tracing::debug!(
                                peer_ids = %dropped_participant_id,
                                reason = err.reason.as_metric_label(),
                                "Failed to send call end to participant"
                            );
                        }
                    }
                    UpdateCallAction::UpdateParticipants(update) => {
                        for (participant_id, _) in update.all_participants() {
                            send_call_update(state, participant_id, &update).await;
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
        let others = accepted_target
            .notified_clients
            .into_iter()
            .filter(|callee_id| callee_id != answerer_id);
        send_call_cancelled(state, others, &cancelled).await;
    }
}

async fn handle_call_reject(state: &AppState, client: &ClientSession, reject: CallReject) {
    let rejecter_id = client.id();
    let call_id = &reject.call_id;

    if reject.rejecting_client_id != *rejecter_id {
        message_rejected("client_id_mismatch");
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
            tracing::debug!(reason = "call_not_found", "Call message rejected");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        }
        CallTerminationOutcome::ClientNotNotified => {
            tracing::debug!(reason = "not_notified", "Call message rejected");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        }
        CallTerminationOutcome::Continued => {
            tracing::info!(
                reason = reject_reason_label(&reject.reason),
                "Call rejected"
            );
        }
        CallTerminationOutcome::TargetFailed(ringing_targets, update) => {
            tracing::info!(
                reason = reject_reason_label(&reject.reason),
                "Call rejected"
            );
            for ringing_target in &ringing_targets {
                log_target_cancelled(rejecter_id, ringing_target, "rejected");
            }

            cancel_failed_target(
                state,
                call_id,
                ringing_targets,
                CallCancelReason::Rejected(reject.reason),
            )
            .await;

            for (participant_id, _) in update.all_participants() {
                send_call_update(state, participant_id, &update).await;
            }
        }
        CallTerminationOutcome::Changed(actions) => {
            tracing::error!(
                actions = actions.len(),
                "Ignoring unexpected update call actions after rejecting call"
            );
        }
    }
}

async fn handle_call_end(state: &AppState, client: &ClientSession, end: CallEnd) {
    let ender_id = client.id();
    let call_id = &end.call_id;

    if end.ending_client_id != *ender_id {
        message_rejected("client_id_mismatch");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Ending client ID mismatch"),
        )
        .await;
        return;
    }

    let duration = state.calls.call_duration(call_id);
    let Some(actions) = state.calls.end_call(call_id, ender_id) else {
        tracing::debug!(reason = "call_not_found", "Call message rejected");
        send_call_error(client, call_id, CallErrorReason::CallNotFound, None).await;
        return;
    };

    log_call_teardown(ender_id, ender_id, &actions, duration, "hangup");

    for action in actions {
        match action {
            UpdateCallAction::CancelRingingTarget(ringing_target) => {
                let cancelled = server::CallCancelled::new(
                    *call_id,
                    HashSet::from([ringing_target.target]),
                    CallCancelReason::CallerCancelled,
                );
                send_call_cancelled(state, ringing_target.notified_clients, &cancelled).await;
            }
            UpdateCallAction::DropParticipant(_, participant_id) => {
                if let Err(err) = state.send_message(&participant_id, end.clone()).await {
                    tracing::debug!(
                        peer_ids = %participant_id,
                        reason = err.reason.as_metric_label(),
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
                for (participant_id, _) in update.all_participants() {
                    send_call_update(state, participant_id, &update).await;
                }
            }
        }
    }
}

/// Handles a dead-link report. A single report is only recorded; once both
/// endpoints of the pair have reported, the later joiner is evicted: it
/// receives the reason naming the peer it could not reach followed by a
/// `CallEnd`, while the remaining participants get the regular leave fan-out.
async fn handle_link_failure_report(
    state: &AppState,
    client: &ClientSession,
    call_id: &CallId,
    peer_id: ClientId,
) {
    let duration = state.calls.call_duration(call_id);
    match state
        .calls
        .report_link_failure(call_id, client.id(), &peer_id)
    {
        LinkReportOutcome::InvalidReport => {
            // No error reply: reports routinely race the reported peer's own
            // leave, and an error would make the reporter tear down a healthy
            // call. The reporter learns the roster changed via CallUpdate.
            tracing::debug!("Ignoring link failure report for a non-participant pair");
        }
        LinkReportOutcome::Recorded => {
            tracing::info!(peer_ids = %peer_id, "Call link failure reported");
        }
        LinkReportOutcome::Evicted {
            evicted,
            unreachable,
            actions,
        } => {
            tracing::info!(
                peer_ids = peer_ids_without([&evicted, &unreachable], client.id()).as_deref(),
                "Call link failure confirmed, evicting the later joiner"
            );
            log_call_teardown(
                client.id(),
                &evicted,
                &actions,
                duration,
                CallErrorReason::PeerConnectionFailed(unreachable.clone()).as_metric_label(),
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
                tracing::debug!(
                    peer_ids = %evicted,
                    reason = err.reason.as_metric_label(),
                    "Failed to send link eviction error"
                );
            }
            if let Err(err) = state
                .send_message(&evicted, CallEnd::new(*call_id, evicted.clone()))
                .await
            {
                tracing::debug!(
                    peer_ids = %evicted,
                    reason = err.reason.as_metric_label(),
                    "Failed to send link eviction call end"
                );
            }

            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );
                        send_call_cancelled(state, ringing_target.notified_clients, &cancelled)
                            .await;
                    }
                    UpdateCallAction::DropParticipant(_, participant_id) => {
                        if let Err(err) = state
                            .send_message(&participant_id, CallEnd::new(*call_id, evicted.clone()))
                            .await
                        {
                            tracing::debug!(
                                peer_ids = %participant_id,
                                reason = err.reason.as_metric_label(),
                                "Failed to send call end to participant"
                            );
                        }
                    }
                    UpdateCallAction::UpdateParticipants(update) => {
                        for (participant_id, _) in update.all_participants() {
                            send_call_update(state, participant_id, &update).await;
                        }
                    }
                }
            }
        }
    }
}

async fn handle_call_error(state: &AppState, client: &ClientSession, error: CallError) {
    let erroring_id = client.id();
    let call_id = &error.call_id;

    let reason = match &error.reason {
        CallErrorReason::WebrtcFailure(client_id)
        | CallErrorReason::AudioFailure(client_id)
        | CallErrorReason::SignalingFailure(client_id) => {
            if erroring_id != client_id {
                message_rejected("client_id_mismatch");
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
            message_rejected(other.as_metric_label());
            return;
        }
    };

    let duration = state.calls.call_duration(call_id);
    match state.calls.call_error(call_id, erroring_id) {
        CallTerminationOutcome::CallNotFound => {
            tracing::debug!(reason = "call_not_found", "Call message rejected");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        }
        CallTerminationOutcome::ClientNotNotified => {
            tracing::debug!(reason = "not_notified", "Call message rejected");
            send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        }
        CallTerminationOutcome::Continued => {
            tracing::info!(reason = reason.as_metric_label(), "Call errored");
        }
        CallTerminationOutcome::TargetFailed(ringing_targets, update) => {
            tracing::info!(reason = reason.as_metric_label(), "Call errored");
            for ringing_target in &ringing_targets {
                log_target_cancelled(erroring_id, ringing_target, reason.as_metric_label());
            }
            fail_ringing_targets(state, call_id, ringing_targets, update, reason).await;
        }
        CallTerminationOutcome::Changed(actions) => {
            log_call_teardown(
                erroring_id,
                erroring_id,
                &actions,
                duration,
                reason.as_metric_label(),
            );

            for action in actions {
                match action {
                    UpdateCallAction::CancelRingingTarget(ringing_target) => {
                        let cancelled = server::CallCancelled::new(
                            *call_id,
                            HashSet::from([ringing_target.target]),
                            CallCancelReason::CallerCancelled,
                        );
                        send_call_cancelled(state, ringing_target.notified_clients, &cancelled)
                            .await;
                    }
                    UpdateCallAction::DropParticipant(_, client_id) => {
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
                            tracing::debug!(
                                peer_ids = %client_id,
                                reason = err.reason.as_metric_label(),
                                "Failed to send call error to participant"
                            );
                        }

                        if let Err(err) = state
                            .send_message(&client_id, CallEnd::new(*call_id, erroring_id.clone()))
                            .await
                        {
                            tracing::debug!(
                                peer_ids = %client_id,
                                reason = err.reason.as_metric_label(),
                                "Failed to send call end to participant"
                            );
                        }
                    }
                    UpdateCallAction::UpdateParticipants(updates) => {
                        // Survivors only ever learn about the erroring client, never a
                        // call-scoped reason, which their clients treat as their own
                        // call failing.
                        let survivor_error = matches!(
                            reason,
                            CallErrorReason::WebrtcFailure(_)
                                | CallErrorReason::AudioFailure(_)
                                | CallErrorReason::SignalingFailure(_)
                        )
                        .then(|| CallError {
                            call_id: *call_id,
                            reason: reason.clone(),
                            message: None,
                        });

                        for (client_id, _) in updates.all_participants() {
                            if let Some(error) = survivor_error.clone()
                                && let Err(err) = state.send_message(client_id, error).await
                            {
                                tracing::debug!(
                                    peer_ids = %client_id,
                                    reason = err.reason.as_metric_label(),
                                    "Failed to send call error to participant"
                                );
                            }

                            send_call_update(state, client_id, &updates).await;
                        }
                    }
                }
            }
        }
    }
}

async fn handle_call_drop_target(
    state: &AppState,
    client: &ClientSession,
    drop_target: CallDropTarget,
) {
    let dropping_id = client.id();
    let call_id = &drop_target.call_id;
    let drop_reason = drop_reason_label(drop_target.reason);

    match state.calls.drop_target(
        call_id,
        dropping_id,
        &drop_target.target,
        drop_target.reason,
    ) {
        DropTargetOutcome::CallNotFound => {
            tracing::debug!(reason = "call_not_found", "Call message rejected");
            send_call_error(client, call_id, CallErrorReason::CallNotFound, None).await;
        }
        DropTargetOutcome::NotPermitted => {
            let call_target = drop_target.target.as_metric_label();
            let reason = CallErrorReason::NotConferenceLeader(drop_target.target);
            tracing::info!(
                reason = reason.as_metric_label(),
                call_target,
                "Call message rejected"
            );
            send_call_error(client, call_id, reason, None).await;

            if let Some(update) = state.calls.call_state_update(call_id, dropping_id) {
                send_call_update(state, dropping_id, &update).await;
            }
        }
        DropTargetOutcome::Obsolete(update) => {
            tracing::debug!("Target drop is now obsolete, sending current call state");
            send_call_update(state, dropping_id, &update).await;
        }
        DropTargetOutcome::RingingTargetCancelled(ringing_target, update) => {
            log_target_cancelled(dropping_id, &ringing_target, drop_reason);

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
            send_call_cancelled(state, ringing_target.notified_clients, &cancelled).await;

            broadcast_call_update(state, &update, dropping_id).await;
        }
        DropTargetOutcome::ParticipantDropped(dropped_client_id, update) => {
            tracing::info!(
                reason = drop_reason,
                peer_ids = %dropped_client_id,
                "Call participant dropped"
            );

            if let Err(err) = state
                .send_message(
                    &dropped_client_id,
                    CallEnd::new(*call_id, dropping_id.clone()),
                )
                .await
            {
                tracing::debug!(
                    peer_ids = %dropped_client_id,
                    reason = err.reason.as_metric_label(),
                    "Failed to send call end to dropped participant"
                );
            }

            broadcast_call_update(state, &update, dropping_id).await;
        }
    }
}

async fn check_webrtc_relay(
    state: &AppState,
    client: &ClientSession,
    call_id: &CallId,
    from_client_id: &ClientId,
    to_client_id: &ClientId,
) -> bool {
    let client_id = client.id();

    if from_client_id != client_id {
        message_rejected("client_id_mismatch");
        send_call_error(
            client,
            call_id,
            CallErrorReason::Other,
            Some("Source client ID mismatch"),
        )
        .await;
        return false;
    }

    if !state.calls.has_active_call(call_id, client_id) {
        tracing::debug!(reason = "call_not_found", "Call message rejected");
        send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        return false;
    }

    if !state.calls.has_active_call(call_id, to_client_id) {
        tracing::debug!("Recipient is not a call participant, dropping WebRTC message");
        return false;
    }

    true
}

async fn relay_webrtc_message(
    state: &AppState,
    client: &ClientSession,
    call_id: &CallId,
    to_client_id: &ClientId,
    bytes: usize,
    message: impl Into<ServerMessage>,
) {
    tracing::trace!(bytes, peer_ids = %to_client_id, "Relaying WebRTC message");

    if let Err(err) = state.send_message(to_client_id, message).await {
        tracing::debug!(
            peer_ids = %to_client_id,
            reason = err.reason.as_metric_label(),
            "Failed to relay WebRTC message"
        );
        send_call_error(
            client,
            call_id,
            CallErrorReason::SignalingFailure(to_client_id.clone()),
            None,
        )
        .await;
    }
}

async fn handle_webrtc_offer(state: &AppState, client: &ClientSession, offer: WebrtcOffer) {
    let call_id = offer.call_id;
    let to_client_id = offer.to_client_id.clone();
    if !check_webrtc_relay(
        state,
        client,
        &call_id,
        &offer.from_client_id,
        &to_client_id,
    )
    .await
    {
        return;
    }
    let bytes = offer.sdp.len();
    relay_webrtc_message(state, client, &call_id, &to_client_id, bytes, offer).await;
}

async fn handle_webrtc_answer(state: &AppState, client: &ClientSession, answer: WebrtcAnswer) {
    let call_id = answer.call_id;
    let to_client_id = answer.to_client_id.clone();
    if !check_webrtc_relay(
        state,
        client,
        &call_id,
        &answer.from_client_id,
        &to_client_id,
    )
    .await
    {
        return;
    }
    let bytes = answer.sdp.len();
    relay_webrtc_message(state, client, &call_id, &to_client_id, bytes, answer).await;
}

async fn handle_webrtc_ice_candidate(
    state: &AppState,
    client: &ClientSession,
    ice_candidate: WebrtcIceCandidate,
) {
    let call_id = ice_candidate.call_id;
    let to_client_id = ice_candidate.to_client_id.clone();
    if !check_webrtc_relay(
        state,
        client,
        &call_id,
        &ice_candidate.from_client_id,
        &to_client_id,
    )
    .await
    {
        return;
    }
    let bytes = ice_candidate.candidate.len();
    relay_webrtc_message(state, client, &call_id, &to_client_id, bytes, ice_candidate).await;
}

/// Cancels targets whose notified clients all rejected or errored and tells the
/// remaining participants about the shrunken call.
async fn fail_ringing_targets(
    state: &AppState,
    call_id: &CallId,
    ringing_targets: Vec<RingingTarget>,
    update: UpdateParticipants,
    reason: CallErrorReason,
) {
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

    for (participant_id, _) in update.all_participants_without_self(caller_id) {
        send_call_update(state, participant_id, &update).await;
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
        tracing::debug!(
            peer_ids = %caller_id,
            reason = err.reason.as_metric_label(),
            "Failed to send call cancellation to source client"
        );
    }
}

async fn send_call_cancelled(
    state: &AppState,
    recipients: impl IntoIterator<Item = ClientId>,
    cancelled: &server::CallCancelled,
) {
    for recipient_id in recipients {
        if let Err(err) = state.send_message(&recipient_id, cancelled.clone()).await {
            tracing::debug!(
                peer_ids = %recipient_id,
                reason = err.reason.as_metric_label(),
                "Failed to send call cancelled to notified client"
            );
        }
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
        tracing::debug!(
            peer_ids = %participant_id,
            reason = err.reason.as_metric_label(),
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
        tracing::debug!(%err, "Failed to send call error message");
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

    #[derive(Clone, Default)]
    struct RecordedCallIds(Arc<parking_lot::Mutex<Vec<String>>>);

    impl tracing::field::Visit for RecordedCallIds {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "call_id" {
                self.0.lock().push(format!("{value:?}"));
            }
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for RecordedCallIds {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _: &tracing::span::Id,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            attrs.record(&mut self.clone());
        }

        fn on_record(
            &self,
            _: &tracing::span::Id,
            values: &tracing::span::Record<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            values.record(&mut self.clone());
        }
    }

    #[test(tokio::test)]
    async fn call_message_carries_the_call_id_only_for_a_party() {
        use tracing_subscriber::layer::SubscriberExt;

        let setup = TestSetup::new();
        let own = setup.session.id().clone();
        let caller = ClientId::from("client2");
        let callee = ClientId::from("client3");
        let ring = |call_id: &CallId, caller: &ClientId, callee: &ClientId| {
            setup
                .app_state
                .calls
                .attempt_call(
                    call_id,
                    caller,
                    &vacs_protocol::ws::shared::CallSource {
                        client_id: caller.clone(),
                        position_id: None,
                        station_id: None,
                    },
                    &CallTarget::Client(callee.clone()),
                    &std::collections::HashSet::from([callee.clone()]),
                )
                .expect("call attempt should succeed");
        };
        let foreign_call = CallId::new();
        ring(&foreign_call, &caller, &callee);
        let own_call = CallId::new();
        ring(&own_call, &own, &callee);

        let recorded = RecordedCallIds::default();
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(recorded.clone()));

        for message in [
            ClientMessage::CallEnd(CallEnd::new(foreign_call, own.clone())),
            ClientMessage::WebrtcIceCandidate(WebrtcIceCandidate {
                call_id: foreign_call,
                from_client_id: own.clone(),
                to_client_id: caller.clone(),
                candidate: "candidate".to_string(),
            }),
            ClientMessage::CallEnd(CallEnd::new(own_call, own.clone())),
        ] {
            let control_flow =
                handle_application_message(&setup.app_state, &setup.session, message).await;
            assert_eq!(control_flow, ControlFlow::Continue(()));
        }

        assert_eq!(*recorded.0.lock(), vec![own_call.to_string()]);
    }

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
    async fn handle_application_message_call_accept_with_unreachable_caller() {
        let setup = TestSetup::new();
        let (caller, caller_rx) = setup.register_client(create_client_info(1)).await;
        let (callee, mut callee_rx) = setup.register_client(create_client_info(2)).await;

        let call_id = CallId::new();
        let control_flow = handle_application_message(
            &setup.app_state,
            &caller,
            ClientMessage::CallInvite(CallInvite {
                call_id,
                source: shared::CallSource {
                    client_id: caller.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(callee.id().clone())]),
                prio: false,
            }),
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));

        // The caller's session ended between the invite and the acceptance, so the server can no
        // longer reach it
        drop(caller_rx);

        let control_flow = handle_application_message(
            &setup.app_state,
            &callee,
            ClientMessage::CallAccept(CallAccept {
                call_id,
                accepting_client_id: callee.id().clone(),
            }),
        )
        .await;
        assert_eq!(control_flow, ControlFlow::Continue(()));

        assert_eq!(
            setup.app_state.calls.active_call_count(),
            0,
            "A call the caller was never told about must not stay active"
        );
        let messages: Vec<_> = std::iter::from_fn(|| callee_rx.try_recv().ok()).collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, ServerMessage::CallError(_))),
            "Callee should receive a call error, got {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, ServerMessage::CallEnd(_))),
            "Callee should be told the call ended, got {messages:?}"
        );
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
