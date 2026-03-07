use crate::app::state::http::HttpState;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::webrtc::AppStateWebrtcExt;
use crate::app::state::{AppState, AppStateInner};
use crate::audio::manager::{AudioManagerHandle, SourceType};
use crate::config::{
    BackendEndpoint, CLIENT_SETTINGS_FILE_NAME, Persistable, PersistedClientConfig,
};
use crate::error::{Error, HandleUnauthorizedExt};
use std::collections::HashSet;
use tauri::{AppHandle, Manager, State};
use vacs_signaling::protocol::http::webrtc::IceConfig;
use vacs_signaling::protocol::vatsim::{ClientId, PositionId};
use vacs_signaling::protocol::ws::shared;
use vacs_signaling::protocol::ws::shared::{CallId, CallSource, CallTarget};

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_connect(
    app: AppHandle,
    app_state: State<'_, AppState>,
    http_state: State<'_, HttpState>,
    position_id: Option<PositionId>,
) -> Result<(), Error> {
    let mut app_state = app_state.lock().await;

    #[cfg(any(debug_assertions, feature = "rc"))]
    let position_id = position_id.or_else(|| app_state.config.backend.dev_position_id.clone());

    app_state.connect_signaling(&app, position_id).await?;

    if !app_state.config.ice.is_default() {
        log::info!("Modified ICE config detected, not fetching from server");
        return Ok(());
    }

    refresh_ice_config(&http_state, &mut app_state).await;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_disconnect(app: AppHandle) -> Result<(), Error> {
    app.state::<AppState>()
        .lock()
        .await
        .disconnect_signaling(&app)
        .await;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_terminate(
    app: AppHandle,
    http_state: State<'_, HttpState>,
) -> Result<(), Error> {
    log::debug!("Terminating signaling server session");

    http_state
        .http_delete::<()>(BackendEndpoint::TerminateWsSession, None)
        .await
        .handle_unauthorized(&app)
        .await?;

    log::info!("Successfully terminated signaling server session");

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_start_call(
    app: AppHandle,
    app_state: State<'_, AppState>,
    http_state: State<'_, HttpState>,
    audio_manager: State<'_, AudioManagerHandle>,
    target: CallTarget,
    source: CallSource,
    prio: bool,
) -> Result<CallId, Error> {
    log::debug!("Starting call with {target:?} as {source:?}");

    let mut state = app_state.lock().await;

    let call_id = CallId::new();
    let invite = shared::CallInvite {
        call_id,
        target,
        source,
        prio,
    };
    state.send_signaling_message(invite.clone()).await?;

    if state.is_ice_config_expired() {
        refresh_ice_config(&http_state, &mut state).await;
    }

    state.start_unanswered_call_timer(&app, &call_id);
    state.set_outgoing_call(Some(invite));

    audio_manager.read().restart(SourceType::Ringback);

    Ok(call_id)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_accept_call(
    app: AppHandle,
    app_state: State<'_, AppState>,
    call_id: CallId,
) -> Result<(), Error> {
    log::debug!("Accepting call {call_id:?}");

    let mut state = app_state.lock().await;
    state.accept_call(&app, Some(call_id)).await?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_end_call(
    app: AppHandle,
    app_state: State<'_, AppState>,
    call_id: CallId,
) -> Result<(), Error> {
    log::debug!("Ending call {call_id:?}");

    let mut state = app_state.lock().await;
    state.end_call(&app, Some(call_id)).await?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_get_ignored_clients(
    app_state: State<'_, AppState>,
) -> Result<HashSet<ClientId>, Error> {
    let state = app_state.lock().await;

    Ok(state.config.client.ignored.clone())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_add_ignored_client(
    app: AppHandle,
    app_state: State<'_, AppState>,
    client_id: ClientId,
) -> Result<bool, Error> {
    let (persisted_stations_config, added): (PersistedClientConfig, bool) = {
        let mut state = app_state.lock().await;
        let added = state.config.client.ignored.insert(client_id);
        (state.config.client.clone().into(), added)
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_stations_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(added)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn signaling_remove_ignored_client(
    app: AppHandle,
    app_state: State<'_, AppState>,
    client_id: ClientId,
) -> Result<bool, Error> {
    let (persisted_stations_config, removed): (PersistedClientConfig, bool) = {
        let mut state = app_state.lock().await;
        let removed = state.config.client.ignored.remove(&client_id);
        (state.config.client.clone().into(), removed)
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_stations_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(removed)
}

async fn refresh_ice_config(http_state: &HttpState, app_state: &mut AppStateInner) {
    let config = match http_state
        .http_get::<IceConfig>(BackendEndpoint::IceConfig, None)
        .await
    {
        Ok(config) => config,
        Err(err) => {
            log::warn!("Failed to fetch ICE config, falling back to default: {err:?}");
            return;
        }
    };

    log::info!(
        "Received ICE config from server, expires at {}",
        config.expires_at.unwrap_or_default()
    );
    app_state.set_ice_config(config);
}
