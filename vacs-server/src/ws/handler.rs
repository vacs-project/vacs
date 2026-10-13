use crate::logging;
use crate::metrics::ClientMetrics;
use crate::metrics::guards::ClientConnectionGuard;
use crate::metrics::labels::AsMetricLabel;
use crate::state::AppState;
use crate::ws::auth::{LoginSuccess, handle_websocket_login};
use crate::ws::message::send_message_raw;
use axum::extract::ws::{CloseCode, CloseFrame, Message, Utf8Bytes, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum_client_ip::ClientIp;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Instant;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode as TungsteniteCloseCode;
use tracing::Instrument;
use uuid::Uuid;
use vacs_protocol::profile::ActiveProfile;
use vacs_protocol::ws::server;
use vacs_protocol::ws::server::LoginFailureReason;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    ClientIp(ip): ClientIp,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let span = tracing::info_span!(
            parent: None,
            "connection",
            conn_id = %Uuid::now_v7(),
            client_id = tracing::field::Empty,
        );
        async move {
            let started = Instant::now();
            tracing::info!(client_ip = %logging::client_network(ip), "Client connected");
            let end = handle_socket(socket, state).await;
            tracing::info!(
                reason = end,
                duration_ms = started.elapsed().as_millis() as u64,
                "Client disconnected"
            );
        }
        .instrument(span)
    })
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) -> &'static str {
    let client_connection_guard = ClientConnectionGuard::new();

    let (mut websocket_tx, mut websocket_rx) = socket.split();

    let Some(LoginSuccess {
        client_info,
        active_profile,
        protocol_version,
    }) = handle_websocket_login(state.clone(), &mut websocket_rx, &mut websocket_tx).await
    else {
        return "login_failed";
    };

    let position_id = client_info.position_id.clone();
    let profile = match &active_profile {
        ActiveProfile::Specific(_) => "specific",
        ActiveProfile::Custom => "custom",
        ActiveProfile::None => "none",
    };

    let res = state
        .register_client(client_info, active_profile, client_connection_guard)
        .await;
    let (mut client, mut rx) = match res {
        Ok(client) => client,
        Err(_) => {
            tracing::info!(
                reason = LoginFailureReason::DuplicateId.as_metric_label(),
                "Login rejected"
            );
            ClientMetrics::login_attempt(false);
            ClientMetrics::login_failure(LoginFailureReason::DuplicateId);

            if let Err(err) = send_message_raw(
                &mut websocket_tx,
                server::LoginFailure {
                    reason: LoginFailureReason::DuplicateId,
                },
            )
            .await
            {
                tracing::debug!(
                    err = format!("{err:#}"),
                    "Failed to send login failure message"
                );
            }

            if let Err(err) = websocket_tx
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::from(TungsteniteCloseCode::Protocol),
                    reason: Utf8Bytes::from("Login failure"),
                })))
                .await
            {
                tracing::debug!(%err, "Failed to send close frame");
            }
            return "login_failed";
        }
    };

    ClientMetrics::login_attempt(true);
    tracing::info!(
        position_id = position_id.as_ref().map(tracing::field::display),
        protocol_version,
        profile,
        "Client logged in"
    );

    let (mut broadcast_rx, mut shutdown_rx) = state.get_client_receivers();

    let end = client
        .handle_interaction(
            &state,
            websocket_rx,
            websocket_tx,
            &mut broadcast_rx,
            &mut rx,
            &mut shutdown_rx,
        )
        .await;

    state.unregister_client(client.id(), None).await;

    end.as_metric_label()
}
