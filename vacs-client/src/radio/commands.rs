use crate::app::PersistedClientConfig;
use crate::app::state::AppState;
use crate::audio::manager::AudioManagerHandle;
use crate::config::{CLIENT_SETTINGS_FILE_NAME, Persistable};
use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use crate::platform::Capabilities;
use crate::playback::commands::stop_playing_source;
use crate::playback::recorder::PlaybackRecorderHandle;
use crate::radio::{
    DynRadio, Frequency, FrontendRadioConfig, RadioConfig, RadioHandle, RadioState, RadioStation,
    StationStateUpdate,
};
use tauri::{AppHandle, Manager, State};

fn radio(radio: &RadioHandle) -> Result<DynRadio, Error> {
    let guard = radio.read();
    Ok(guard
        .as_ref()
        .ok_or_else(|| crate::radio::RadioError::Integration("No radio configured".into()))?
        .clone())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_get_config(
    app_state: State<'_, AppState>,
) -> Result<FrontendRadioConfig, Error> {
    Ok(app_state.lock().await.config.client.radio.clone().into())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_set_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
    keybind_engine: State<'_, KeybindEngineHandle>,
    radio_handle: State<'_, RadioHandle>,
    playback_recorder: State<'_, PlaybackRecorderHandle>,
    radio_config: FrontendRadioConfig,
) -> Result<(), Error> {
    let capabilities = Capabilities::default();
    if !capabilities.keybind_listener {
        return Err(Error::CapabilityNotAvailable("Keybinds".to_string()));
    }

    let radio_config: RadioConfig = {
        let state = app_state.lock().await;
        let radio_config: RadioConfig = radio_config.try_into()?;
        radio_config.validate(&state.config.client.transmit_config)?;
        radio_config
    };

    radio_handle.write().take();

    // The recorder outlives the radio it was gating on: left running, it would keep capturing
    // with the dead radio's stale event stream - forever, if the new integration is one
    // `make_source` doesn't support and thus never re-creates it. `shutdown` cancels the
    // recorder's background task and awaits its exit, including the underlying source's
    // capture teardown.
    stop_playing_source(&playback_recorder, &app.state::<AudioManagerHandle>());
    let old_recorder = playback_recorder.write().take();
    if let Some(recorder) = old_recorder {
        recorder.shutdown().await;
    }

    let new_radio = radio_config.radio(app.clone()).await?;

    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;
        let radio_integration_enabled = radio_config.integration.is_some();
        if radio_integration_enabled != state.config.client.radio.integration.is_some() {
            keybind_engine
                .write()
                .await
                .set_config(
                    &state.config.client.transmit_config,
                    &state.config.client.keybinds,
                    radio_integration_enabled,
                )
                .await?;
        }
        state.config.client.radio = radio_config;
        state.config.client.clone().into()
    };
    *radio_handle.write() = new_radio;

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_add_station(
    radio_handle: State<'_, RadioHandle>,
    callsign: String,
) -> Result<RadioStation, Error> {
    Ok(radio(&radio_handle)?.add_station(&callsign).await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_set_station_state(
    radio_handle: State<'_, RadioHandle>,
    frequency: Frequency,
    update: StationStateUpdate,
) -> Result<RadioStation, Error> {
    Ok(radio(&radio_handle)?
        .set_station_state(frequency, update)
        .await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_get_stations(
    radio_handle: State<'_, RadioHandle>,
) -> Result<Vec<RadioStation>, Error> {
    Ok(radio(&radio_handle)?.get_stations().await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_fast_couple(radio_handle: State<'_, RadioHandle>) -> Result<(), Error> {
    Ok(radio(&radio_handle)?.fast_couple().await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_reconnect(radio_handle: State<'_, RadioHandle>) -> Result<(), Error> {
    Ok(radio(&radio_handle)?.reconnect().await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_get_state(radio_handle: State<'_, RadioHandle>) -> Result<RadioState, Error> {
    Ok(radio_handle
        .read()
        .as_ref()
        .map_or(RadioState::NotConfigured, |radio| radio.state()))
}
