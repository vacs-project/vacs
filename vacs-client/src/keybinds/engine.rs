use crate::app::state::AppState;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::webrtc::AppStateWebrtcExt;
use crate::audio::manager::AudioManagerHandle;
use crate::config::{KeybindsConfig, RadioConfig, TransmitConfig, TransmitMode};
use crate::error::Error;
use crate::keybinds::runtime::{DynKeybindListener, KeybindListener, PlatformListener};
use crate::keybinds::{KeyEvent, Keybind};
use crate::radio::{DynRadio, RadioState, TransmissionState};
use keyboard_types::{Code, KeyState};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "linux")]
use crate::platform::Platform;

#[derive(Debug)]
pub struct KeybindEngine {
    mode: TransmitMode,
    transmit_code: Option<Code>,
    accept_call_code: Option<Code>,
    end_call_code: Option<Code>,
    toggle_radio_prio_code: Option<Code>,
    radio_config: RadioConfig,
    app: AppHandle,
    listener: RwLock<Option<DynKeybindListener>>,
    radio: RwLock<Option<DynRadio>>,
    rx_task: Option<JoinHandle<()>>,
    shutdown_token: CancellationToken,
    stop_token: Option<CancellationToken>,
    pressed: Arc<AtomicBool>,
    call_active: Arc<AtomicBool>,
    radio_prio: Arc<AtomicBool>,
    implicit_radio_prio: Arc<AtomicBool>,
}

pub type KeybindEngineHandle = Arc<TokioRwLock<KeybindEngine>>;

impl KeybindEngine {
    pub fn new(
        app: AppHandle,
        transmit_config: &TransmitConfig,
        call_control_config: &KeybindsConfig,
        radio_config: &RadioConfig,
        shutdown_token: CancellationToken,
    ) -> Self {
        Self {
            mode: transmit_config.mode,
            transmit_code: Self::select_active_transmit_code(transmit_config),
            accept_call_code: Self::select_accept_call_code(call_control_config),
            end_call_code: Self::select_end_call_code(call_control_config),
            toggle_radio_prio_code: Self::select_toggle_radio_prio_code(call_control_config),
            radio_config: radio_config.clone(),
            app,
            listener: RwLock::new(None),
            radio: RwLock::new(None),
            rx_task: None,
            shutdown_token,
            stop_token: None,
            pressed: Arc::new(AtomicBool::new(false)),
            call_active: Arc::new(AtomicBool::new(false)),
            radio_prio: Arc::new(AtomicBool::new(false)),
            implicit_radio_prio: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&mut self) -> Result<(), Error> {
        if self.rx_task.is_some() {
            return Ok(());
        }
        let has_call_controls = self.accept_call_code.is_some()
            || self.end_call_code.is_some()
            || self.toggle_radio_prio_code.is_some();

        if self.mode == TransmitMode::VoiceActivation && !has_call_controls {
            log::trace!(
                "TransmitMode set to voice activation and no call controls defined, no keybind engine required"
            );
            return Ok(());
        } else if self.mode != TransmitMode::VoiceActivation && self.transmit_code.is_none() {
            log::trace!(
                "No keybind set for TransmitMode {:?}, keybind engine not starting",
                self.mode
            );
            return Ok(());
        }

        self.stop_token = Some(self.shutdown_token.child_token());

        let (listener, rx) = PlatformListener::start().await?;
        *self.listener.write() = Some(Arc::new(listener));

        if self.mode == TransmitMode::RadioIntegration {
            let replay = self
                .app
                .state::<AppState>()
                .lock()
                .await
                .config
                .client
                .replay
                .clone();
            let radio = self.radio_config.radio(self.app.clone(), &replay).await?;
            *self.radio.write() = radio;
        } else {
            self.app.emit("radio:integration-available", false).ok();
        }

        self.spawn_rx_loop(rx);

        Ok(())
    }

    pub fn stop(&mut self) {
        {
            let mut listener = self.listener.write();
            if listener.take().is_some() {
                self.reset_input_state();
            }
        }

        self.radio.write().take();
        self.app.emit("radio:integration-available", false).ok();

        if let Some(stop_token) = self.stop_token.take() {
            stop_token.cancel();
        }

        if let Some(rx_task) = self.rx_task.take() {
            rx_task.abort();
        }
    }

    pub fn shutdown(&mut self) {
        self.shutdown_token.cancel();
        self.stop();
    }

    pub async fn set_config(
        &mut self,
        transmit_config: &TransmitConfig,
        keybinds_config: &KeybindsConfig,
    ) -> Result<(), Error> {
        self.stop();

        self.transmit_code = Self::select_active_transmit_code(transmit_config);
        self.mode = transmit_config.mode;

        self.accept_call_code = Self::select_accept_call_code(keybinds_config);
        self.end_call_code = Self::select_end_call_code(keybinds_config);
        self.toggle_radio_prio_code = Self::select_toggle_radio_prio_code(keybinds_config);

        self.reset_input_state();

        self.start().await?;

        Ok(())
    }

    pub async fn set_radio_config(&mut self, config: &RadioConfig) -> Result<(), Error> {
        self.stop();

        self.radio_config = config.clone();

        self.reset_input_state();

        self.start().await?;

        Ok(())
    }

    pub async fn reconnect_radio(&self) -> Result<(), Error> {
        let radio = self.radio.read().clone();
        if let Some(radio) = radio {
            log::info!("Reconnecting radio integration");
            radio
                .reconnect()
                .await
                .map_err(|err| Error::Radio(Box::new(err)))?;
        }
        Ok(())
    }

    pub fn set_call_active(&self, active: bool) {
        self.call_active.store(active, Ordering::Relaxed);

        if active {
            if matches!(self.mode, TransmitMode::RadioIntegration)
                && self.pressed.load(Ordering::Relaxed)
                && !self.radio_prio.load(Ordering::Relaxed)
            {
                log::trace!(
                    "Setting implicit radio prio after entering call while {:?} key is pressed",
                    self.mode
                );

                self.radio_prio.store(true, Ordering::Relaxed);
                self.implicit_radio_prio.store(true, Ordering::Relaxed);
                self.app.emit("audio:implicit-radio-prio", true).ok();
            }
        } else {
            self.implicit_radio_prio.store(false, Ordering::Relaxed);
            self.radio_prio.store(false, Ordering::Relaxed);
            self.app.emit("audio:implicit-radio-prio", false).ok();
        }
    }

    pub fn call_active(&self) -> bool {
        self.call_active.load(Ordering::Relaxed)
    }

    pub fn set_radio_prio(&self, prio: bool) {
        let prev_prio = self.radio_prio.swap(prio, Ordering::Relaxed);
        if !prio && prev_prio && self.pressed.load(Ordering::Relaxed) {
            log::trace!(
                "Radio prio unset while {:?} key is pressed, setting implicit radio prio for cleanup",
                self.mode
            );
            self.implicit_radio_prio.store(true, Ordering::Relaxed);
        }

        match (&self.mode, self.pressed.load(Ordering::Relaxed)) {
            (TransmitMode::VoiceActivation, _) | (TransmitMode::PushToMute, false) => {
                log::info!(
                    "Setting audio input {}",
                    if prio { "muted" } else { "unmuted" }
                );
                self.app
                    .state::<AudioManagerHandle>()
                    .read()
                    .set_input_muted(prio);
            }
            _ => {}
        }
    }

    pub fn radio_prio(&self) -> bool {
        self.radio_prio.load(Ordering::Relaxed) || self.implicit_radio_prio.load(Ordering::Relaxed)
    }

    pub fn should_attach_input_muted(&self) -> bool {
        match (&self.mode, self.pressed.load(Ordering::Relaxed)) {
            (TransmitMode::PushToTalk, false) => true,
            (TransmitMode::PushToMute, true) => true,
            (TransmitMode::RadioIntegration, false) => true,
            (TransmitMode::RadioIntegration, true) => self.radio_prio.load(Ordering::Relaxed),
            _ => false,
        }
    }

    pub fn radio_state(&self) -> RadioState {
        if let Some(radio) = self.radio.read().as_ref() {
            radio.state()
        } else {
            RadioState::NotConfigured
        }
    }

    pub fn radio(&self) -> Option<DynRadio> {
        self.radio.read().clone()
    }

    /// Get the external (OS-configured) key for a keybind, if available.
    ///
    /// On Wayland, keybinds are configured at the OS level via the XDG Global Shortcuts
    /// portal. This method queries the listener to get the actual key combination the
    /// user configured in their desktop environment.
    ///
    /// Returns `None` on all other platforms where keybinds are configured in-app.
    #[cfg(target_os = "linux")]
    pub fn get_external_binding(&self, keybind: Keybind) -> Option<String> {
        if matches!(Platform::get(), Platform::LinuxWayland) {
            return self
                .listener
                .read()
                .as_ref()
                .and_then(|l| l.get_external_binding(keybind));
        }
        None
    }

    /// Get the external (OS-configured) key for a keybind, if available.
    ///
    /// Returns `None` on all other platforms where keybinds are configured in-app.
    #[cfg(not(target_os = "linux"))]
    pub fn get_external_binding(&self, _keybind: Keybind) -> Option<String> {
        None
    }

    fn reset_input_state(&self) {
        self.pressed.store(false, Ordering::Relaxed);

        let muted = match &self.mode {
            TransmitMode::PushToTalk | TransmitMode::RadioIntegration => true,
            TransmitMode::PushToMute | TransmitMode::VoiceActivation => false,
        };

        log::trace!(
            "Resetting audio input {}",
            if muted { "muted" } else { "unmuted" }
        );

        self.app
            .state::<AudioManagerHandle>()
            .read()
            .set_input_muted(muted);
    }

    async fn handle_call_control_event(
        app: &AppHandle,
        code: &Code,
        accept_call: &Option<Code>,
        end_call: &Option<Code>,
        toggle_radio_prio: &Option<Code>,
    ) {
        let shared_call_controls = accept_call == end_call;

        if shared_call_controls
            && (accept_call.is_some_and(|c| c == *code) || end_call.is_some_and(|c| c == *code))
        {
            log::trace!("Shared call control key pressed");

            let state = app.state::<AppState>();
            let mut state = state.lock().await;

            if state.active_call_id().is_some() || state.outgoing_call_id().is_some() {
                match state.end_call(app, None).await {
                    Ok(found) if !found => log::trace!("No active call to end via keybind"),
                    Err(err) => log::warn!("Failed to end active call via keybind: {err}"),
                    _ => {}
                }
            } else {
                match state.accept_call(app, None).await {
                    Ok(found) if !found => log::trace!("No incoming call to accept via keybind"),
                    Err(err) => log::warn!("Failed to accept incoming call via keybind: {err}"),
                    _ => {}
                }
            }
        } else if accept_call.is_some_and(|c| c == *code) {
            log::trace!("Accept call key pressed");

            let state = app.state::<AppState>();
            let mut state = state.lock().await;

            match state.accept_call(app, None).await {
                Ok(found) if !found => log::trace!("No incoming call to accept via keybind"),
                Err(err) => log::warn!("Failed to accept incoming call via keybind: {err}"),
                _ => {}
            }
        } else if end_call.is_some_and(|c| c == *code) {
            log::trace!("End call key pressed");

            let state = app.state::<AppState>();
            let mut state = state.lock().await;

            match state.end_call(app, None).await {
                Ok(found) if !found => log::trace!("No active call to end via keybind"),
                Err(err) => log::warn!("Failed to end active call via keybind: {err}"),
                _ => {}
            }
        } else if toggle_radio_prio.is_some_and(|c| c == *code) {
            log::trace!("Toggle radio prio key pressed");

            let keybind_engine = app.state::<KeybindEngineHandle>();
            let keybind_engine = keybind_engine.read().await;

            if keybind_engine.call_active() {
                let prio = !keybind_engine.radio_prio();
                log::trace!("Toggled radio prio {}", if prio { "on" } else { "off" });
                keybind_engine.set_radio_prio(prio);
                app.emit("audio:radio-prio", prio).ok();
            }
        }
    }

    fn spawn_rx_loop(&mut self, mut rx: UnboundedReceiver<KeyEvent>) {
        let app = self.app.clone();
        let transmit = self.transmit_code;
        let accept_call = self.accept_call_code;
        let end_call = self.end_call_code;
        let toggle_radio_prio = self.toggle_radio_prio_code;

        if transmit.is_none()
            && accept_call.is_none()
            && end_call.is_none()
            && toggle_radio_prio.is_none()
        {
            return;
        }

        let mode = self.mode;
        let stop_token = self
            .stop_token
            .clone()
            .unwrap_or(self.shutdown_token.child_token());
        let radio = self.radio.read().clone();
        let pressed = self.pressed.clone();
        let call_active = self.call_active.clone();
        let radio_prio = self.radio_prio.clone();
        let implicit_radio_prio = self.implicit_radio_prio.clone();

        let handle = tauri::async_runtime::spawn(async move {
            log::debug!(
                "Keybind engine starting: mode={mode:?}, transmit={transmit:?}, accept_call={accept_call:?}, end_call={end_call:?}",
            );

            loop {
                tokio::select! {
                    biased;
                    _ = stop_token.cancelled() => break,
                    res = rx.recv() => {
                        let Some(event) = res else { break; };

                        if event.state == KeyState::Down {
                            Self::handle_call_control_event(&app, &event.code, &accept_call, &end_call, &toggle_radio_prio).await;
                        }

                        if transmit.is_none_or(|c| c != event.code) {
                            continue;
                        }

                        let muted = match (&mode, &event.state) {
                            (TransmitMode::PushToTalk | TransmitMode::RadioIntegration, KeyState::Down) if !pressed.swap(true, Ordering::Relaxed) => false,
                            (TransmitMode::PushToTalk | TransmitMode::RadioIntegration, KeyState::Up) if pressed.swap(false, Ordering::Relaxed) => true,
                            (TransmitMode::PushToMute, KeyState::Down) if !pressed.swap(true, Ordering::Relaxed) => true,
                            (TransmitMode::PushToMute, KeyState::Up) if pressed.swap(false, Ordering::Relaxed) => false,
                            _ => continue,
                        };

                        match (&mode, call_active.load(Ordering::Relaxed), radio_prio.load(Ordering::Relaxed)) {
                            (TransmitMode::RadioIntegration, false, _) => {
                                let state = event.state.into();
                                if let Some(radio) = radio.as_ref() {
                                    log::trace!("No call active, setting radio transmission {state:?}");
                                    Self::set_radio_transmit(radio, state).await;
                                } else {
                                    log::trace!("No call active, radio not initialized, cannot set transmission {state:?}");
                                }
                            },
                            (TransmitMode::RadioIntegration, true, false) => {
                                log::trace!("Call active, no radio prio, setting audio input {}", if muted { "muted" } else { "unmuted" });
                                Self::set_input_muted(&app, muted);
                            },
                            (TransmitMode::RadioIntegration, true, true) => {
                                let state = event.state.into();
                                if let Some(radio) = radio.as_ref() {
                                    log::trace!("Call active, radio prio set, setting audio input muted and radio transmission {state:?}");
                                    Self::set_input_muted(&app, true);
                                    Self::set_radio_transmit(radio, state).await;
                                } else {
                                    log::trace!("Call active, radio prio set, radio not initialized, setting audio input muted, but cannot set transmission {state:?}");
                                    Self::set_input_muted(&app, true);
                                }
                            }
                            (TransmitMode::PushToTalk | TransmitMode::PushToMute, true, false) => {
                                log::trace!("Call active, setting audio input {}", if muted { "muted" } else { "unmuted" });
                                Self::set_input_muted(&app, muted);
                            },
                            (TransmitMode::PushToTalk, true, true) => {
                                log::trace!("Call active, would set audio input {}, but radio prio is set, so keeping audio input muted", if muted { "muted" } else { "unmuted" });
                                Self::set_input_muted(&app, true);
                            }
                            _ => {}

                        }

                        if event.state.is_up() && implicit_radio_prio.swap(false, Ordering::Relaxed) {
                            if radio_prio.swap(false, Ordering::Relaxed) {
                                log::trace!("Implicit radio prio cleared on {:?} key release", mode);
                                app.emit("audio:implicit-radio-prio", false).ok();
                            } else if let Some(radio) = radio.as_ref() {
                                log::trace!("Implicit radio prio cleared on {mode:?} key release, but radio prio was not set. Setting transmission Inactive");
                                Self::set_radio_transmit(radio, TransmissionState::Inactive).await;
                            } else {
                                log::trace!("Implicit radio prio cleared on {mode:?} key release, but radio not initialized, ignoring");
                            }
                        }
                    }
                }
            }

            log::trace!("Keybinds engine loop finished");
        });

        self.rx_task = Some(handle);
    }

    #[inline]
    fn select_active_transmit_code(config: &TransmitConfig) -> Option<Code> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // Wayland Code Mapping Strategy:
            //
            // On Wayland, shortcuts are configured at the OS level via the XDG Global Shortcuts
            // portal. The portal allows complex key combinations (e.g., Ctrl+Alt+Shift+P) that
            // cannot be represented as a single keyboard_types::Code.
            //
            // To work around this, we map each transmit mode to a unique, unlikely-to-be-pressed
            // function key (F33-F35). These keys don't exist on most keyboards, so there's no
            // conflict with user input. When the portal activates a shortcut, we emit the
            // corresponding F-key code, and the rest of the keybind engine works unchanged.
            //
            // This effectively overrides the user-configured codes in the config file on Wayland,
            // since the actual key binding is managed by the desktop environment.
            let code = match config.mode {
                TransmitMode::VoiceActivation => None,
                TransmitMode::PushToTalk => Some(Code::F33),
                TransmitMode::PushToMute => Some(Code::F34),
                TransmitMode::RadioIntegration => Some(Code::F35),
            };
            log::trace!(
                "Using portal shortcut code {code:?} for transmit mode {:?}",
                config.mode
            );
            return code;
        }

        match config.mode {
            TransmitMode::VoiceActivation => None,
            TransmitMode::PushToTalk => config.push_to_talk,
            TransmitMode::PushToMute => config.push_to_mute,
            TransmitMode::RadioIntegration => config.radio_push_to_talk,
        }
    }

    #[inline]
    fn select_accept_call_code(config: &KeybindsConfig) -> Option<Code> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // Wayland Code Mapping Strategy:
            // Same as with the transmit code, we define our global shortcuts on OS level.
            // As we cannot bind the same key to multiple actions, we'll always use F32
            // as both accept and end call key.
            return Some(Code::F32);
        }

        config.accept_call
    }

    #[inline]
    fn select_end_call_code(config: &KeybindsConfig) -> Option<Code> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // Wayland Code Mapping Strategy:
            // Same as with the transmit code, we define our global shortcuts on OS level.
            // As we cannot bind the same key to multiple actions, we'll always use F32
            // as both accept and end call key.
            return Some(Code::F32);
        }

        config.end_call
    }

    #[inline]
    fn select_toggle_radio_prio_code(config: &KeybindsConfig) -> Option<Code> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // Wayland Code Mapping Strategy:
            // Same as with the transmit code, we define our global shortcuts on OS level.
            return Some(Code::F31);
        }

        config.toggle_radio_prio
    }

    #[inline]
    fn set_input_muted(app: &AppHandle, muted: bool) {
        app.state::<AudioManagerHandle>()
            .read()
            .set_input_muted(muted);
    }

    #[inline]
    async fn set_radio_transmit(radio: &DynRadio, state: TransmissionState) {
        if let Err(err) = radio.transmit(state).await {
            log::warn!("Failed to set radio transmission state {state:?}: {err}");
        }
    }
}

impl Drop for KeybindEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
