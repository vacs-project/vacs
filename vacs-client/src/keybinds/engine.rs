use crate::app::state::AppState;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::webrtc::refresh_expired_ice_config;
use crate::audio::manager::AudioManagerHandle;
use crate::error::Error;
use crate::keybinds::joystick::JoystickServiceHandle;
use crate::keybinds::runtime::{
    DynKeybindListener, KeybindListener, PlatformListener, WeakKeybindListener,
};
use crate::keybinds::{
    CallMicMode, InputCode, KeyEvent, Keybind, KeybindsConfig, TransmitConfig, Trigger,
};
use crate::radio::{RadioHandle, TransmissionState};
use keyboard_types::KeyState;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "linux")]
use crate::keybinds::{PortalAction, compose_wayland_trigger};
#[cfg(target_os = "linux")]
use crate::platform::Platform;

#[derive(Debug)]
pub struct KeybindEngine {
    call_mic_mode: CallMicMode,
    call_trigger: Option<Trigger>,
    radio_trigger: Option<Trigger>,
    accept_call_trigger: Option<Trigger>,
    end_call_trigger: Option<Trigger>,
    toggle_radio_prio_trigger: Option<Trigger>,
    app: AppHandle,
    listener: RwLock<Option<DynKeybindListener>>,
    rx_task: Option<JoinHandle<()>>,
    control_task: Option<JoinHandle<()>>,
    shutdown_token: CancellationToken,
    stop_token: Option<CancellationToken>,
    /// Whether this configuration lets radio TX fall back to the call trigger
    /// when no dedicated radio shortcut is bound at the OS level.
    radio_portal_fallback: bool,
    /// Whether that fallback is active right now. Resolved from the listener's
    /// live bindings, which change while the app runs.
    radio_follows_call: Arc<AtomicBool>,
    call_pressed: Arc<AtomicBool>,
    radio_pressed: Arc<AtomicBool>,
    call_active: Arc<AtomicBool>,
    radio_prio: Arc<AtomicBool>,
    implicit_radio_prio: Arc<AtomicBool>,
    radio_transmitting: Arc<AtomicBool>,
}

pub type KeybindEngineHandle = Arc<TokioRwLock<KeybindEngine>>;

impl KeybindEngine {
    pub fn new(
        app: AppHandle,
        transmit_config: &TransmitConfig,
        call_control_config: &KeybindsConfig,
        radio_integration_enabled: bool,
        shutdown_token: CancellationToken,
    ) -> Self {
        let radio_portal_fallback =
            transmit_config.radio_falls_back_to_call(radio_integration_enabled);

        Self {
            call_mic_mode: transmit_config.call_mic_mode,
            call_trigger: transmit_config.active_call_trigger(),
            radio_trigger: transmit_config.active_radio_trigger(radio_integration_enabled),
            accept_call_trigger: Self::select_accept_call_trigger(call_control_config),
            end_call_trigger: Self::select_end_call_trigger(call_control_config),
            toggle_radio_prio_trigger: Self::select_toggle_radio_prio_trigger(call_control_config),
            app,
            listener: RwLock::new(None),
            rx_task: None,
            control_task: None,
            shutdown_token,
            stop_token: None,
            radio_portal_fallback,
            radio_follows_call: Arc::new(AtomicBool::new(radio_portal_fallback)),
            call_pressed: Arc::new(AtomicBool::new(false)),
            radio_pressed: Arc::new(AtomicBool::new(false)),
            call_active: Arc::new(AtomicBool::new(false)),
            radio_prio: Arc::new(AtomicBool::new(false)),
            implicit_radio_prio: Arc::new(AtomicBool::new(false)),
            radio_transmitting: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether any active trigger is a joystick button (requiring the shared
    /// joystick service to be running).
    fn any_button_trigger(&self) -> bool {
        [
            &self.call_trigger,
            &self.radio_trigger,
            &self.accept_call_trigger,
            &self.end_call_trigger,
            &self.toggle_radio_prio_trigger,
        ]
        .into_iter()
        .flatten()
        .any(|trigger| matches!(trigger, Trigger::Input(InputCode::Button(_))))
    }

    pub async fn start(&mut self) -> Result<(), Error> {
        if self.rx_task.is_some() {
            return Ok(());
        }
        let has_call_controls = self.accept_call_trigger.is_some()
            || self.end_call_trigger.is_some()
            || self.toggle_radio_prio_trigger.is_some();

        if self.call_mic_mode == CallMicMode::VoiceActivation
            && self.radio_trigger.is_none()
            && !has_call_controls
        {
            log::trace!(
                "TransmitMode set to voice activation, no radio PTT set and no call controls defined -> no keybind engine required"
            );
            return Ok(());
        } else if self.call_mic_mode != CallMicMode::VoiceActivation
            && self.call_trigger.is_none()
            && self.radio_trigger.is_none()
        {
            log::trace!(
                "No keybind set for TransmitMode {:?}, keybind engine not starting",
                self.call_mic_mode
            );
            return Ok(());
        }

        self.stop_token = Some(self.shutdown_token.child_token());

        let any_button = self.any_button_trigger();

        // All input sources write into this one channel; the engine loop below
        // reads the merged stream directly.
        let (key_event_tx, key_event_rx) = unbounded_channel();

        // A keyboard listener failure (e.g. portal unavailable on Wayland) must
        // not disable joystick bindings, and vice versa: start whichever sources
        // are available and only fail if none are.
        let keyboard_ok = match PlatformListener::start(key_event_tx.clone()).await {
            Ok(listener) => {
                *self.listener.write() = Some(Arc::new(listener));
                true
            }
            Err(err) if any_button => {
                log::error!(
                    "Keybind listener failed to start, continuing with joystick bindings only: {err}"
                );
                false
            }
            Err(err) => return Err(err.into()),
        };

        if any_button
            && let Err(err) = self
                .app
                .state::<JoystickServiceHandle>()
                .register(key_event_tx)
                .await
        {
            if !keyboard_ok {
                return Err(err.into());
            }
            log::error!(
                "Joystick service failed to start, continuing with keyboard bindings only: {err}"
            );
        }

        self.refresh_radio_follows_call();
        self.spawn_rx_loop(key_event_rx);

        Ok(())
    }

    fn refresh_radio_follows_call(&self) {
        refresh_radio_follows_call(
            self.listener.read().as_ref().map(Arc::downgrade).as_ref(),
            self.radio_portal_fallback,
            &self.radio_follows_call,
            &self.call_pressed,
            &self.radio_pressed,
        );
    }

    /// Whether radio TX and the call MIC action share one trigger, either
    /// because they are configured to the same input or because the portal
    /// fallback is active.
    ///
    /// Reads the fallback without re-resolving it. Callers run on other tasks,
    /// and the event loop classifies a press before it marks the key as held, so
    /// a refresh from here could flip the fallback inside that gap and have the
    /// release classified differently from its press.
    fn radio_shares_call_trigger(&self) -> bool {
        self.radio_trigger.is_some()
            && (self.radio_trigger == self.call_trigger
                || self.radio_follows_call.load(Ordering::Relaxed))
    }

    pub fn stop(&mut self) {
        // The engine may run without a platform listener (joystick-only mode
        // when the keyboard listener failed to start), so the running state is
        // tracked by the rx task, not the listener.
        let was_running = self.rx_task.is_some();

        self.listener.write().take();

        if let Some(stop_token) = self.stop_token.take() {
            stop_token.cancel();
        }

        if let Some(rx_task) = self.rx_task.take() {
            rx_task.abort();
        }

        if let Some(control_task) = self.control_task.take() {
            control_task.abort();
        }

        if was_running {
            self.reset_input_state();
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
        radio_integration_enabled: bool,
    ) -> Result<(), Error> {
        self.stop();

        self.call_mic_mode = transmit_config.call_mic_mode;
        self.call_trigger = transmit_config.active_call_trigger();
        self.radio_trigger = transmit_config.active_radio_trigger(radio_integration_enabled);
        self.radio_portal_fallback =
            transmit_config.radio_falls_back_to_call(radio_integration_enabled);
        self.radio_follows_call
            .store(self.radio_portal_fallback, Ordering::Relaxed);

        self.accept_call_trigger = Self::select_accept_call_trigger(keybinds_config);
        self.end_call_trigger = Self::select_end_call_trigger(keybinds_config);
        self.toggle_radio_prio_trigger = Self::select_toggle_radio_prio_trigger(keybinds_config);

        self.reset_input_state();

        self.start().await?;

        Ok(())
    }

    pub fn set_call_active(&self, active: bool) {
        self.call_active.store(active, Ordering::Relaxed);

        if active {
            if self.radio_shares_call_trigger()
                && self.radio_pressed.load(Ordering::Relaxed)
                && self.radio_transmitting.load(Ordering::Relaxed)
                && !self.radio_prio.load(Ordering::Relaxed)
                && self.call_mic_mode != CallMicMode::VoiceActivation
            {
                log::trace!(
                    "Setting implicit radio prio after entering call while {:?} key is pressed",
                    self.call_mic_mode
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
        if !prio && prev_prio && self.radio_pressed.load(Ordering::Relaxed) {
            log::trace!(
                "Radio prio unset while {:?} key is pressed, setting implicit radio prio for cleanup",
                self.call_mic_mode
            );
            self.implicit_radio_prio.store(true, Ordering::Relaxed);
        }

        match (
            &self.call_mic_mode,
            self.call_pressed.load(Ordering::Relaxed),
        ) {
            (CallMicMode::VoiceActivation, _) | (CallMicMode::PushToMute, false) => {
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
        let call_pressed = self.call_pressed.load(Ordering::Relaxed);
        let radio_pressed = self.radio_pressed.load(Ordering::Relaxed);
        let radio_prio = self.radio_prio.load(Ordering::Relaxed);
        let separate_keys = self.radio_trigger.is_some() && !self.radio_shares_call_trigger();
        match self.call_mic_mode {
            // Radio prio mutes the call mic in these modes (see set_radio_prio);
            // an attach while prio is active must not lift that mute.
            CallMicMode::VoiceActivation => radio_prio,
            CallMicMode::PushToTalk => {
                if separate_keys {
                    // PTT-Diff: call PTT alone determines MIC state; prio has no effect (§8.4)
                    !call_pressed
                } else {
                    // PTT-Same/None: prio can force mute even while key held
                    !call_pressed || (radio_pressed && radio_prio)
                }
            }
            CallMicMode::PushToMute => call_pressed || radio_prio,
        }
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
        self.call_pressed.store(false, Ordering::Relaxed);
        self.radio_pressed.store(false, Ordering::Relaxed);

        // The rx task is aborted before this runs, so the key release that would
        // have keyed the radio down will never be processed. Without sending it
        // here, reconfiguring while the radio key is held leaves the radio
        // transmitting with no key left to release it.
        if self.radio_transmitting.swap(false, Ordering::Relaxed) {
            log::debug!("Radio transmit: false (input state reset)");

            let radio_handle = self.app.state::<RadioHandle>().inner().clone();
            tauri::async_runtime::spawn(async move {
                Self::set_radio_transmit(&radio_handle, TransmissionState::Inactive).await;
            });
        }

        let muted = match &self.call_mic_mode {
            CallMicMode::PushToTalk => true,
            CallMicMode::PushToMute | CallMicMode::VoiceActivation => false,
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
        trigger: &Trigger,
        accept_call: Option<&Trigger>,
        end_call: Option<&Trigger>,
        toggle_radio_prio: Option<&Trigger>,
    ) {
        let is_accept = accept_call == Some(trigger);
        let is_end = end_call == Some(trigger);

        if is_accept {
            // Outside the app state lock taken below; see refresh_expired_ice_config.
            refresh_expired_ice_config(app).await;
        }

        // A trigger bound to both accept and end (the same key configured for
        // both, or the shared Wayland portal call-control shortcut) toggles:
        // end the active/outgoing call if there is one, otherwise accept.
        if is_accept && is_end {
            log::trace!("Shared call control key pressed");

            let state = app.state::<AppState>();
            let mut state = state.lock().await;

            if let Some(call_id) = state.current_call_id() {
                match state.end_call(app, call_id).await {
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
        } else if is_accept {
            log::trace!("Accept call key pressed");

            let state = app.state::<AppState>();
            let mut state = state.lock().await;

            match state.accept_call(app, None).await {
                Ok(found) if !found => log::trace!("No incoming call to accept via keybind"),
                Err(Error::Webrtc(err))
                    if matches!(err.as_ref(), vacs_webrtc::error::WebrtcError::CallActive) =>
                {
                    log::debug!("Ignoring accept keybind while another call is active");
                }
                Err(err) => log::warn!("Failed to accept incoming call via keybind: {err}"),
                _ => {}
            }
        } else if is_end {
            log::trace!("End call key pressed");

            let state = app.state::<AppState>();
            let mut state = state.lock().await;

            if let Some(call_id) = state.current_call_id() {
                match state.end_call(app, call_id).await {
                    Ok(found) if !found => log::trace!("No active call to end via keybind"),
                    Err(err) => log::warn!("Failed to end active call via keybind: {err}"),
                    _ => {}
                }
            }
        } else if toggle_radio_prio == Some(trigger) {
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
        let call_trigger = self.call_trigger.clone();
        let radio_trigger = self.radio_trigger.clone();
        let accept_call = self.accept_call_trigger.clone();
        let end_call = self.end_call_trigger.clone();
        let toggle_radio_prio = self.toggle_radio_prio_trigger.clone();

        if call_trigger.is_none()
            && accept_call.is_none()
            && end_call.is_none()
            && toggle_radio_prio.is_none()
            && radio_trigger.is_none()
        {
            return;
        }

        let mode = self.call_mic_mode;
        let stop_token = self
            .stop_token
            .clone()
            .unwrap_or(self.shutdown_token.child_token());
        let radio_handle = self.app.state::<RadioHandle>().inner().clone();
        let radio_portal_fallback = self.radio_portal_fallback;
        let radio_follows_call = self.radio_follows_call.clone();
        // Weak, not owning: `stop()` drops the engine's handle before aborting
        // this task, and an owning clone here would keep the old listener (and
        // its portal session) alive until the runtime reclaims the aborted
        // task, overlapping it with the session `start()` opens next.
        let listener = self.listener.read().as_ref().map(Arc::downgrade);
        let call_pressed = self.call_pressed.clone();
        let radio_pressed = self.radio_pressed.clone();
        let call_active = self.call_active.clone();
        let radio_prio_arc = self.radio_prio.clone();
        let implicit_radio_prio = self.implicit_radio_prio.clone();
        let radio_transmitting = self.radio_transmitting.clone();

        let (control_tx, mut control_rx) = tokio::sync::mpsc::channel::<Trigger>(3);
        self.control_task = Some({
            let app = app.clone();
            let accept_call = accept_call.clone();
            let end_call = end_call.clone();
            let toggle_radio_prio = toggle_radio_prio.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(trigger) = control_rx.recv().await {
                    Self::handle_call_control_event(
                        &app,
                        &trigger,
                        accept_call.as_ref(),
                        end_call.as_ref(),
                        toggle_radio_prio.as_ref(),
                    )
                    .await;
                }
            })
        });

        let handle = tauri::async_runtime::spawn(async move {
            log::debug!(
                "Keybind engine starting: mode={mode:?}, transmit={call_trigger:?}, radio={radio_trigger:?}, accept_call={accept_call:?}, end_call={end_call:?}",
            );

            loop {
                tokio::select! {
                    biased;
                    _ = stop_token.cancelled() => break,
                    res = rx.recv() => {
                        let Some(event) = res else { break; };

                        let is_control_press = event.state == KeyState::Down
                            && [&accept_call, &end_call, &toggle_radio_prio]
                                .iter()
                                .any(|t| t.as_ref() == Some(&event.trigger));
                        if is_control_press && control_tx.try_send(event.trigger.clone()).is_err() {
                            log::trace!("Call control handler busy, dropping key press");
                        }

                        refresh_radio_follows_call(
                            listener.as_ref(),
                            radio_portal_fallback,
                            &radio_follows_call,
                            &call_pressed,
                            &radio_pressed,
                        );

                        let (is_call_key, is_radio_key) = classify_trigger(
                            &event.trigger,
                            call_trigger.as_ref(),
                            radio_trigger.as_ref(),
                            radio_follows_call.load(Ordering::Relaxed),
                        );

                        if !is_call_key && !is_radio_key { continue; }

                        let key_down = event.state == KeyState::Down;

                        if is_call_key && call_pressed.swap(key_down, Ordering::Relaxed) == key_down { continue; }
                        if is_radio_key && radio_pressed.swap(key_down, Ordering::Relaxed) == key_down { continue; }

                        let call_active = call_active.load(Ordering::Relaxed);
                        let radio_prio = radio_prio_arc.load(Ordering::Relaxed);
                        // Implicit prio (set at call entry for radio TX continuity) must not affect
                        // MIC dispatch - only explicit (user-toggled) prio changes MIC behaviour.
                        let effective_prio = radio_prio && !implicit_radio_prio.load(Ordering::Relaxed);

                        let separate = is_call_key ^ is_radio_key;

                        if is_radio_key && (separate || !call_active || radio_prio || mode != CallMicMode::PushToTalk) {
                            radio_transmitting.store(key_down, Ordering::Relaxed);
                            Self::set_radio_transmit(&radio_handle, event.state.into()).await;
                            log::debug!("Radio transmit: {key_down}");
                        }

                        if call_active {
                            let mic_action = match (mode, is_call_key, effective_prio) {
                                (CallMicMode::VoiceActivation, ..) => None,

                                // PTT call key: follows key state, or mute-locked when explicit prio is on (§8.3/§8.4/§8.5)
                                (CallMicMode::PushToTalk, true, false) => Some(!key_down),
                                (CallMicMode::PushToTalk, true, true) => Some(true),

                                // PTM: follows key state; explicit prio suppresses MIC changes (§8.6/§8.7)
                                (CallMicMode::PushToMute, _, false) => Some(key_down),

                                // PTT radio key (Diff config): MIC unchanged; radio TX handled above (§8.4)
                                _ => None,
                            };

                            if let Some(muted) = mic_action {
                                Self::set_input_muted(&app, muted);
                            }
                        }

                        if !key_down && is_radio_key && implicit_radio_prio.swap(false, Ordering::Relaxed) {
                            if radio_prio_arc.swap(false, Ordering::Relaxed) {
                                app.emit("audio:implicit-radio-prio", false).ok();
                            } else {
                                radio_transmitting.store(false, Ordering::Relaxed);
                                // prio was already cleared externally; ensure radio TX stops
                                Self::set_radio_transmit(&radio_handle, TransmissionState::Inactive).await;
                                log::debug!("Radio transmit: false (implicit)");
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
    fn select_accept_call_trigger(config: &KeybindsConfig) -> Option<Trigger> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // The portal exposes a single shared call-control shortcut (end
            // active / accept next), so both accept and end carry the same
            // portal action; a configured joystick button replaces it.
            return compose_wayland_trigger(Some(PortalAction::CallControl), &config.accept_call);
        }

        config.accept_call.clone().map(Trigger::Input)
    }

    #[inline]
    fn select_end_call_trigger(config: &KeybindsConfig) -> Option<Trigger> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // See select_accept_call_trigger: shared portal call-control shortcut.
            return compose_wayland_trigger(Some(PortalAction::CallControl), &config.end_call);
        }

        config.end_call.clone().map(Trigger::Input)
    }

    #[inline]
    fn select_toggle_radio_prio_trigger(config: &KeybindsConfig) -> Option<Trigger> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            return compose_wayland_trigger(
                Some(PortalAction::ToggleRadioPrio),
                &config.toggle_radio_prio,
            );
        }

        config.toggle_radio_prio.clone().map(Trigger::Input)
    }

    #[inline]
    fn set_input_muted(app: &AppHandle, muted: bool) {
        app.state::<AudioManagerHandle>()
            .read()
            .set_input_muted(muted);
    }

    #[inline]
    async fn set_radio_transmit(radio_handle: &RadioHandle, state: TransmissionState) {
        let radio = radio_handle.read().clone();
        if let Some(radio) = radio
            && let Err(err) = radio.transmit(state).await
        {
            log::warn!("Failed to set radio transmission state {state:?}: {err}");
        }
    }
}

impl Drop for KeybindEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Classifies an incoming trigger as the call key, the radio key, both or neither.
///
/// Exactly one thing can be the radio key. While the fallback is active the
/// dedicated shortcut is unbound, so also honoring it would let a momentarily
/// stale fallback count one press twice against the engine's shared
/// `radio_pressed` flag, swallow the matching release and leave the transmitter
/// keyed. A single trigger configured for both still classifies as both, which
/// is how the non-portal platforms express "radio follows the call key".
fn classify_trigger(
    trigger: &Trigger,
    call_trigger: Option<&Trigger>,
    radio_trigger: Option<&Trigger>,
    radio_follows_call: bool,
) -> (bool, bool) {
    let is_call_key = call_trigger == Some(trigger);
    let is_radio_key = if radio_follows_call {
        is_call_key
    } else {
        radio_trigger == Some(trigger)
    };

    (is_call_key, is_radio_key)
}

/// Re-resolves whether radio TX follows the call trigger, from the listener's
/// live view of the OS level bindings.
///
/// Never re-resolves while a key is held: flipping the fallback mid-press would
/// classify the release differently from the press and leave the radio keyed.
/// Only ever called from `start()` and from the event loop, which never overlap,
/// so the held-key check and the write it guards cannot interleave with another
/// writer.
///
/// Runs on every key event, so the two cheap guards come first and the listener
/// is only resolved once they pass.
fn refresh_radio_follows_call(
    listener: Option<&WeakKeybindListener>,
    fallback: bool,
    follows_call: &AtomicBool,
    call_pressed: &AtomicBool,
    radio_pressed: &AtomicBool,
) {
    // Without the portal fallback there is nothing to resolve: `follows_call` is
    // false from construction and no binding can change that.
    if !fallback || call_pressed.load(Ordering::Relaxed) || radio_pressed.load(Ordering::Relaxed) {
        return;
    }

    let follows = !listener
        .and_then(|l| l.upgrade())
        .is_some_and(|l| l.has_external_binding(Keybind::RadioPushToTalk));

    if follows_call.swap(follows, Ordering::Relaxed) != follows {
        log::debug!(
            "Radio TX now follows the {} trigger",
            if follows { "call" } else { "radio" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinds::{KeybindsError, PortalAction};
    use tokio::sync::mpsc::UnboundedSender;

    /// Stands in for a platform listener so the resolution can be exercised
    /// without a portal, an `AppHandle` or a running event loop.
    #[derive(Debug)]
    struct FakeListener {
        radio_bound: bool,
    }

    impl KeybindListener for FakeListener {
        async fn start(_key_event_tx: UnboundedSender<KeyEvent>) -> Result<Self, KeybindsError> {
            unreachable!("test listeners are constructed directly")
        }

        fn has_external_binding(&self, keybind: Keybind) -> bool {
            self.radio_bound && keybind == Keybind::RadioPushToTalk
        }
    }

    fn listener(radio_bound: bool) -> DynKeybindListener {
        Arc::new(FakeListener { radio_bound })
    }

    struct Fixture {
        follows: AtomicBool,
        call_pressed: AtomicBool,
        radio_pressed: AtomicBool,
    }

    impl Fixture {
        fn new(follows: bool) -> Self {
            Self {
                follows: AtomicBool::new(follows),
                call_pressed: AtomicBool::new(false),
                radio_pressed: AtomicBool::new(false),
            }
        }

        fn refresh(&self, listener: Option<&WeakKeybindListener>, fallback: bool) -> bool {
            refresh_radio_follows_call(
                listener,
                fallback,
                &self.follows,
                &self.call_pressed,
                &self.radio_pressed,
            );
            self.follows.load(Ordering::Relaxed)
        }
    }

    fn call() -> Trigger {
        Trigger::Portal(PortalAction::PushToTalk)
    }

    fn radio() -> Trigger {
        Trigger::Portal(PortalAction::RadioPushToTalk)
    }

    #[test]
    fn radio_follows_the_call_key_while_no_dedicated_shortcut_is_bound() {
        let listener = listener(false);
        let fixture = Fixture::new(false);

        assert!(fixture.refresh(Some(&Arc::downgrade(&listener)), true));
    }

    #[test]
    fn radio_stops_following_the_call_key_once_a_shortcut_is_bound() {
        let listener = listener(true);
        let fixture = Fixture::new(true);

        assert!(!fixture.refresh(Some(&Arc::downgrade(&listener)), true));
    }

    #[test]
    fn configs_without_the_portal_fallback_never_resolve() {
        // A bound shortcut must not flip a config that has no fallback rule, so
        // every non-Wayland platform keeps its configured triggers verbatim.
        let listener = listener(false);
        let fixture = Fixture::new(false);

        assert!(!fixture.refresh(Some(&Arc::downgrade(&listener)), false));
    }

    #[test]
    fn the_fallback_never_flips_while_a_key_is_held() {
        // Flipping mid-press would classify the release differently from the
        // press and leave the radio keyed.
        let listener = listener(true);
        let weak = Arc::downgrade(&listener);

        let held_call = Fixture::new(true);
        held_call.call_pressed.store(true, Ordering::Relaxed);
        assert!(held_call.refresh(Some(&weak), true), "call key held");

        let held_radio = Fixture::new(true);
        held_radio.radio_pressed.store(true, Ordering::Relaxed);
        assert!(held_radio.refresh(Some(&weak), true), "radio key held");
    }

    #[test]
    fn a_dropped_listener_reads_as_unbound() {
        // `stop()` takes the listener while the event loop still holds its weak
        // handle; resolving through a dangling one must not panic.
        let weak = Arc::downgrade(&listener(true));
        let fixture = Fixture::new(false);

        assert!(fixture.refresh(Some(&weak), true));
        assert!(fixture.refresh(None, true));
    }

    #[test]
    fn separate_triggers_classify_independently() {
        assert_eq!(
            classify_trigger(&radio(), Some(&call()), Some(&radio()), false),
            (false, true)
        );
        assert_eq!(
            classify_trigger(&call(), Some(&call()), Some(&radio()), false),
            (true, false)
        );
    }

    #[test]
    fn the_fallback_makes_the_call_key_the_radio_key() {
        assert_eq!(
            classify_trigger(&call(), Some(&call()), Some(&radio()), true),
            (true, true)
        );
    }

    #[test]
    fn a_stale_fallback_never_double_counts_the_dedicated_trigger() {
        // Regression: classifying the dedicated shortcut as a radio key while the
        // fallback was still active let one press take `radio_pressed` twice, so
        // the release was swallowed and the transmitter stayed keyed.
        assert_eq!(
            classify_trigger(&radio(), Some(&call()), Some(&radio()), true),
            (false, false)
        );
    }

    #[test]
    fn one_trigger_bound_to_both_classifies_as_both() {
        assert_eq!(
            classify_trigger(&call(), Some(&call()), Some(&call()), false),
            (true, true)
        );
    }

    #[test]
    fn an_unrelated_trigger_classifies_as_neither() {
        let other = Trigger::Portal(PortalAction::CallControl);

        assert_eq!(
            classify_trigger(&other, Some(&call()), Some(&radio()), false),
            (false, false)
        );
        assert_eq!(
            classify_trigger(&other, Some(&call()), Some(&radio()), true),
            (false, false)
        );
    }
}
