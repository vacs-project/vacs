use crate::app::state::AppState;
use crate::app::state::http::HttpState;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::config::BackendEndpoint;
use crate::error::{Error, HandleUnauthorizedExt};
use anyhow::Context;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use vacs_signaling::protocol::http::auth::{InitVatsimLogin, UserInfo};

#[tauri::command]
#[vacs_macros::log_err]
pub async fn auth_open_oauth_url(http_state: State<'_, HttpState>) -> Result<(), Error> {
    let auth_url = http_state
        .http_get::<InitVatsimLogin>(BackendEndpoint::InitAuth, None)
        .await?
        .url;

    log::info!("Opening auth URL: {auth_url}");

    crate::external::open_url_detached(auth_url)
        .await
        .context("Failed to open auth URL with the default browser")?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn auth_check_session(
    app: AppHandle,
    http_state: State<'_, HttpState>,
) -> Result<(), Error> {
    log::debug!("Fetching user info");
    let response = http_state
        .http_get::<UserInfo>(BackendEndpoint::UserInfo, None)
        .await;

    match response {
        Ok(user_info) => {
            log::info!("Authenticated as CID {}", user_info.cid);

            app.state::<AppState>()
                .lock()
                .await
                .set_client_id(Some(user_info.cid.clone()));

            app.emit("auth:authenticated", user_info.cid).ok();
            Ok(())
        }
        Err(Error::Unauthorized) => {
            log::info!("Not authenticated");

            app.state::<AppState>().lock().await.set_client_id(None);

            app.emit("auth:unauthenticated", Value::Null).ok();
            Ok(())
        }
        Err(err) => {
            log::info!("Not authenticated");

            app.state::<AppState>().lock().await.set_client_id(None);

            app.emit("auth:unauthenticated", Value::Null).ok();
            Err(err)
        }
    }
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn auth_logout(
    app: AppHandle,
    app_state: State<'_, AppState>,
    http_state: State<'_, HttpState>,
) -> Result<(), Error> {
    log::debug!("Logging out");

    app_state.lock().await.disconnect_signaling(&app).await;

    http_state
        .http_post::<(), ()>(BackendEndpoint::Logout, None, None)
        .await
        .handle_unauthorized(&app)
        .await?;

    http_state
        .clear_cookie_store()
        .context("Failed to clear cookie store")?;

    app.state::<AppState>().lock().await.set_client_id(None);

    log::info!("Successfully logged out");

    app.emit("auth:unauthenticated", Value::Null).ok();

    Ok(())
}
