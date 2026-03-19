use crate::app::state::AppState;
use crate::config::{CLIENT_SETTINGS_FILE_NAME, Persistable, PersistedClientConfig};
use crate::error::Error;
use crate::remote::server::RemoteServerHandle;
use crate::remote::{FrontendRemoteConfig, RemoteStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendRemoteConfigWithStatus {
    #[serde(flatten)]
    pub config: FrontendRemoteConfig,
    #[serde(flatten)]
    pub status: RemoteStatus,
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn remote_get_config(
    app_state: State<'_, AppState>,
    remote_server: State<'_, RemoteServerHandle>,
) -> Result<FrontendRemoteConfigWithStatus, Error> {
    Ok(FrontendRemoteConfigWithStatus {
        config: app_state.lock().await.config.client.remote.clone().into(),
        status: remote_server.lock().await.status(),
    })
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn remote_set_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
    remote_server: State<'_, RemoteServerHandle>,
    remote_config: FrontendRemoteConfig,
) -> Result<(), Error> {
    let (persisted_client_config, changed) = {
        let mut state = app_state.lock().await;

        let remote = &state.config.client.remote;
        let changed = remote.listen_addr != remote_config.listen_addr
            || remote.enabled != remote_config.enabled;

        state.config.client.remote = remote_config.into();
        (
            PersistedClientConfig::from(state.config.client.clone()),
            changed,
        )
    };

    let remote = &persisted_client_config.client.remote;
    match (remote.enabled, changed) {
        (true, true) => remote_server
            .lock()
            .await
            .restart(remote.listen_addr, remote.serve_frontend),
        (false, _) => remote_server.lock().await.stop(),
        _ => {}
    }

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(())
}
