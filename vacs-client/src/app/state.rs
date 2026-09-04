pub(crate) mod audio;
pub(crate) mod http;
pub(crate) mod keybinds;
pub(crate) mod playback;
pub(crate) mod radio;
mod sealed;
pub(crate) mod signaling;
pub(crate) mod webrtc;

use crate::app::state::signaling::{AppStateSignalingExt, ConnectionState};
use crate::app::state::webrtc::{Call, UnansweredCallGuard};
use crate::audio::manager::{AudioManager, AudioManagerHandle};
use crate::config::AppConfig;
use crate::error::{StartupError, StartupErrorExt};
use crate::keybinds::engine::{KeybindEngine, KeybindEngineHandle};
use crate::playback::recorder::PlaybackRecorderHandle;
use crate::radio::RadioHandle;
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
use vacs_signaling::protocol::vatsim::{ClientId, StationId};
use vacs_signaling::protocol::ws::server;
use vacs_signaling::protocol::ws::shared::{CallId, CallInvite};
use vacs_signaling::transport::tokio::TokioTransport;

pub struct AppStateInner {
    pub config: AppConfig,
    shutdown_token: CancellationToken,
    signaling_client: SignalingClient<TokioTransport, TauriTokenProvider>,
    audio_manager: AudioManagerHandle,
    keybind_engine: KeybindEngineHandle,
    playback_recorder: PlaybackRecorderHandle,
    radio: RadioHandle,
    active_call: Option<Call>,
    unanswered_call_guard: Option<UnansweredCallGuard>,
    held_calls: HashMap<CallId, Call>, // call_id -> call
    pub(crate) outgoing_call: Option<CallInvite>,
    pub(crate) incoming_calls: HashMap<CallId, CallInvite>,
    pub test_profile_watcher: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
    pub(crate) client_id: Option<ClientId>,
    pub(crate) connection_state: ConnectionState,
    pub(crate) session_info: Option<server::SessionInfo>,
    pub(crate) default_call_sources: Vec<StationId>,
    pub(crate) stations: Vec<server::StationInfo>,
    pub(crate) clients: Vec<server::ClientInfo>,
}

pub type AppState = TokioMutex<AppStateInner>;

impl AppStateInner {
    /// Builds the mock audio backend, honoring `VACS_MOCK_AUDIO_CONFIG`:
    /// when set, it must point to a TOML file deserializing into a
    /// `MockBackendConfig`, allowing tests to define custom device layouts.
    #[cfg(feature = "mock-audio")]
    fn mock_audio_backend() -> anyhow::Result<vacs_audio::backend::mock::MockBackend> {
        use anyhow::Context;

        let Some(config_path) = std::env::var_os("VACS_MOCK_AUDIO_CONFIG") else {
            return Ok(vacs_audio::backend::mock::MockBackend::default());
        };

        log::info!("Loading mock audio config from {config_path:?}");
        let raw = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read mock audio config {config_path:?}"))?;
        let config = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse mock audio config {config_path:?}"))?;
        Ok(vacs_audio::backend::mock::MockBackend::new(config))
    }

    pub fn new(app: &AppHandle) -> Result<Self, StartupError> {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_startup_err(StartupError::Config)?;

        let config = AppConfig::parse(&config_dir).map_startup_err(StartupError::Config)?;
        let shutdown_token = CancellationToken::new();

        // Log the selected backend: a mock-audio build can never use real
        // audio, which must be diagnosable from the logs.
        #[cfg(feature = "mock-audio")]
        let audio_backend: Arc<dyn vacs_audio::backend::AudioBackend> = {
            log::info!("Using mock audio backend (mock-audio feature enabled)");
            Arc::new(Self::mock_audio_backend().map_startup_err(StartupError::Audio)?)
        };
        #[cfg(not(feature = "mock-audio"))]
        let audio_backend: Arc<dyn vacs_audio::backend::AudioBackend> = {
            log::info!("Using cpal audio backend");
            Arc::new(vacs_audio::backend::cpal::CpalBackend)
        };

        Ok(Self {
            config: config.clone(),
            signaling_client: Self::new_signaling_client(
                app.clone(),
                &config.backend.ws_url,
                shutdown_token.child_token(),
                config.client.max_signaling_reconnect_attempts(),
            ),
            audio_manager: Arc::new(RwLock::new(
                AudioManager::new(audio_backend, app.clone(), &config.audio)
                    .map_startup_err(StartupError::Audio)?,
            )),
            keybind_engine: Arc::new(TokioRwLock::new(KeybindEngine::new(
                app.clone(),
                &config.client.transmit_config,
                &config.client.keybinds,
                config.client.radio.integration.is_some(),
                shutdown_token.child_token(),
            ))),
            playback_recorder: Arc::new(RwLock::new(None)),
            radio: Arc::new(RwLock::new(None)),
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
            default_call_sources: Vec::new(),
            stations: Vec::new(),
            clients: Vec::new(),
        })
    }

    pub async fn shutdown(&self) {
        self.shutdown_token.cancel();
        let recorder = self.playback_recorder.write().take();
        if let Some(recorder) = recorder {
            recorder.shutdown().await;
        }
    }
}
