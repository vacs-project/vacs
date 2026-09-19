use crate::app::PersistedClientConfig;
use crate::app::state::AppState;
use crate::config::{CLIENT_SETTINGS_FILE_NAME, Persistable};
use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use crate::keybinds::joystick::JoystickServiceHandle;
use crate::keybinds::{
    FrontendKeybindsConfig, FrontendTransmitConfig, InputCode, JoystickButton, JoystickDevice,
    Keybind, KeybindsConfig, TransmitConfig, Trigger,
};
use crate::platform::Capabilities;
use keyboard_types::KeyState;
use std::collections::HashSet;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

/// Default timeout for a "press a joystick button to bind" capture.
const DEFAULT_CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);

/// Ensure the platform can handle the given bindings: at least one input source
/// must be available, and keyboard bindings require the keyboard listener
/// (unavailable on X11/unknown Linux, where only joystick bindings work).
fn ensure_keybind_capability(inputs: &[&Option<InputCode>]) -> Result<(), Error> {
    let capabilities = Capabilities::default();

    if !capabilities.keybind_listener && !capabilities.joystick {
        return Err(Error::CapabilityNotAvailable("Keybinds".to_string()));
    }

    if !capabilities.keybind_listener
        && inputs
            .iter()
            .any(|input| matches!(input, Some(InputCode::Key(_))))
    {
        return Err(Error::CapabilityNotAvailable(
            "Keyboard keybinds".to_string(),
        ));
    }

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_get_transmit_config(
    app_state: State<'_, AppState>,
) -> Result<FrontendTransmitConfig, Error> {
    Ok(app_state
        .lock()
        .await
        .config
        .client
        .transmit_config
        .clone()
        .into())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_set_transmit_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
    keybind_engine: State<'_, KeybindEngineHandle>,
    transmit_config: FrontendTransmitConfig,
) -> Result<(), Error> {
    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        let transmit_config: TransmitConfig = transmit_config.try_into()?;

        ensure_keybind_capability(&[
            &transmit_config.push_to_talk,
            &transmit_config.push_to_mute,
            &transmit_config.radio_push_to_talk,
        ])?;

        state.config.client.radio.validate(&transmit_config)?;

        keybind_engine
            .write()
            .await
            .set_config(
                &transmit_config,
                &state.config.client.keybinds,
                state.config.client.radio.integration.is_some(),
            )
            .await?;

        state.config.client.transmit_config = transmit_config;
        state.config.client.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_get_keybinds_config(
    app_state: State<'_, AppState>,
) -> Result<FrontendKeybindsConfig, Error> {
    Ok(app_state.lock().await.config.client.keybinds.clone().into())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_set_binding(
    app: AppHandle,
    app_state: State<'_, AppState>,
    keybind_engine: State<'_, KeybindEngineHandle>,
    input: Option<InputCode>,
    keybind: Keybind,
) -> Result<(), Error> {
    ensure_keybind_capability(&[&input])?;

    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        let mut keybinds_config: KeybindsConfig = state.config.client.keybinds.clone();

        match keybind {
            Keybind::AcceptCall => keybinds_config.accept_call = input,
            Keybind::EndCall => keybinds_config.end_call = input,
            Keybind::ToggleRadioPrio => keybinds_config.toggle_radio_prio = input,
            Keybind::SayAgain => keybinds_config.say_again = input,
            _ => {}
        }

        keybind_engine
            .write()
            .await
            .set_config(
                &state.config.client.transmit_config,
                &keybinds_config,
                state.config.client.radio.integration.is_some(),
            )
            .await?;

        state.config.client.keybinds = keybinds_config;
        state.config.client.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(())
}

/// State for the "press a joystick button to bind" capture flow.
///
/// Holds the id and cancellation token of the capture currently in flight (if
/// any) so a new capture, or an explicit cancel from the UI, can abort it. A
/// completed capture leaves its (spent) token in place; cancelling it later is
/// a no-op.
#[derive(Debug, Default)]
pub struct JoystickCaptureState {
    active: parking_lot::Mutex<Option<(String, CancellationToken)>>,
}

/// Wait for the next joystick button press and return it, or `None` on timeout
/// or cancellation.
///
/// Uses the shared joystick service, so this works independently of the keybind
/// engine lifecycle - in particular before any joystick binding exists. Starting
/// a new capture cancels a previous one still in flight; `capture_id` correlates
/// a later [`keybinds_cancel_joystick_capture`] with this capture, so a stale
/// cancel cannot abort a newer capture.
#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_capture_joystick_button(
    app_state: State<'_, AppState>,
    capture_state: State<'_, JoystickCaptureState>,
    joystick: State<'_, JoystickServiceHandle>,
    capture_id: String,
    timeout_ms: Option<u64>,
) -> Result<Option<JoystickButton>, Error> {
    let capabilities = Capabilities::default();
    if !capabilities.joystick {
        return Err(Error::CapabilityNotAvailable("Joystick".to_string()));
    }

    let token = CancellationToken::new();
    if let Some((_, previous)) = capture_state
        .active
        .lock()
        .replace((capture_id, token.clone()))
    {
        previous.cancel();
    }

    let ignored: HashSet<String> = app_state
        .lock()
        .await
        .config
        .client
        .ignored_joysticks
        .iter()
        .map(|device| device.device.clone())
        .collect();

    let mut rx = joystick.subscribe().await?;
    let timeout = timeout_ms.map_or(DEFAULT_CAPTURE_TIMEOUT, Duration::from_millis);

    let next_button = async {
        while let Some(event) = rx.recv().await {
            if let Some(button) = capturable_button(event, &ignored) {
                return Some(button);
            }
        }
        None
    };

    let captured = tokio::select! {
        biased;
        _ = token.cancelled() => None,
        button = next_button => button,
        _ = tokio::time::sleep(timeout) => None,
    };

    Ok(captured)
}

/// The next capturable button press from an event, skipping releases and
/// presses originating from ignored devices (e.g. flight sim hardware with
/// latched switches that would instantly win every capture).
fn capturable_button(
    event: crate::keybinds::KeyEvent,
    ignored_devices: &HashSet<String>,
) -> Option<JoystickButton> {
    if event.state != KeyState::Down {
        return None;
    }
    let Trigger::Input(InputCode::Button(button)) = event.trigger else {
        return None;
    };
    (!ignored_devices.contains(&button.device)).then_some(button)
}

/// A joystick device together with its presence and capture-ignore state, as
/// shown in the ignore-list settings UI. Identity follows the device GUID,
/// matching [`JoystickDevice`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct JoystickDeviceEntry {
    #[serde(flatten)]
    pub device: JoystickDevice,
    pub connected: bool,
    pub ignored: bool,
}

impl PartialEq for JoystickDeviceEntry {
    fn eq(&self, other: &Self) -> bool {
        self.device == other.device
    }
}

impl Eq for JoystickDeviceEntry {}

impl std::hash::Hash for JoystickDeviceEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.device.hash(state);
    }
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_list_joystick_devices(
    app_state: State<'_, AppState>,
    joystick: State<'_, JoystickServiceHandle>,
) -> Result<HashSet<JoystickDeviceEntry>, Error> {
    let capabilities = Capabilities::default();
    if !capabilities.joystick {
        return Err(Error::CapabilityNotAvailable("Joystick".to_string()));
    }

    let connected = joystick.connected_devices().await?;
    let ignored = app_state
        .lock()
        .await
        .config
        .client
        .ignored_joysticks
        .clone();
    Ok(merge_device_entries(connected, ignored))
}

fn merge_device_entries(
    connected: HashSet<JoystickDevice>,
    ignored: HashSet<JoystickDevice>,
) -> HashSet<JoystickDeviceEntry> {
    let mut entries: HashSet<JoystickDeviceEntry> = connected
        .into_iter()
        .map(|device| JoystickDeviceEntry {
            ignored: ignored.contains(&device),
            connected: true,
            device,
        })
        .collect();

    for device in ignored {
        // No-op for GUIDs that already have a connected entry
        entries.insert(JoystickDeviceEntry {
            device,
            connected: false,
            ignored: true,
        });
    }

    entries
}

/// Replace the list of joystick devices excluded from binding capture.
///
/// Entries persist by device GUID, so they survive unplugging; existing
/// bindings on ignored devices keep working.
#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_set_ignored_joysticks(
    app: AppHandle,
    app_state: State<'_, AppState>,
    devices: HashSet<JoystickDevice>,
) -> Result<(), Error> {
    let capabilities = Capabilities::default();
    if !capabilities.joystick {
        return Err(Error::CapabilityNotAvailable("Joystick".to_string()));
    }

    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;
        state.config.client.ignored_joysticks = devices;
        state.config.client.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(())
}

/// Cancel the joystick button capture with the given id, if it is still the
/// active one (a newer capture supersedes it and stays unaffected).
#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_cancel_joystick_capture(
    capture_state: State<'_, JoystickCaptureState>,
    capture_id: String,
) -> Result<(), Error> {
    let mut active = capture_state.active.lock();
    if active.as_ref().is_some_and(|(id, _)| *id == capture_id)
        && let Some((_, token)) = active.take()
    {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_get_external_binding(
    keybind_engine: State<'_, KeybindEngineHandle>,
    keybind: Keybind,
) -> Result<Option<String>, Error> {
    let capabilities = Capabilities::default();
    if !capabilities.keybind_listener {
        return Err(Error::CapabilityNotAvailable("Keybinds".to_string()));
    }
    Ok(keybind_engine.read().await.get_external_binding(keybind))
}

#[tauri::command]
#[vacs_macros::log_err]
pub fn keybinds_open_system_shortcuts_settings() -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        use crate::platform::DesktopEnvironment;
        return DesktopEnvironment::get()
            .open_keyboard_shortcuts_settings()
            .map_err(|err| Error::Other(Box::new(anyhow::anyhow!(err))));
    }

    #[cfg(not(target_os = "linux"))]
    {
        return Err(Error::Other(Box::new(anyhow::anyhow!(
            "Opening keyboard shortcuts settings is only supported on Linux"
        ))));
    }
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_is_portal_shortcut_bound(
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] keybind_engine: State<
        '_,
        KeybindEngineHandle,
    >,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] keybind: Keybind,
) -> Result<bool, Error> {
    #[cfg(target_os = "linux")]
    {
        // Reads the listener's live view of the portal bindings rather than
        // opening a throwaway portal session per query: the extra sessions are
        // leak-prone, and on portal backends that only report shortcuts after
        // BindShortcuts they always came back empty.
        return Ok(keybind_engine
            .read()
            .await
            .get_external_binding(keybind)
            .is_some());
    }

    #[cfg(not(target_os = "linux"))]
    {
        return Err(Error::Other(Box::new(anyhow::anyhow!(
            "Checking portal shortcut bindings is only supported on Linux"
        ))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinds::KeyEvent;

    fn button_event(device: &str, state: KeyState) -> KeyEvent {
        KeyEvent::button(
            JoystickButton {
                device: device.to_string(),
                button: 0,
                name: None,
            },
            "test".to_string(),
            state,
        )
    }

    #[test]
    fn merge_device_entries_combines_connected_and_ignored() {
        let device = |guid: &str, name: Option<&str>| JoystickDevice {
            device: guid.to_string(),
            name: name.map(str::to_string),
        };

        let connected = HashSet::from([
            device("yoke", Some("Yoke")),
            device("throttle", Some("Throttle")),
        ]);
        let ignored = HashSet::from([
            // connected under a fresher name than the persisted one
            device("throttle", Some("Old Throttle Name")),
            // not connected right now, must still be listed
            device("pedals", Some("Pedals")),
        ]);

        let entries = merge_device_entries(connected, ignored);
        assert_eq!(entries.len(), 3);

        let by_guid = |guid: &str| entries.iter().find(|e| e.device.device == guid).unwrap();
        assert_eq!(
            (by_guid("yoke").connected, by_guid("yoke").ignored),
            (true, false)
        );
        let throttle = by_guid("throttle");
        assert_eq!((throttle.connected, throttle.ignored), (true, true));
        assert_eq!(throttle.device.name.as_deref(), Some("Throttle"));
        assert_eq!(
            (by_guid("pedals").connected, by_guid("pedals").ignored),
            (false, true)
        );
    }

    #[test]
    fn capturable_button_skips_ignored_devices() {
        let ignored: HashSet<String> = ["latched-throttle".to_string()].into();

        assert!(
            capturable_button(button_event("latched-throttle", KeyState::Down), &ignored).is_none()
        );
        assert_eq!(
            capturable_button(button_event("yoke", KeyState::Down), &ignored)
                .map(|button| button.device),
            Some("yoke".to_string())
        );
    }

    #[test]
    fn capturable_button_skips_releases_and_keys() {
        let ignored = HashSet::new();

        assert!(capturable_button(button_event("yoke", KeyState::Up), &ignored).is_none());
        assert!(
            capturable_button(
                KeyEvent::key(keyboard_types::Code::KeyA, "A".to_string(), KeyState::Down),
                &ignored
            )
            .is_none()
        );
    }
}
