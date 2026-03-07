pub(crate) mod audio;
pub(crate) mod http;
pub(crate) mod keybinds;
mod sealed;
pub(crate) mod signaling;
pub(crate) mod webrtc;

use crate::app::state::signaling::{AppStateSignalingExt, ConnectionState};
use crate::app::state::webrtc::{Call, UnansweredCallGuard};
use crate::audio::manager::{AudioManager, AudioManagerHandle};
use crate::config::AppConfig;
use crate::error::{StartupError, StartupErrorExt};
use crate::keybinds::engine::{KeybindEngine, KeybindEngineHandle};
use crate::signaling::auth::TauriTokenProvider;
use notify_debouncer_full::notify::RecommendedWatcher;
use notify_debouncer_full::{Debouncer, RecommendedCache};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex as TokioMutex, RwLock as TokioRwLock};
use tokio_util::sync::CancellationToken;
use vacs_signaling::client::SignalingClient;
use vacs_signaling::protocol::vatsim::ClientId;
use vacs_signaling::protocol::ws::server;
use vacs_signaling::protocol::ws::shared::{CallId, CallInvite};
use vacs_signaling::transport::tokio::TokioTransport;

pub struct AppStateInner {
    pub config: AppConfig,
    shutdown_token: CancellationToken,
    signaling_client: SignalingClient<TokioTransport, TauriTokenProvider>,
    audio_manager: AudioManagerHandle,
    keybind_engine: KeybindEngineHandle,
    active_call: Option<Call>,
    unanswered_call_guard: Option<UnansweredCallGuard>,
    held_calls: HashMap<CallId, Call>, // call_id -> call
    pub(crate) outgoing_call: Option<CallInvite>,
    pub(crate) incoming_calls: HashMap<CallId, CallInvite>,
    pub test_profile_watcher: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
    pub(crate) client_id: Option<ClientId>,
    pub(crate) connection_state: ConnectionState,
    pub(crate) session_info: Option<server::SessionInfo>,
    pub(crate) stations: Vec<server::StationInfo>,
    pub(crate) clients: Vec<server::ClientInfo>,
}

pub type AppState = TokioMutex<AppStateInner>;

impl AppStateInner {
    pub fn new(app: &AppHandle) -> Result<Self, StartupError> {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_startup_err(StartupError::Config)?;

        let config = AppConfig::parse(&config_dir).map_startup_err(StartupError::Config)?;
        let shutdown_token = CancellationToken::new();

        Ok(Self {
            config: config.clone(),
            signaling_client: Self::new_signaling_client(
                app.clone(),
                &config.backend.ws_url,
                shutdown_token.child_token(),
                config.client.max_signaling_reconnect_attempts(),
            ),
            audio_manager: Arc::new(RwLock::new(
                AudioManager::new(app.clone(), &config.audio)
                    .map_startup_err(StartupError::Audio)?,
            )),
            keybind_engine: Arc::new(TokioRwLock::new(KeybindEngine::new(
                app.clone(),
                &config.client.transmit_config,
                &config.client.keybinds,
                &config.client.radio,
                shutdown_token.child_token(),
            ))),
            shutdown_token,
            active_call: None,
            unanswered_call_guard: None,
            held_calls: HashMap::new(),
            outgoing_call: None,
            incoming_calls: HashMap::new(),
            test_profile_watcher: None,
            client_id: None,
            connection_state: ConnectionState::Disconnected,
            session_info: None,
            stations: Vec::new(),
            clients: Vec::new(),
        })
    }

    pub fn shutdown(&self) {
        self.shutdown_token.cancel();
    }
}
