use crate::error::Error;
use crate::keybinds::engine::KeybindEngineHandle;
use crate::radio::{DynRadio, Frequency, RadioStation, StationStateUpdate};
use tauri::State;

async fn radio(engine: &KeybindEngineHandle) -> Result<DynRadio, Error> {
    engine
        .read()
        .await
        .radio()
        .ok_or_else(|| crate::radio::RadioError::Integration("No radio configured".into()).into())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_add_station(
    keybind_engine: State<'_, KeybindEngineHandle>,
    callsign: String,
) -> Result<RadioStation, Error> {
    let radio = radio(&keybind_engine).await?;
    Ok(radio.add_station(&callsign).await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_set_station_state(
    keybind_engine: State<'_, KeybindEngineHandle>,
    frequency: Frequency,
    update: StationStateUpdate,
) -> Result<RadioStation, Error> {
    let radio = radio(&keybind_engine).await?;
    Ok(radio.set_station_state(frequency, update).await?)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn radio_get_stations(
    keybind_engine: State<'_, KeybindEngineHandle>,
) -> Result<Vec<RadioStation>, Error> {
    let radio = radio(&keybind_engine).await?;
    Ok(radio.get_stations().await?)
}
