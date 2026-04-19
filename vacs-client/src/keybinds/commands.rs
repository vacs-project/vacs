use crate::app::state::AppState;
use crate::config::{CLIENT_SETTINGS_FILE_NAME, FrontendKeybindsConfig, FrontendRadioConfig, FrontendTransmitConfig, KeybindsConfig, Persistable, PersistedClientConfig, RadioConfig, TransmitConfig, TransmitMode, InputCode};
use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use crate::keybinds::{Keybind, KeybindsError};
use crate::platform::Capabilities;
use crate::radio::{RadioIntegration, RadioState};
use keyboard_types::Code;
use tauri::{AppHandle, Manager, State};

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
    let capabilities = Capabilities::default();
    if !capabilities.keybind_listener {
        return Err(Error::CapabilityNotAvailable("Keybinds".to_string()));
    }

    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        let transmit_config: TransmitConfig = transmit_config.try_into()?;

        validate_afv_radio_integration_config(&transmit_config, &state.config.client.radio)?;

        keybind_engine
            .write()
            .await
            .set_config(&transmit_config, &state.config.client.keybinds)
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
    code: Option<String>,
    keybind: Keybind,
) -> Result<(), Error> {
    let capabilities = Capabilities::default();
    if !capabilities.keybind_listener {
        return Err(Error::CapabilityNotAvailable("Keybinds".to_string()));
    }

    let code = code
        .as_ref()
        .map(|s| s.parse::<Code>())
        .transpose()
        .map_err(|_| Error::Other(Box::new(anyhow::anyhow!("Unrecognized key code: {}. Please report this error in our GitHub repository's issue tracker.", code.unwrap_or_default()))))?;

    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        let mut keybinds_config: KeybindsConfig = state.config.client.keybinds.clone();

        match keybind {
            Keybind::AcceptCall => keybinds_config.accept_call = code,
            Keybind::EndCall => keybinds_config.end_call = code,
            Keybind::ToggleRadioPrio => keybinds_config.toggle_radio_prio = code,
            _ => {}
        }

        keybind_engine
            .write()
            .await
            .set_config(&state.config.client.transmit_config, &keybinds_config)
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

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_get_radio_config(
    app_state: State<'_, AppState>,
) -> Result<FrontendRadioConfig, Error> {
    Ok(app_state.lock().await.config.client.radio.clone().into())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_set_radio_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
    keybind_engine: State<'_, KeybindEngineHandle>,
    radio_config: FrontendRadioConfig,
) -> Result<(), Error> {
    let capabilities = Capabilities::default();
    if !capabilities.keybind_listener {
        return Err(Error::CapabilityNotAvailable("Keybinds".to_string()));
    }

    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        let radio_config: RadioConfig = radio_config.try_into()?;

        validate_afv_radio_integration_config(&state.config.client.transmit_config, &radio_config)?;

        keybind_engine
            .write()
            .await
            .set_radio_config(&radio_config)
            .await?;

        state.config.client.radio = radio_config;
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
pub async fn keybinds_get_radio_state(
    keybind_engine: State<'_, KeybindEngineHandle>,
) -> Result<RadioState, Error> {
    let capabilities = Capabilities::default();
    if !capabilities.keybind_listener {
        return Ok(RadioState::NotConfigured);
    }

    Ok(keybind_engine.read().await.radio_state())
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
pub async fn keybinds_reconnect_radio(
    keybind_engine: State<'_, KeybindEngineHandle>,
) -> Result<(), Error> {
    keybind_engine.read().await.reconnect_radio().await
}

fn validate_afv_radio_integration_config(
    transmit_config: &TransmitConfig,
    radio_config: &RadioConfig,
) -> Result<(), Error> {
    if transmit_config.mode == TransmitMode::RadioIntegration
        && radio_config.integration == RadioIntegration::AudioForVatsim
        && let Some(selected_key) = transmit_config.radio_push_to_talk
        && let Some(afv_key) = radio_config.audio_for_vatsim.as_ref().and_then(|c| c.emit)
        && selected_key == InputCode::Key(afv_key)
    {
        return Err(KeybindsError::Other(
            "AFV emit key must be distinct from your radio integration push-to-talk key"
                .to_string(),
        )
        .into());
    }
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn keybinds_capture_joystick_button() -> Result<Option<String>, Error> {
    let result = tokio::task::spawn_blocking(|| {
        let mut gilrs = gilrs::Gilrs::new().map_err(|e| {
            Error::Other(Box::new(anyhow::anyhow!("gilrs init failed: {e}")))
        })?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

        loop {
            if std::time::Instant::now() > deadline {
                return Ok::<Option<String>, Error>(None);
            }

            while let Some(gilrs::Event { event, .. }) = gilrs.next_event() {
                let button = match event {
                    gilrs::EventType::ButtonPressed(b, _) => b,
                    gilrs::EventType::ButtonChanged(b, v, _) if v >= 0.5 => b,
                    _ => continue,
                };

                let idx: u8 = match button {
                    gilrs::Button::South => 0,
                    gilrs::Button::East => 1,
                    gilrs::Button::North => 2,
                    gilrs::Button::West => 3,
                    gilrs::Button::C => 4,
                    gilrs::Button::Z => 5,
                    gilrs::Button::LeftTrigger => 6,
                    gilrs::Button::LeftTrigger2 => 7,
                    gilrs::Button::RightTrigger => 8,
                    gilrs::Button::RightTrigger2 => 9,
                    gilrs::Button::Select => 10,
                    gilrs::Button::Start => 11,
                    gilrs::Button::Mode => 12,
                    gilrs::Button::LeftThumb => 13,
                    gilrs::Button::DPadUp => 14,
                    gilrs::Button::DPadDown => 15,
                    gilrs::Button::DPadLeft => 16,
                    gilrs::Button::DPadRight => 17,
                    _ => continue,
                };
                
                return Ok(Some(format!("Joystick:{idx}")));
            }

            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    })
        .await
        .map_err(|e| Error::Other(Box::new(anyhow::anyhow!("join error: {e}"))))??;

    Ok(result)
}

