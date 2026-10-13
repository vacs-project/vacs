use crate::logging;
use crate::metrics::labels::AsMetricLabel;
use crate::metrics::{ClientMetrics, ErrorMetrics, ProfileMetrics, VatsimSyncMetrics};
use crate::state::AppState;
use crate::ws::message::{MessageResult, receive_message, send_message_raw};
use axum::extract::ws;
use axum::extract::ws::WebSocket;
use futures_util::stream::{SplitSink, SplitStream};
use semver::Version;
use std::sync::Arc;
use std::time::Duration;
use vacs_protocol::profile::{ActiveProfile, ProfileId};
use vacs_protocol::vatsim::{ClientId, PositionId};
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{ClientInfo, LoginFailureReason};
use vacs_protocol::ws::shared::ErrorReason;
use vacs_protocol::ws::{server, shared};
use vacs_vatsim::{ControllerInfo, FacilityType};

/// A completed websocket login.
pub struct LoginSuccess {
    pub client_info: ClientInfo,
    pub active_profile: ActiveProfile<ProfileId>,
    /// The protocol version the client announced, for the login line.
    pub protocol_version: String,
}

pub async fn handle_websocket_login(
    state: Arc<AppState>,
    websocket_receiver: &mut SplitStream<WebSocket>,
    websocket_sender: &mut SplitSink<WebSocket, ws::Message>,
) -> Option<LoginSuccess> {
    let mut protocol_version = None;
    let result = tokio::time::timeout(
        Duration::from_millis(state.config.auth.login_flow_timeout_millis),
        async {
            loop {
                match receive_message(websocket_receiver).await {
                    MessageResult::ApplicationMessage(ClientMessage::Login(login)) => {
                        protocol_version = Some(login.protocol_version.clone());
                        return process_login_request(
                            &state,
                            &login.token,
                            &login.protocol_version,
                            login.custom_profile,
                            login.position_id,
                        )
                        .await;
                    }
                    MessageResult::ApplicationMessage(message) => {
                        tracing::debug!(
                            kind = message.as_metric_label(),
                            "Received unexpected message during login"
                        );
                        return Err(LoginOutcome::Failure(LoginFailureReason::Unauthorized));
                    }
                    MessageResult::ControlMessage => continue,
                    MessageResult::Disconnected | MessageResult::Error(_) => {
                        return Err(LoginOutcome::Disconnected);
                    }
                };
            }
        },
    )
    .await;

    let outcome = match result {
        Ok(Ok((client_info, active_profile))) => {
            return Some(LoginSuccess {
                client_info,
                active_profile,
                protocol_version: protocol_version.unwrap_or_default(),
            });
        }
        Ok(Err(outcome)) => outcome,
        Err(_) => LoginOutcome::Failure(LoginFailureReason::Timeout),
    };
    handle_login_outcome(websocket_sender, outcome, protocol_version.as_deref()).await;
    None
}

enum LoginOutcome {
    Failure(LoginFailureReason),
    Error(ErrorReason),
    Disconnected,
}

async fn process_login_request(
    state: &Arc<AppState>,
    token: &str,
    protocol_version: &str,
    custom_profile: bool,
    position_id: Option<PositionId>,
) -> Result<(ClientInfo, ActiveProfile<ProfileId>), LoginOutcome> {
    if !is_protocol_compatible(state, protocol_version) {
        return Err(LoginOutcome::Failure(
            LoginFailureReason::IncompatibleProtocolVersion,
        ));
    }

    let cid = state.verify_ws_auth_token(token).await.map_err(|err| {
        tracing::debug!(
            err = format!("{err:#}"),
            "Websocket auth token not accepted"
        );
        LoginOutcome::Failure(LoginFailureReason::InvalidCredentials)
    })?;
    tracing::Span::current().record("client_id", tracing::field::display(&cid));

    if !state.config.vatsim.require_active_connection {
        tracing::debug!("No active VATSIM connection required, skipping VATSIM lookup");

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
            }) => Err(LoginOutcome::Failure(
                LoginFailureReason::NoActiveVatsimConnection,
            )),
            Some(controller_info) => {
                let positions = state.clients.find_positions(&controller_info);

                let position = if positions.is_empty() {
                    tracing::debug!(
                        callsign = %controller_info.callsign,
                        frequency = %controller_info.frequency,
                        facility_type = controller_info.facility_type.as_metric_label(),
                        "No position matches the VATSIM connection"
                    );
                    VatsimSyncMetrics::position_match("none");
                    None
                } else if positions.len() == 1 {
                    tracing::debug!(
                        callsign = %controller_info.callsign,
                        frequency = %controller_info.frequency,
                        position_id = %positions[0].id,
                        "Position matches the VATSIM connection"
                    );
                    VatsimSyncMetrics::position_match("matched");
                    Some(&positions[0])
                } else if let Some(target_pid) = position_id.as_ref() {
                    if let Some(position) = positions.iter().find(|p| &p.id == target_pid) {
                        tracing::debug!(
                            callsign = %controller_info.callsign,
                            frequency = %controller_info.frequency,
                            positions = positions.len(),
                            position_id = %position.id,
                            "Multiple positions match, using the client's selection"
                        );
                        VatsimSyncMetrics::position_match("ambiguous_resolved");
                        Some(position)
                    } else {
                        tracing::debug!(
                            callsign = %controller_info.callsign,
                            frequency = %controller_info.frequency,
                            positions = positions.len(),
                            position_id = logging::bounded(target_pid.as_str()),
                            "Multiple positions match, but not the client's selection"
                        );
                        VatsimSyncMetrics::position_match("ambiguous_invalid");
                        return Err(LoginOutcome::Failure(
                            LoginFailureReason::InvalidVatsimPosition,
                        ));
                    }
                } else {
                    tracing::debug!(
                        callsign = %controller_info.callsign,
                        frequency = %controller_info.frequency,
                        positions = positions.len(),
                        "Multiple positions match and the client made no selection"
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
            tracing::warn!(
                err = format!("{err:#}"),
                "Failed to retrieve VATSIM user info"
            );
            Err(LoginOutcome::Error(ErrorReason::Internal(
                "Failed to retrieve VATSIM connection info".to_string(),
            )))
        }
    }
}

async fn handle_login_outcome(
    websocket_sender: &mut SplitSink<WebSocket, ws::Message>,
    outcome: LoginOutcome,
    protocol_version: Option<&str>,
) {
    match outcome {
        LoginOutcome::Failure(reason) => {
            if matches!(reason, LoginFailureReason::IncompatibleProtocolVersion) {
                let protocol_version = protocol_version
                    .filter(|version| Version::parse(version).is_ok())
                    .map_or("invalid", logging::bounded);
                tracing::info!(
                    reason = reason.as_metric_label(),
                    protocol_version,
                    "Login rejected"
                );
            } else {
                tracing::info!(reason = reason.as_metric_label(), "Login rejected");
            }
            ClientMetrics::login_attempt(false);
            ClientMetrics::login_failure(reason.clone());
            let message = server::LoginFailure { reason };
            if let Err(err) = send_message_raw(websocket_sender, message).await {
                tracing::debug!(
                    err = format!("{err:#}"),
                    "Failed to send login failure message"
                );
            }
        }
        LoginOutcome::Error(reason) => {
            tracing::info!(reason = reason.as_metric_label(), "Login rejected");
            ClientMetrics::login_attempt(false);
            ErrorMetrics::error(&reason);
            if let Err(err) = send_message_raw(websocket_sender, shared::Error::from(reason)).await
            {
                tracing::debug!(
                    err = format!("{err:#}"),
                    "Failed to send login error message"
                );
            }
        }
        LoginOutcome::Disconnected => {
            tracing::debug!("Client disconnected during login");
            ClientMetrics::login_attempt(false);
        }
    }
}
