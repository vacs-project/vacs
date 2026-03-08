use crate::app::state::AppState;
use crate::error::Error;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
#[vacs_macros::log_err]
pub async fn remote_is_enabled(app_state: State<'_, AppState>) -> Result<bool, Error> {
    Ok(app_state.lock().await.config.client.remote.enabled)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn remote_broadcast_store_sync(
    app: AppHandle,
    store: String,
    state: serde_json::Value,
) -> Result<(), Error> {
    app.emit(
        "store:sync",
        serde_json::json!({"store": store, "state": state}),
    )
    .ok();
    Ok(())
}
