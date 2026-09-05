use crate::app::state::AppState;
use crate::app::state::http::HttpState;
use crate::app::state::signaling::ConnectionState;
use crate::app::{ClockMode, FrontendCallConfig, FrontendClientPageSettings};
use crate::audio::manager::AudioManagerHandle;
use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use crate::platform::Capabilities;
use crate::playback::commands::{
    playback_clear, playback_continue, playback_delete, playback_export, playback_get_enabled,
    playback_list, playback_pause, playback_seek, playback_set_enabled, playback_start,
    playback_stop,
};
use crate::playback::recorder::PlaybackRecorderHandle;
use crate::radio::RadioHandle;
use crate::remote::RemoteStatus;
use crate::remote::commands::FrontendRemoteConfigWithStatus;
use crate::remote::protocol::{
    ClientMessage, ProblemDetails, RemoteCommand, RemoteEvent, ServerMessage,
};
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
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use vacs_signaling::protocol::vatsim::{ClientId, StationId};
use vacs_signaling::protocol::ws::server::{ClientInfo, SessionInfo, StationInfo};
use vacs_signaling::protocol::ws::shared::CallInvite;

const BROADCAST_CHANNEL_SIZE: usize = 256;
const DISPATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct RemoteServerState {
    pub app_handle: AppHandle,
    pub event_tx: broadcast::Sender<ServerMessage>,
    pub shutdown: CancellationToken,
    pub client_count: Arc<AtomicUsize>,
}

impl RemoteServerState {
    fn emit_status(&self) {
        let status = RemoteStatus {
            // Always true if we're emitting from a remote server state.
            // The handle is responsible for emitting the correct status on shutdown.
            listening: true,
            connected_clients: self.client_count.load(Ordering::Relaxed),
        };
        self.app_handle.emit("remote:status", &status).ok();
    }
}

pub struct RemoteServer {
    shutdown: Option<CancellationToken>,
    app_handle: AppHandle,
    client_count: Arc<AtomicUsize>,
}

impl RemoteServer {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            shutdown: None,
            app_handle,
            client_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn start(&mut self, listen_addr: SocketAddr, serve_frontend: bool) {
        if self.is_listening() {
            log::warn!("Remote server already running, ignoring start request");
            return;
        }

        let shutdown_token = CancellationToken::new();
        self.shutdown = Some(shutdown_token.clone());
        self.client_count = Arc::new(AtomicUsize::new(0));

        let app_handle = self.app_handle.clone();
        let client_count = self.client_count.clone();
        tokio::spawn(async move {
            if let Err(err) = start_server(
                app_handle.clone(),
                listen_addr,
                serve_frontend,
                shutdown_token.clone(),
                client_count,
            )
            .await
            {
                log::error!("Remote control server error: {err}");
                shutdown_token.cancel();
                let status = RemoteStatus {
                    listening: false,
                    connected_clients: 0,
                };
                app_handle.emit("remote:status", &status).ok();
            }
        });

        self.emit_status();
    }

    pub fn stop(&mut self) {
        if let Some(token) = self.shutdown.take() {
            log::info!("Stopping remote control server");
            token.cancel();
            self.client_count.store(0, Ordering::Relaxed);
            self.emit_status();
        }
    }

    pub fn restart(&mut self, listen_addr: SocketAddr, serve_frontend: bool) {
        self.stop();
        self.start(listen_addr, serve_frontend);
    }

    pub fn is_listening(&self) -> bool {
        self.shutdown
            .as_ref()
            .is_some_and(|token| !token.is_cancelled())
    }

    pub fn connected_clients(&self) -> usize {
        self.client_count.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> RemoteStatus {
        RemoteStatus {
            listening: self.is_listening(),
            connected_clients: self.connected_clients(),
        }
    }

    pub fn emit_status(&self) {
        self.app_handle.emit("remote:status", &self.status()).ok();
    }
}

pub type RemoteServerHandle = tokio::sync::Mutex<RemoteServer>;

pub async fn start_server(
    app_handle: AppHandle,
    listen_addr: SocketAddr,
    serve_frontend: bool,
    shutdown: CancellationToken,
    client_count: Arc<AtomicUsize>,
) -> anyhow::Result<()> {
    let (event_tx, _) = broadcast::channel::<ServerMessage>(BROADCAST_CHANNEL_SIZE);

    let state = RemoteServerState {
        app_handle: app_handle.clone(),
        event_tx: event_tx.clone(),
        shutdown: shutdown.clone(),
        client_count,
    };

    let forwarder_ids = register_event_forwarders(&app_handle, &event_tx);

    let mut router = Router::new().route("/ws", get(ws_handler));

    if serve_frontend {
        router = router.fallback(serve_embedded_asset);
    }

    let app = router.with_state(state);

    log::info!("Remote control server listening on http://{listen_addr}");

    let result = async {
        let listener = tokio::net::TcpListener::bind(listen_addr).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await?;
        anyhow::Ok(())
    }
    .await;

    // The forwarders must not outlive the server: they are registered globally
    // on the app, so leaving them behind would pile up another full set of
    // listeners on every server restart.
    for id in forwarder_ids {
        app_handle.unlisten(id);
    }

    if result.is_ok() {
        log::info!("Remote control server stopped");
    }
    result
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

fn register_event_forwarders(
    app: &AppHandle,
    event_tx: &broadcast::Sender<ServerMessage>,
) -> Vec<tauri::EventId> {
    RemoteEvent::ALL
        .iter()
        .map(|&remote_event| {
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
            })
        })
        .collect()
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
    state.client_count.fetch_add(1, Ordering::Relaxed);
    state.emit_status();
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
                let client_msg = match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(msg) => msg,
                    Err(err) => {
                        log::warn!(
                            "[{peer}] Failed to parse remote client message ({err}): {text}"
                        );
                        // If this was an invoke, settle the client's pending request
                        // with an error instead of leaving its promise hanging forever.
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                            && value.get("type").and_then(serde_json::Value::as_str)
                                == Some("invoke")
                            && let Some(id) = value.get("id").and_then(serde_json::Value::as_str)
                        {
                            let response = ServerMessage::err(
                                id.to_string(),
                                ProblemDetails::invalid_message(),
                            );
                            let _ = client_tx.send(response).await;
                        }
                        continue;
                    }
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
                    ClientMessage::Ping => {
                        let _ = client_tx.send(ServerMessage::Pong).await;
                    }
                    ClientMessage::Invoke { id, cmd, args } => {
                        let response = tokio::time::timeout(
                            DISPATCH_TIMEOUT,
                            dispatch_command(&state.app_handle, cmd, args),
                        )
                        .await
                        .unwrap_or_else(|_| {
                            log::warn!("[{peer}] Remote client command {cmd:?} timed out");
                            DispatchResult::Err(ProblemDetails::timeout())
                        });
                        let _ = client_tx.send(response.with_id(id)).await;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(data) => {
                let _ = client_tx.send(ServerMessage::WsPong(data.to_vec())).await;
            }
            _ => {}
        }
    }

    forward_task.abort();
    state.client_count.fetch_sub(1, Ordering::Relaxed);
    if !state.shutdown.is_cancelled() {
        state.emit_status();
    }
    log::info!("[{peer}] Remote client disconnected");
}

enum DispatchResult {
    Ok(serde_json::Value),
    Err(ProblemDetails),
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
        Ok(v) => DispatchResult::Ok(serde_json::to_value(v).unwrap_or(serde_json::Value::Null)),
        Err(e) => DispatchResult::Err(ProblemDetails::from(&e)),
    }
}

fn desktop_only() -> DispatchResult {
    DispatchResult::Err(ProblemDetails::desktop_only())
}

macro_rules! args {
    ($args:expr, $key:literal) => {
        match serde_json::from_value(
            $args
                .get($key)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        ) {
            Ok(v) => v,
            Err(e) => {
                return DispatchResult::Err(ProblemDetails::invalid_argument($key, e))
            }
        }
    };
    ($args:expr, $($key:literal),+ $(,)?) => {
        ($(args!($args, $key)),+)
    };
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStateSnapshot {
    connection_state: ConnectionState,
    session_info: Option<SessionInfo>,
    default_call_sources: Vec<StationId>,
    stations: Vec<StationInfo>,
    clients: Vec<ClientInfo>,
    client_id: Option<ClientId>,
    call_config: FrontendCallConfig,
    client_page_settings: FrontendClientPageSettings,
    capabilities: Capabilities,
    incoming_calls: Vec<CallInvite>,
    outgoing_call: Option<CallInvite>,
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
    use crate::radio::commands::*;
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
            let call_config = args!(args, "callConfig");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(app_set_call_config(app.clone(), app_state, audio_manager, call_config).await)
        }
        AppLoadTestProfile => {
            // Only allow manual reloading of test profile if already set
            let path: Option<String> = args!(args, "path");
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
            let config_name: Option<String> = args!(args, "configName");
            let app_state = app.state::<AppState>();
            dispatch(app_set_selected_client_page_config(app.clone(), app_state, config_name).await)
        }
        AppGetVersion => dispatch(Ok(app_get_version())),
        AppGetClockMode => {
            let app_state = app.state::<AppState>();
            dispatch(app_get_clock_mode(app_state).await)
        }
        AppSetClockMode => {
            let clock_mode: ClockMode = args!(args, "clockMode");
            let app_state = app.state::<AppState>();
            dispatch(app_set_clock_mode(app.clone(), app_state, clock_mode).await)
        }
        AppGetCplMode => {
            let app_state = app.state::<AppState>();
            dispatch(app_get_cpl_mode(app_state).await)
        }
        AppSetCplMode => {
            let cpl_mode = args!(args, "cplMode");
            let app_state = app.state::<AppState>();
            dispatch(app_set_cpl_mode(app.clone(), app_state, cpl_mode).await)
        }

        AudioGetHosts => {
            let app_state = app.state::<AppState>();
            dispatch(audio_get_hosts(app_state).await)
        }
        AudioSetHost => {
            let host_name: String = args!(args, "hostName");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_set_host(app.clone(), app_state, audio_manager, host_name).await)
        }
        AudioGetDevices => {
            let device_type = args!(args, "deviceType");
            let app_state = app.state::<AppState>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(audio_get_devices(app_state, audio_manager, device_type).await)
        }
        AudioSetDevice => {
            let (device_type, device_name) = args!(args, "deviceType", "deviceName");
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
            let (volume_type, volume) = args!(args, "volumeType", "volume");
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
            let prio: bool = args!(args, "prio");
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(audio_set_radio_prio(app.clone(), keybind_engine, prio).await)
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
            let transmit_config = args!(args, "transmitConfig");
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
            let (input, keybind) = args!(args, "input", "keybind");
            let app_state = app.state::<AppState>();
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(
                keybinds_set_binding(app.clone(), app_state, keybind_engine, input, keybind).await,
            )
        }
        KeybindsCaptureJoystickButton => {
            let capture_id = args!(args, "captureId");
            let app_state = app.state::<AppState>();
            let capture_state = app.state::<crate::keybinds::commands::JoystickCaptureState>();
            let joystick = app.state::<crate::keybinds::joystick::JoystickServiceHandle>();
            dispatch(
                keybinds_capture_joystick_button(
                    app_state,
                    capture_state,
                    joystick,
                    capture_id,
                    None,
                )
                .await,
            )
        }
        KeybindsCancelJoystickCapture => {
            let capture_id = args!(args, "captureId");
            let capture_state = app.state::<crate::keybinds::commands::JoystickCaptureState>();
            dispatch(keybinds_cancel_joystick_capture(capture_state, capture_id).await)
        }
        KeybindsListJoystickDevices => {
            let app_state = app.state::<AppState>();
            let joystick = app.state::<crate::keybinds::joystick::JoystickServiceHandle>();
            dispatch(keybinds_list_joystick_devices(app_state, joystick).await)
        }
        KeybindsSetIgnoredJoysticks => {
            let devices = args!(args, "devices");
            let app_state = app.state::<AppState>();
            dispatch(keybinds_set_ignored_joysticks(app.clone(), app_state, devices).await)
        }
        KeybindsGetExternalBinding => {
            let keybind = args!(args, "keybind");
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(keybinds_get_external_binding(keybind_engine, keybind).await)
        }
        KeybindsIsPortalShortcutBound => {
            let keybind = args!(args, "keybind");
            let keybind_engine = app.state::<KeybindEngineHandle>();
            dispatch(keybinds_is_portal_shortcut_bound(keybind_engine, keybind).await)
        }

        PlaybackGetEnabled => {
            let app_state = app.state::<AppState>();
            dispatch(playback_get_enabled(app_state).await)
        }
        PlaybackSetEnabled => {
            let app_state = app.state::<AppState>();
            dispatch(playback_set_enabled(app.clone(), app_state, args!(args, "enabled")).await)
        }
        PlaybackList => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            dispatch(playback_list(recorder).await)
        }
        PlaybackDelete => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            dispatch(playback_delete(recorder, args!(args, "id")).await)
        }
        PlaybackClear => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            dispatch(playback_clear(recorder).await)
        }
        PlaybackStart => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(
                playback_start(
                    app.clone(),
                    recorder,
                    audio_manager,
                    args!(args, "id"),
                    args!(args, "deviceType"),
                    args!(args, "initialProgress"),
                    args!(args, "startPaused"),
                )
                .await,
            )
        }
        PlaybackPause => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(playback_pause(recorder, audio_manager).await)
        }
        PlaybackContinue => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(playback_continue(recorder, audio_manager).await)
        }
        PlaybackStop => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(playback_stop(recorder, audio_manager).await)
        }
        PlaybackSeek => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            let audio_manager = app.state::<AudioManagerHandle>();
            dispatch(playback_seek(recorder, audio_manager, args!(args, "millis")).await)
        }
        PlaybackExport => {
            let recorder = app.state::<PlaybackRecorderHandle>();
            dispatch(playback_export(recorder, args!(args, "id")).await)
        }

        RadioAddStation => {
            let callsign = args!(args, "callsign");
            let radio = app.state::<RadioHandle>();
            dispatch(radio_add_station(radio, callsign).await)
        }
        RadioFastCouple => {
            let radio = app.state::<RadioHandle>();
            dispatch(radio_fast_couple(radio).await)
        }

        RadioGetConfig => {
            let app_state = app.state::<AppState>();
            dispatch(radio_get_config(app_state).await)
        }
        RadioReconnect => {
            let radio = app.state::<RadioHandle>();
            dispatch(radio_reconnect(radio).await)
        }
        RadioSetConfig => {
            let radio_config = args!(args, "radioConfig");
            let app_state = app.state::<AppState>();
            let radio = app.state::<RadioHandle>();
            let keybind_engine = app.state::<KeybindEngineHandle>();
            let recorder = app.state::<PlaybackRecorderHandle>();
            dispatch(
                radio_set_config(
                    app.clone(),
                    app_state,
                    keybind_engine,
                    radio,
                    recorder,
                    radio_config,
                )
                .await,
            )
        }
        RadioSetStationState => {
            let (frequency, update) = args!(args, "frequency", "update");
            let radio = app.state::<RadioHandle>();
            dispatch(radio_set_station_state(radio, frequency, update).await)
        }
        RadioGetStations => {
            let radio = app.state::<RadioHandle>();
            dispatch(radio_get_stations(radio).await)
        }
        RadioGetState => {
            let radio = app.state::<RadioHandle>();
            dispatch(radio_get_state(radio).await)
        }

        SignalingConnect => {
            let position_id = args!(args, "positionId");
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
            let (target, source, prio) = args!(args, "target", "source", "prio");
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
            let call_id = args!(args, "callId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_accept_call(app.clone(), app_state, call_id).await)
        }
        SignalingEndCall => {
            let call_id = args!(args, "callId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_end_call(app.clone(), app_state, call_id).await)
        }
        SignalingGetIgnoredClients => {
            let app_state = app.state::<AppState>();
            dispatch(signaling_get_ignored_clients(app_state).await)
        }
        SignalingAddIgnoredClient => {
            let client_id = args!(args, "clientId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_add_ignored_client(app.clone(), app_state, client_id).await)
        }
        SignalingRemoveIgnoredClient => {
            let client_id = args!(args, "clientId");
            let app_state = app.state::<AppState>();
            dispatch(signaling_remove_ignored_client(app.clone(), app_state, client_id).await)
        }

        RemoteBroadcastStoreSync => {
            // Mirrors the Tauri command in remote/commands.rs - both emit the same
            // event, but this path is used by remote (WS) clients while the command
            // is used by the desktop (IPC) frontend.
            let (store, state, source_id): (String, serde_json::Value, String) =
                args!(args, "store", "state", "sourceId");
            app.emit(
                "store:sync",
                serde_json::json!({"store": store, "state": state, "sourceId": source_id}),
            )
            .ok();
            DispatchResult::Ok(serde_json::Value::Null)
        }

        RemoteRequestStoreSync => {
            app.emit("store:sync:request", ()).ok();
            DispatchResult::Ok(serde_json::Value::Null)
        }

        RemoteIsEnabled => {
            let app_state = app.state::<AppState>();
            dispatch(crate::remote::commands::remote_is_enabled(app_state).await)
        }

        RemoteGetConfig => {
            let app_state = app.state::<AppState>();
            let remote_server = app.state::<RemoteServerHandle>();
            let result = FrontendRemoteConfigWithStatus {
                config: app_state.lock().await.config.client.remote.clone().into(),
                status: remote_server.lock().await.status(),
            };
            DispatchResult::Ok(serde_json::to_value(result).unwrap_or_default())
        }

        RemoteGetSessionState => {
            let app_state = app.state::<AppState>();
            let state = app_state.lock().await;

            let snapshot = SessionStateSnapshot {
                connection_state: state.connection_state,
                session_info: state.session_info.clone(),
                default_call_sources: state.default_call_sources.clone(),
                stations: state.stations.clone(),
                clients: state.clients.clone(),
                client_id: state.client_id.clone(),
                call_config: state.config.client.call.clone().into(),
                client_page_settings: FrontendClientPageSettings::from(&state.config),
                capabilities: *Capabilities::get(),
                incoming_calls: state.incoming_calls.values().cloned().collect(),
                outgoing_call: state.outgoing_call.clone(),
            };

            DispatchResult::Ok(serde_json::to_value(snapshot).unwrap_or_default())
        }

        AppOpenFolder
        | AppOpenUrl
        | AppQuit
        | AppUpdate
        | AppSetAlwaysOnTop
        | AppSetFullscreen
        | AppResetWindowSize
        | AppLoadExtraClientPageConfig
        | AuthOpenOauthUrl
        | KeybindsOpenSystemShortcutsSettings => {
            unreachable!("desktop-only commands should have been rejected up front")
        }
    }
}
