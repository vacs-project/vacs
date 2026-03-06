use crate::app::state::AppState;
use crate::app::state::http::HttpState;
use crate::app::state::signaling::ConnectionState;
use crate::audio::manager::AudioManagerHandle;
use crate::config::{FrontendCallConfig, FrontendClientPageSettings};
use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use crate::platform::Capabilities;
use crate::remote::protocol::{ClientMessage, RemoteCommand, RemoteEvent, ServerMessage};
use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{AppHandle, Listener, Manager};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use vacs_signaling::protocol::vatsim::ClientId;
use vacs_signaling::protocol::ws::server::{ClientInfo, SessionInfo, StationInfo};

const BROADCAST_CHANNEL_SIZE: usize = 256;
const DISPATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct RemoteServerState {
    pub app_handle: AppHandle,
    pub event_tx: broadcast::Sender<ServerMessage>,
    pub shutdown: CancellationToken,
}

pub async fn start_server(
    app_handle: AppHandle,
    listen_addr: SocketAddr,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let (event_tx, _) = broadcast::channel::<ServerMessage>(BROADCAST_CHANNEL_SIZE);

    let state = RemoteServerState {
        app_handle: app_handle.clone(),
        event_tx: event_tx.clone(),
        shutdown: shutdown.clone(),
    };

    register_event_forwarders(&app_handle, &event_tx);

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .fallback(serve_embedded_asset)
        .with_state(state);

    log::info!("Remote control server listening on http://{listen_addr}");

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown.cancelled_owned())
    .await?;

    log::info!("Remote control server stopped");
    Ok(())
}

async fn serve_embedded_asset(State(state): State<RemoteServerState>, uri: Uri) -> Response {
    let resolver = state.app_handle.asset_resolver();

    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // SPA fallback: serve index.html for unresolved paths (client-side routing).
    resolver
        .get(path.to_string())
        .or_else(|| resolver.get("index.html".to_string()))
        .map(|asset| {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, asset.mime_type())
                .body(Body::from(asset.bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        })
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn register_event_forwarders(app: &AppHandle, event_tx: &broadcast::Sender<ServerMessage>) {
    for &remote_event in RemoteEvent::ALL {
        let tx = event_tx.clone();
        app.listen(remote_event.as_str(), move |event| {
            let payload = serde_json::from_str(event.payload())
                .unwrap_or(serde_json::Value::String(event.payload().to_string()));
            let msg = ServerMessage::Event {
                name: remote_event,
                payload,
            };
            if tx.receiver_count() > 0 {
                let _ = tx.send(msg);
            }
        });
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<RemoteServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state, peer))
}

async fn handle_ws_connection(socket: WebSocket, state: RemoteServerState, peer: SocketAddr) {
    log::info!("[{peer}] Remote client connected");
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut event_rx = state.event_tx.subscribe();
    let subscribed_events = Arc::new(parking_lot::Mutex::new(HashSet::<RemoteEvent>::new()));

    let (client_tx, mut client_rx) = tokio::sync::mpsc::channel::<ServerMessage>(64);
    let subs = subscribed_events.clone();
    let shutdown = state.shutdown.clone();
    let forward_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    log::debug!("Shutdown requested, stopping remote event forwarder");
                    break;
                }
                result = event_rx.recv() => {
                    let msg = match result {
                        Ok(msg) => msg,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Remote event forwarder lagged, dropped {n} events");
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    if let ServerMessage::Event { ref name, .. } = msg
                        && !subs.lock().contains(name)
                    {
                        continue;
                    }
                    if let Ok(msg) = msg.serialize() && ws_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Some(msg) = client_rx.recv() => {
                    if let Ok(msg) = msg.serialize() && ws_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else {
                    log::warn!("[{peer}] Failed to parse remote client message: {text}");
                    continue;
                };

                match client_msg {
                    ClientMessage::Subscribe { event } => {
                        log::debug!("[{peer}] Remote client subscribed to event: {event}");
                        subscribed_events.lock().insert(event);
                    }
                    ClientMessage::Unsubscribe { event } => {
                        log::debug!("[{peer}] Remote client unsubscribed from event: {event}");
                        subscribed_events.lock().remove(&event);
                    }
                    ClientMessage::Invoke { id, cmd, args } => {
                        let response = tokio::time::timeout(
                            DISPATCH_TIMEOUT,
                            dispatch_command(&state.app_handle, cmd, args),
                        )
                        .await
                        .unwrap_or_else(|_| {
                            log::warn!("[{peer}] Remote client command {cmd:?} timed out");
                            DispatchResult::Err(serde_json::json!({
                                "title": "Timeout",
                                "message": "The command did not complete within the time limit",
                                "isNonCritical": true
                            }))
                        });
                        let _ = client_tx.send(response.with_id(id)).await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    forward_task.abort();
    log::info!("[{peer}] Remote client disconnected");
}

enum DispatchResult {
    Ok(serde_json::Value),
    Err(serde_json::Value),
}

impl DispatchResult {
    fn with_id(self, id: String) -> ServerMessage {
        match self {
            DispatchResult::Ok(data) => ServerMessage::ok(id, data),
            DispatchResult::Err(error) => ServerMessage::err(id, error),
        }
    }
}

fn dispatch<T: serde::Serialize>(result: Result<T, Error>) -> DispatchResult {
    match result {
        Ok(v) => DispatchResult::Ok(
            serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
        ),
        Err(e) => DispatchResult::Err(
            serde_json::to_value(&e).unwrap_or_else(|_| {
                serde_json::json!({"title": "Internal error", "message": "Failed to serialize error"})
            }),
        ),
    }
}

fn desktop_only() -> DispatchResult {
    DispatchResult::Err(serde_json::json!({
        "title": "Desktop only",
        "message": "This operation is only available on the desktop application",
        "isNonCritical": true
    }))
}

macro_rules! arg {
    ($args:expr, $key:literal) => {
        match serde_json::from_value(
            $args
                .get($key)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        ) {
            Ok(v) => v,
            Err(e) => {
                return DispatchResult::Err(serde_json::json!({
                    "title": "Invalid argument",
                    "message": format!("Failed to parse argument '{}': {}", $key, e),
                    "isNonCritical": true
                }))
            }
        }
    };
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStateSnapshot {
    connection_state: ConnectionState,
    session_info: Option<SessionInfo>,
    stations: Vec<StationInfo>,
    clients: Vec<ClientInfo>,
    client_id: Option<ClientId>,
    call_config: FrontendCallConfig,
    client_page_settings: FrontendClientPageSettings,
    capabilities: Capabilities,
}

async fn dispatch_command(
    app: &AppHandle,
    cmd: RemoteCommand,
    args: serde_json::Value,
) -> DispatchResult {
    use crate::app::commands::*;
    use crate::audio::commands::*;
    use crate::auth::commands::*;
    use crate::keybinds::commands::*;
    use crate::signaling::commands::*;
    use RemoteCommand::*;

    if cmd.is_desktop_only() {
        return desktop_only();
    }

    match cmd {
        AppFrontendReady => {
            // Acknowledge, but do not actually process as this would reset state on remote connect
            DispatchResult::Ok(serde_json::Value::Null)
        }
        AppCheckForUpdate => dispatch(app_check_for_update(app.clone()).await),
        AppPlatformCapabilities => dispatch(app_platform_capabilities().await),
        AppGetCallConfig => {
            let app_state = app.state::<AppState>();
            dispatch(app_get_call_config(app_state).await)
        }
        AppSetCallConfig => {
            let call_config = arg!(args, "callConfig");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(app_set_call_config(app.clone(), app_state, audio_manager, call_config).await)
        }
        AppLoadTestProfile => {
            // Only allow manual reloading of test profile if already set
            let path: Option<String> = arg!(args, "path");
            if path.is_none() {
                return desktop_only();
            }
            let app_state = app.state::<AppState>();
            dispatch(app_load_test_profile(app.clone(), app_state, path).await)
        }
        AppUnloadTestProfile => {
            let app_state = app.state::<AppState>();
            dispatch(app_unload_test_profile(app_state).await)
        }
        AppGetClientPageSettings => {
            let app_state = app.state::<AppState>();
            dispatch(app_get_client_page_settings(app_state).await)
        }
        AppSetSelectedClientPageConfig => {
            let config_name: Option<String> = arg!(args, "configName");
            let app_state = app.state::<AppState>();
            dispatch(app_set_selected_client_page_config(app.clone(), app_state, config_name).await)
        }

        AudioGetHosts => {
            let app_state = app.state::<AppState>();
            dispatch(audio_get_hosts(app_state).await)
        }
        AudioSetHost => {
            let host_name: String = arg!(args, "hostName");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_set_host(app.clone(), app_state, audio_manager, host_name).await)
        }
        AudioGetDevices => {
            let device_type = arg!(args, "deviceType");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_get_devices(app_state, audio_manager, device_type).await)
        }
        AudioSetDevice => {
            let device_type = arg!(args, "deviceType");
            let device_name: String = arg!(args, "deviceName");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(
                audio_set_device(
                    app.clone(),
                    app_state,
                    audio_manager,
                    device_type,
                    device_name,
                )
                .await,
            )
        }
        AudioGetVolumes => {
            let app_state = app.state::<AppState>();
            dispatch(audio_get_volumes(app_state).await)
        }
        AudioSetVolume => {
            let volume_type = arg!(args, "volumeType");
            let volume: f32 = arg!(args, "volume");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(
                audio_set_volume(app.clone(), app_state, audio_manager, volume_type, volume).await,
            )
        }
        AudioPlayUiClick => {
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_play_ui_click(audio_manager).await)
        }
        AudioStartInputLevelMeter => {
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_start_input_level_meter(app_state, audio_manager, app.clone()).await)
        }
        AudioStopInputLevelMeter => {
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_stop_input_level_meter(audio_manager).await)
        }
        AudioSetRadioPrio => {
            let prio: bool = arg!(args, "prio");
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(audio_set_radio_prio(keybind_engine, prio).await)
        }

        AuthOpenOauthUrl => {
            let http_state = app.state::<HttpState>();
            dispatch(auth_open_oauth_url(http_state).await)
        }
        AuthCheckSession => {
            let http_state = app.state::<HttpState>();
            dispatch(auth_check_session(app.clone(), http_state).await)
        }
        AuthLogout => {
            let app_state = app.state::<AppState>();
            let http_state = app.state::<HttpState>();
            dispatch(auth_logout(app.clone(), app_state, http_state).await)
        }

        KeybindsGetTransmitConfig => {
            let app_state = app.state::<AppState>();
            dispatch(keybinds_get_transmit_config(app_state).await)
        }
        KeybindsSetTransmitConfig => {
            let transmit_config = arg!(args, "transmitConfig");
            let app_state = app.state::<AppState>();
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(
                keybinds_set_transmit_config(
                    app.clone(),
                    app_state,
                    keybind_engine,
                    transmit_config,
                )
                .await,
            )
        }
        KeybindsGetKeybindsConfig => {
            let app_state = app.state::<AppState>();
            dispatch(keybinds_get_keybinds_config(app_state).await)
        }
        KeybindsSetBinding => {
            let code: Option<String> = arg!(args, "code");
            let keybind = arg!(args, "keybind");
            let app_state = app.state::<AppState>();
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(
                keybinds_set_binding(app.clone(), app_state, keybind_engine, code, keybind).await,
            )
        }
        KeybindsGetRadioConfig => {
            let app_state = app.state::<AppState>();
            dispatch(keybinds_get_radio_config(app_state).await)
        }
        KeybindsSetRadioConfig => {
            let radio_config = arg!(args, "radioConfig");
            let app_state = app.state::<AppState>();
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(
                keybinds_set_radio_config(app.clone(), app_state, keybind_engine, radio_config)
                    .await,
            )
        }
        KeybindsGetRadioState => {
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(keybinds_get_radio_state(keybind_engine).await)
        }
        KeybindsGetExternalBinding => {
            let keybind = arg!(args, "keybind");
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(keybinds_get_external_binding(keybind_engine, keybind).await)
        }
        KeybindsReconnectRadio => {
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(keybinds_reconnect_radio(keybind_engine).await)
        }

        SignalingConnect => {
            let position_id = arg!(args, "positionId");
            let app_state = app.state::<AppState>();
            let http_state = app.state::<HttpState>();
            dispatch(signaling_connect(app.clone(), app_state, http_state, position_id).await)
        }
        SignalingDisconnect => dispatch(signaling_disconnect(app.clone()).await),
        SignalingTerminate => {
            let http_state = app.state::<HttpState>();
            dispatch(signaling_terminate(app.clone(), http_state).await)
        }
        SignalingStartCall => {
            let target = arg!(args, "target");
            let source = arg!(args, "source");
            let prio: bool = arg!(args, "prio");
            let app_state = app.state::<AppState>();
            let http_state = app.state::<HttpState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(
                signaling_start_call(
                    app.clone(),
                    app_state,
                    http_state,
                    audio_manager,
                    target,
                    source,
                    prio,
                )
                .await,
            )
        }
        SignalingAcceptCall => {
            let call_id = arg!(args, "callId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_accept_call(app.clone(), app_state, call_id).await)
        }
        SignalingEndCall => {
            let call_id = arg!(args, "callId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_end_call(app.clone(), app_state, call_id).await)
        }
        SignalingGetIgnoredClients => {
            let app_state = app.state::<AppState>();
            dispatch(signaling_get_ignored_clients(app_state).await)
        }
        SignalingAddIgnoredClient => {
            let client_id = arg!(args, "clientId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_add_ignored_client(app.clone(), app_state, client_id).await)
        }
        SignalingRemoveIgnoredClient => {
            let client_id = arg!(args, "clientId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_remove_ignored_client(app.clone(), app_state, client_id).await)
        }

        RemoteGetSessionState => {
            let app_state = app.state::<AppState>();
            let state = app_state.lock().await;

            let snapshot = SessionStateSnapshot {
                connection_state: state.connection_state,
                session_info: state.session_info.clone(),
                stations: state.stations.clone(),
                clients: state.clients.clone(),
                client_id: state.client_id.clone(),
                call_config: state.config.client.call.clone().into(),
                client_page_settings: FrontendClientPageSettings::from(&state.config),
                capabilities: *Capabilities::get(),
            };

            DispatchResult::Ok(serde_json::to_value(snapshot).unwrap_or_default())
        }

        AppOpenFolder
        | AppQuit
        | AppUpdate
        | AppSetAlwaysOnTop
        | AppSetFullscreen
        | AppResetWindowSize
        | AppLoadExtraClientPageConfig
        | KeybindsOpenSystemShortcutsSettings => {
            unreachable!("desktop-only commands should have been rejected up front")
        }
    }
}
