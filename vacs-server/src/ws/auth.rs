use crate::metrics::{ClientMetrics, ErrorMetrics, ProfileMetrics, VatsimSyncMetrics};
use crate::state::AppState;
use crate::ws::message::{MessageResult, receive_message, send_message_raw};
use axum::extract::ws;
use axum::extract::ws::WebSocket;
use futures_util::stream::{SplitSink, SplitStream};
use semver::Version;
use std::sync::Arc;
use std::time::Duration;
use tracing::instrument;
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{ClientId, PositionId};
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{ClientInfo, LoginFailureReason};
use vacs_protocol::ws::shared::ErrorReason;
use vacs_protocol::ws::{server, shared};
use vacs_vatsim::{ControllerInfo, FacilityType};

#[instrument(level = "debug", skip_all)]
pub async fn handle_websocket_login(
    state: Arc<AppState>,
    websocket_receiver: &mut SplitStream<WebSocket>,
    websocket_sender: &mut SplitSink<WebSocket, ws::Message>,
) -> Option<(ClientInfo, ActiveProfile<ProfileId>)> {
    tracing::trace!("Handling websocket login flow");

    let result = tokio::time::timeout(Duration::from_millis(state.config.auth.login_flow_timeout_millis), async {
        loop {
            match receive_message(websocket_receiver).await {
                MessageResult::ApplicationMessage(ClientMessage::Login (login)) => {
                    return process_login_request(&state, &login.token, &login.protocol_version, login.custom_profile, login.position_id).await;
                }
                MessageResult::ApplicationMessage(message) => {
                    tracing::debug!(msg = ?message, "Received unexpected message during websocket login flow");
                    return Err(LoginOutcome::Failure(LoginFailureReason::Unauthorized));
                }
                MessageResult::ControlMessage => {
                    tracing::trace!("Skipping control message during websocket login flow");
                    continue;
                }
                MessageResult::Disconnected => {
                    tracing::debug!("Client disconnected during websocket login flow");
                    return Err(LoginOutcome::Disconnected);
                }
                MessageResult::Error(err) => {
                    tracing::warn!(?err, "Received error while handling websocket login flow");
                    return Err(LoginOutcome::Disconnected);
                }
            };
        }
    }).await;

    match result {
        Ok(Ok((client_info, active_profile))) => Some((client_info, active_profile)),
        Ok(Err(outcome)) => {
            handle_login_outcome(websocket_sender, outcome).await;
            None
        }
        Err(_) => {
            tracing::debug!("Websocket login flow timed out");
            handle_login_outcome(
                websocket_sender,
                LoginOutcome::Failure(LoginFailureReason::Timeout),
            )
            .await;
            None
        }
    }
}

enum LoginOutcome {
    Failure(LoginFailureReason),
    Error(ErrorReason),
    Disconnected,
}

#[instrument(skip(state, token), level = "debug")]
async fn process_login_request(
    state: &Arc<AppState>,
    token: &str,
    protocol_version: &str,
    custom_profile: bool,
    position_id: Option<PositionId>,
) -> Result<(ClientInfo, ActiveProfile<ProfileId>), LoginOutcome> {
    if !is_protocol_compatible(state, protocol_version) {
        tracing::debug!("Websocket login flow failed, due to incompatible protocol version");
        return Err(LoginOutcome::Failure(
            LoginFailureReason::IncompatibleProtocolVersion,
        ));
    }

    let cid = state.verify_ws_auth_token(token).await.map_err(|err| {
        tracing::debug!(?err, "Websocket login flow failed");
        LoginOutcome::Failure(LoginFailureReason::InvalidCredentials)
    })?;

    if !state.config.vatsim.require_active_connection {
        tracing::trace!(
            ?cid,
            "Websocket token verified, no active VATSIM connection required, websocket login flow completed"
        );

        let position = state.clients.get_position(position_id.as_ref());
        let active_profile = if custom_profile {
            ActiveProfile::Custom
        } else {
            position
                .as_ref()
                .and_then(|p| {
                    p.profile_id
                        .as_ref()
                        .map(|p| ActiveProfile::Specific(p.clone()))
                })
                .unwrap_or(ActiveProfile::None)
        };

        let client_info = ClientInfo {
            id: cid.clone(),
            position_id: position.map(|p| p.id),
            display_name: cid.to_string(),
            frequency: "".to_string(),
        };
        ProfileMetrics::profile_activated(&active_profile);
        return Ok((client_info, active_profile));
    }

    tracing::trace!(
        ?cid,
        "Websocket token verified, checking for active VATSIM connection"
    );
    resolve_vatsim_position(state, cid, custom_profile, position_id).await
}

fn is_protocol_compatible(state: &AppState, protocol_version: &str) -> bool {
    Version::parse(protocol_version)
        .map(|version| state.updates.is_compatible_protocol(version))
        .unwrap_or(false)
}

async fn resolve_vatsim_position(
    state: &Arc<AppState>,
    cid: ClientId,
    custom_profile: bool,
    position_id: Option<PositionId>,
) -> Result<(ClientInfo, ActiveProfile<ProfileId>), LoginOutcome> {
    match state.get_vatsim_controller_info(&cid).await {
        Ok(info) => match info {
            None
            | Some(ControllerInfo {
                facility_type: FacilityType::Unknown,
                ..
            }) => {
                tracing::trace!(?cid, "No active VATSIM connection found, rejecting login");
                Err(LoginOutcome::Failure(
                    LoginFailureReason::NoActiveVatsimConnection,
                ))
            }
            Some(controller_info) => {
                tracing::trace!(
                    ?cid,
                    ?controller_info,
                    "VATSIM user info found, resolving matching positions"
                );
                let positions = state.clients.find_positions(&controller_info);

                let position = if positions.is_empty() {
                    tracing::trace!(?cid, ?controller_info, "No matching position found");
                    VatsimSyncMetrics::position_match("none");
                    None
                } else if positions.len() == 1 {
                    tracing::trace!(?cid, ?controller_info, position = ?positions[0], "Found matching position");
                    VatsimSyncMetrics::position_match("matched");
                    Some(&positions[0])
                } else if let Some(target_pid) = position_id.as_ref() {
                    if let Some(position) = positions.iter().find(|p| &p.id == target_pid) {
                        tracing::trace!(
                            ?cid,
                            ?controller_info,
                            ?position,
                            "Found multiple matching positions, user selection is included, assigning selection"
                        );
                        VatsimSyncMetrics::position_match("ambiguous_resolved");
                        Some(position)
                    } else {
                        tracing::trace!(
                            ?cid,
                            ?controller_info,
                            ?target_pid,
                            "Found multiple matching positions, but user selection is not included, rejecting login as invalid"
                        );
                        VatsimSyncMetrics::position_match("ambiguous_invalid");
                        return Err(LoginOutcome::Failure(
                            LoginFailureReason::InvalidVatsimPosition,
                        ));
                    }
                } else {
                    tracing::trace!(
                        ?cid,
                        ?controller_info,
                        positions = positions.len(),
                        "Found multiple matching positions, rejecting login as ambiguous"
                    );
                    VatsimSyncMetrics::position_match("ambiguous");
                    let position_ids = positions.into_iter().map(|p| p.id.clone()).collect();
                    return Err(LoginOutcome::Failure(
                        LoginFailureReason::AmbiguousVatsimPosition(position_ids),
                    ));
                };

                let client_info = ClientInfo {
                    id: cid,
                    position_id: position.map(|p| p.id.clone()),
                    display_name: controller_info.callsign.clone(),
                    frequency: controller_info.frequency.clone(),
                };

                let active_profile = if custom_profile {
                    ActiveProfile::Custom
                } else {
                    position
                        .and_then(|p| {
                            p.profile_id
                                .as_ref()
                                .map(|p| ActiveProfile::Specific(p.clone()))
                        })
                        .unwrap_or(ActiveProfile::None)
                };

                ProfileMetrics::profile_activated(&active_profile);
                Ok((client_info, active_profile))
            }
        },
        Err(err) => {
            tracing::warn!(?cid, ?err, "Failed to retrieve VATSIM user info");
            Err(LoginOutcome::Error(ErrorReason::Internal(
                "Failed to retrieve VATSIM connection info".to_string(),
            )))
        }
    }
}

async fn handle_login_outcome(
    websocket_sender: &mut SplitSink<WebSocket, ws::Message>,
    outcome: LoginOutcome,
) {
    match outcome {
        LoginOutcome::Failure(reason) => {
            ClientMetrics::login_attempt(false);
            ClientMetrics::login_failure(reason.clone());
            let message = server::LoginFailure { reason };
            if let Err(err) = send_message_raw(websocket_sender, message).await {
                tracing::warn!(?err, "Failed to send websocket login failure message");
            }
        }
        LoginOutcome::Error(reason) => {
            ClientMetrics::login_attempt(false);
            ErrorMetrics::error(&reason);
            if let Err(err) = send_message_raw(websocket_sender, shared::Error::from(reason)).await
            {
                tracing::warn!(?err, "Failed to send websocket login error message");
            }
        }
        LoginOutcome::Disconnected => {
            ClientMetrics::login_attempt(false);
        }
    }
}
