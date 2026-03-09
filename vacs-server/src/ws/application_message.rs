use crate::metrics::{CallMetrics, ErrorMetrics};
use crate::state::AppState;
use crate::state::calls::{CallTerminationOutcome, StartCallError};
use crate::state::clients::session::ClientSession;
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::sync::Arc;
use vacs_protocol::ws::client::{CallReject, ClientMessage};
use vacs_protocol::ws::server::CallCancelReason;
use vacs_protocol::ws::shared::{
    CallAccept, CallEnd, CallError, CallErrorReason, CallId, CallInvite, CallTarget, ErrorReason,
    WebrtcAnswer, WebrtcIceCandidate, WebrtcOffer,
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

    if let Err(until) = state.rate_limiters().check_call_invite(caller_id) {
        tracing::debug!(?until, "Rate limit exceeded, rejecting call invite");
        let reason = ErrorReason::RateLimited {
            retry_after_secs: until.as_secs(),
        };
        ErrorMetrics::error(&reason);
        client
            .send_error(shared::Error::from(reason).with_call_id(invite.call_id))
            .await;
        return;
    }

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

    let target_clients = match &invite.target {
        CallTarget::Client(client_id) => {
            if state.clients.is_client_connected(client_id).await {
                HashSet::from([client_id.clone()])
            } else {
                HashSet::new()
            }
        }
        CallTarget::Position(position_id) => state.clients.clients_for_position(position_id).await,
        CallTarget::Station(station_id) => state.clients.clients_for_station(station_id).await,
    }
    .into_iter()
    .filter(|client_id| client_id != client.id())
    .collect::<HashSet<_>>();

    CallMetrics::call_invite(&invite.source, &invite.target, invite.prio);

    if target_clients.is_empty() {
        tracing::trace!("No clients found for call invite, returning target not found error");
        send_call_error(client, call_id, CallErrorReason::TargetNotFound, None).await;
        return;
    }

    match state
        .calls
        .start_call_attempt(call_id, client.id(), &invite.target, &target_clients)
    {
        Ok(_) => {}
        Err(StartCallError::CallerBusy) => {
            tracing::debug!("Client already has an outgoing call, rejecting call invite");
            send_call_error(client, call_id, CallErrorReason::CallActive, None).await;
            return;
        }
    }

    for callee_id in target_clients {
        tracing::trace!(?callee_id, "Sending call invite to target");
        if let Err(err) = state.send_message(&callee_id, invite.clone()).await {
            tracing::warn!(?err, ?callee_id, "Failed to send call invite to target");
            if let CallTerminationOutcome::Failed(_) = state.calls.call_error(call_id, &callee_id) {
                tracing::trace!(?callee_id, "All call attempts failed, returning call error");
                send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
                return;
            }
        }
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

    let Some(ringing) = state.calls.accept_call(call_id, answerer_id) else {
        tracing::warn!("No ringing call found, returning call error");
        send_call_error(client, call_id, CallErrorReason::CallFailure, None).await;
        return;
    };

    tracing::trace!("Sending call accept to source client");
    if let Err(err) = state.send_message(&ringing.caller_id, accept.clone()).await {
        tracing::warn!(?err, "Failed to send call accept to source client");
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
        return;
    }

    if ringing.notified_clients.len() > 1 {
        let cancelled = server::CallCancelled::new(
            *call_id,
            CallCancelReason::AnsweredElsewhere(answerer_id.clone()),
        );

        for callee_id in ringing.notified_clients {
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
        CallTerminationOutcome::Failed(ringing) => {
            tracing::trace!(
                "All notified clients either rejected or errored, call failed, sending call error to source client"
            );
            // TODO send CallCancelled to all notified, just in case?
            if let Err(err) = state
                .send_message(
                    &ringing.caller_id,
                    server::CallCancelled::new(*call_id, CallCancelReason::Rejected(reject.reason)),
                )
                .await
            {
                tracing::warn!(?err, "Failed to send call error to source client");
            }
            return;
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

    if let Some(ringing) = state.calls.end_ringing_call(call_id, ender_id) {
        tracing::trace!("Ringing call found, canceling");
        let cancelled = server::CallCancelled::new(*call_id, CallCancelReason::CallerCancelled);

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
    } else if let Some(active) = state.calls.end_active_call(call_id, ender_id) {
        tracing::trace!("Active call found, ending");
        if let Some(peer_id) = active.peer(ender_id) {
            tracing::trace!(?peer_id, "Sending call end to peer");
            if let Err(err) = state.send_message(peer_id, end.clone()).await {
                tracing::warn!(?err, ?peer_id, "Failed to send call end to peer");
                send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
            }
        } else {
            tracing::warn!("No peer found for active call, returning call error");
            send_call_error(client, call_id, CallErrorReason::TargetNotFound, None).await;
            return;
        }
    } else {
        tracing::trace!("No ringing or active call found, returning call error");
        send_call_error(client, call_id, CallErrorReason::TargetNotFound, None).await;
        return;
    }
}

#[tracing::instrument(level = "trace", skip(state, client))]
async fn handle_call_error(state: &AppState, client: &ClientSession, error: CallError) {
    tracing::trace!("Handling call error");
    let erroring_id = client.id();
    let call_id = &error.call_id;

    match state.calls.call_error(call_id, erroring_id) {
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
        CallTerminationOutcome::Failed(ringing) => {
            tracing::trace!(
                "All notified clients either rejected or errored, call failed, sending call error to source client"
            );
            // TODO send CallCancelled to all notified, just in case?
            if let Err(err) = state
                .send_message(
                    &ringing.caller_id,
                    server::CallCancelled::new(*call_id, CallCancelReason::Errored(error.reason)),
                )
                .await
            {
                tracing::warn!(?err, "Failed to send call error to source client");
            }
            return;
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
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
        return;
    }

    if let Err(err) = state.send_message(&offer.to_client_id, offer.clone()).await {
        tracing::warn!(?err, "Failed to send WebRTC offer to peer");
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
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
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
        return;
    }

    if let Err(err) = state
        .send_message(&answer.to_client_id, answer.clone())
        .await
    {
        tracing::warn!(?err, "Failed to send WebRTC answer to peer");
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
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
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
        return;
    }

    if let Err(err) = state
        .send_message(&ice_candidate.to_client_id, ice_candidate.clone())
        .await
    {
        tracing::warn!(?err, "Failed to send WebRTC ice candidate to peer");
        send_call_error(client, call_id, CallErrorReason::SignalingFailure, None).await;
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
