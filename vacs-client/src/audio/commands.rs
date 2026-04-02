use crate::app::state::AppState;
use crate::app::state::webrtc::AppStateWebrtcExt;
use crate::audio::manager::{AudioManagerHandle, SourceType};
use crate::audio::{AudioDevices, AudioHosts, AudioVolumes, ClientAudioDeviceType, VolumeType};
use crate::config::{AUDIO_SETTINGS_FILE_NAME, AudioConfig, Persistable, PersistedAudioConfig};
use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use vacs_audio::device::{DeviceSelector, DeviceType};
use vacs_audio::error::AudioError;

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_get_hosts(app_state: State<'_, AppState>) -> Result<AudioHosts, Error> {
    log::debug!("Getting audio hosts");

    let mut selected = app_state
        .lock()
        .await
        .config
        .audio
        .host_name
        .clone()
        .unwrap_or_default();
    if selected.is_empty() {
        selected = DeviceSelector::default_host_name();
    }

    let hosts = DeviceSelector::all_host_names();

    Ok(AudioHosts {
        selected,
        all: hosts,
    })
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_set_host(
    app: AppHandle,
    app_state: State<'_, AppState>,
    audio_manager: State<'_, AudioManagerHandle>,
    host_name: String,
) -> Result<(), Error> {
    let mut state = app_state.lock().await;

    if state.active_call_id().is_some() {
        return Err(AudioError::Other(anyhow::anyhow!(
            "Cannot set audio host while call is active"
        ))
        .into());
    }

    log::info!("Setting audio host (name: {host_name})");

    let persisted_audio_config: PersistedAudioConfig = {
        let mut audio_config = state.config.audio.clone();
        audio_config.host_name = Some(host_name).filter(|x| !x.is_empty());
        // Device IDs are host-scoped, so clear them when switching hosts.
        audio_config.input_device_id = None;
        audio_config.output_device_id = None;

        audio_manager
            .write()
            .switch_output_device(app.clone(), &audio_config, false)?;

        state.config.audio = audio_config;
        state.config.audio.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_audio_config.persist(&config_dir, AUDIO_SETTINGS_FILE_NAME)?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_get_devices(
    app_state: State<'_, AppState>,
    audio_manager: State<'_, AudioManagerHandle>,
    device_type: ClientAudioDeviceType,
) -> Result<AudioDevices, Error> {
    log::debug!("Getting audio devices (type: {:?})", device_type);

    let state = app_state.lock().await;
    get_audio_devices(
        device_type,
        &state.config.audio,
        audio_manager.read().output_device_name(),
        audio_manager.read().speaker_device_name(),
    )
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_set_device(
    app: AppHandle,
    app_state: State<'_, AppState>,
    audio_manager: State<'_, AudioManagerHandle>,
    device_type: ClientAudioDeviceType,
    device_name: Option<String>,
) -> Result<AudioDevices, Error> {
    let mut state = app_state.lock().await;
    let mut audio_manager = audio_manager.write();

    if state.active_call_id().is_some() {
        return Err(AudioError::Other(anyhow::anyhow!(
            "Cannot set audio device while call is active"
        ))
        .into());
    }

    let reattach_input_level_meter = if audio_manager.is_input_device_attached()
        && matches!(device_type, ClientAudioDeviceType::Input)
    {
        log::trace!("Detaching input level meter before switching input device");
        audio_manager.detach_input_device();
        true
    } else {
        false
    };

    log::info!(
        "Setting audio device (name: {:?}, type: {:?})",
        device_name,
        device_type
    );

    let speaker_enabled = device_type == ClientAudioDeviceType::Speaker && device_name.is_some();

    let device_name = device_name.filter(|x| !x.is_empty());
    // Resolve the stable device ID for the selected device name so we can
    // persist it alongside the display name. On next startup, the ID is tried
    // first for reliable matching; the name serves as a fallback for old configs.
    let device_id = device_name.as_deref().and_then(|name| {
        DeviceSelector::resolve_device_id(
            device_type.into(),
            state.config.audio.host_name.as_deref(),
            name,
        )
    });

    let (persisted_audio_config, audio_devices): (PersistedAudioConfig, AudioDevices) = {
        match device_type {
            ClientAudioDeviceType::Input => {
                state.config.audio.input_device_name = device_name;
                state.config.audio.input_device_id = device_id;
            }
            ClientAudioDeviceType::Output => {
                let mut audio_config = state.config.audio.clone();
                audio_config.output_device_name = device_name;
                audio_config.output_device_id = device_id;

                audio_manager.switch_output_device(app.clone(), &audio_config, false)?;

                state.config.audio = audio_config;
            }
            ClientAudioDeviceType::Speaker => {
                let mut audio_config = state.config.audio.clone();
                audio_config.speaker_enabled = speaker_enabled;
                audio_config.speaker_device_name = device_name;
                audio_config.speaker_device_id = device_id;

                audio_manager.switch_speaker_device(app.clone(), &audio_config, false)?;

                state.config.audio = audio_config;
            }
        }

        let audio_devices = get_audio_devices(
            device_type,
            &state.config.audio,
            audio_manager.output_device_name(),
            audio_manager.speaker_device_name(),
        )?;

        if reattach_input_level_meter {
            log::trace!("Re-attaching input level meter after switching input device");
            let app = app.clone();
            audio_manager.attach_input_level_meter(
                app.clone(),
                &state.config.audio,
                Box::new(move |level| {
                    app.emit("audio:input-level", level).ok();
                }),
            )?;
        }

        (state.config.audio.clone().into(), audio_devices)
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_audio_config.persist(&config_dir, AUDIO_SETTINGS_FILE_NAME)?;

    Ok(audio_devices)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_get_volumes(app_state: State<'_, AppState>) -> Result<AudioVolumes, Error> {
    log::debug!("Getting audio volumes");

    let state = app_state.lock().await;
    let audio_config = &state.config.audio;

    Ok(AudioVolumes {
        input: audio_config.input_device_volume,
        output: audio_config.output_device_volume,
        click: audio_config.click_volume,
        chime: audio_config.chime_volume,
    })
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_set_volume(
    app: AppHandle,
    app_state: State<'_, AppState>,
    audio_manager: State<'_, AudioManagerHandle>,
    volume_type: VolumeType,
    volume: f32,
) -> Result<(), Error> {
    log::trace!(
        "Setting audio volume (type: {:?}, volume: {:?})",
        volume_type,
        volume
    );
    let mut state = app_state.lock().await;
    let audio_manager = audio_manager.read();

    match volume_type {
        VolumeType::Input => {
            audio_manager.set_input_volume(volume);
            state.config.audio.input_device_volume = volume;
        }
        VolumeType::Output => {
            audio_manager.set_output_volume(SourceType::Opus, volume);
            audio_manager.set_output_volume(SourceType::Ringback, volume);
            audio_manager.set_output_volume(SourceType::RingbackOneshot, volume);
            audio_manager.set_output_volume(SourceType::CallStart, volume);
            audio_manager.set_output_volume(SourceType::CallEnd, volume);
            state.config.audio.output_device_volume = volume;
        }
        VolumeType::Click => {
            audio_manager.set_output_volume(SourceType::Click, volume);
            state.config.audio.click_volume = volume;
        }
        VolumeType::Chime => {
            audio_manager.set_output_volume(SourceType::Ring, volume);
            audio_manager.set_output_volume(SourceType::PriorityRing, volume);
            state.config.audio.chime_volume = volume;
        }
    }

    let persisted_audio_config: PersistedAudioConfig = state.config.audio.clone().into();

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_audio_config.persist(&config_dir, AUDIO_SETTINGS_FILE_NAME)?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_play_ui_click(
    audio_manager: State<'_, AudioManagerHandle>,
) -> Result<(), Error> {
    if let Some(audio_manager) = audio_manager.try_read_for(Duration::from_millis(500)) {
        audio_manager.start(SourceType::Click);
    } else {
        log::warn!("Play UI click state lock acquire timed out");
    }

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_start_input_level_meter(
    app_state: State<'_, AppState>,
    audio_manager: State<'_, AudioManagerHandle>,
    app: AppHandle,
) -> Result<(), Error> {
    log::trace!("Starting input level meter");

    let state = app_state.lock().await;
    let audio_config = &state.config.audio.clone();
    let mut audio_manager = audio_manager.write();

    if audio_manager.is_input_device_attached() {
        if audio_manager.is_input_level_meter_attached() {
            return Err(AudioError::Other(anyhow::anyhow!(
                "Cannot start input level meter while already active"
            ))
            .into());
        }

        // As this command is called when the user opens the settings page,
        // we don't want to show an error message if the user is in a call.
        return Ok(());
    }

    audio_manager.attach_input_level_meter(
        app.clone(),
        audio_config,
        Box::new(move |level| {
            app.emit("audio:input-level", level).ok();
        }),
    )?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_stop_input_level_meter(
    audio_manager: State<'_, AudioManagerHandle>,
) -> Result<(), Error> {
    log::trace!("Stopping input level meter");

    if audio_manager.read().is_input_level_meter_attached() {
        audio_manager.write().detach_input_device();
    }

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn audio_set_radio_prio(
    app: AppHandle,
    keybind_engine: State<'_, KeybindEngineHandle>,
    prio: bool,
) -> Result<(), Error> {
    keybind_engine.read().await.set_radio_prio(prio);
    app.emit("audio:radio-prio", prio).ok();
    Ok(())
}

fn get_audio_devices(
    device_type: ClientAudioDeviceType,
    audio_config: &AudioConfig,
    picked_output_device: String,
    picked_speaker_device: Option<String>,
) -> Result<AudioDevices, Error> {
    let host = audio_config.host_name.clone();
    let host = host.as_deref();
    let (preferred, picked) = match device_type {
        ClientAudioDeviceType::Input => {
            let preferred = audio_config.input_device_name.clone().unwrap_or_default();
            let picked = DeviceSelector::picked_device_name(
                DeviceType::Input,
                host,
                audio_config.input_device_id.as_deref(),
                Some(&preferred),
            )?;
            (Some(preferred), Some(picked))
        }
        ClientAudioDeviceType::Output => {
            let preferred = audio_config.output_device_name.clone().unwrap_or_default();
            (Some(preferred), Some(picked_output_device))
        }
        ClientAudioDeviceType::Speaker => {
            if audio_config.speaker_enabled {
                let preferred = audio_config.speaker_device_name.clone().unwrap_or_default();
                (
                    Some(preferred),
                    Some(picked_speaker_device.unwrap_or_default()),
                )
            } else {
                (None, None)
            }
        }
    };

    let default = DeviceSelector::default_device_name(device_type.into(), host)?;
    let devices: Vec<String> = DeviceSelector::all_device_names(device_type.into(), host)?;

    Ok(AudioDevices {
        preferred,
        picked,
        default,
        all: devices,
    })
}
