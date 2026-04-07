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

    tauri_plugin_opener::open_url(auth_url, None::<&str>)
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

/// Programmatically completes the full OAuth login flow without opening a
/// browser or requiring deep-link support.
///
/// This allows E2E tests running in headless environments to authenticate
/// as a specific user by CID. The command:
/// 1. Initiates the OAuth flow via signaling server
/// 2. Follows the mock OAuth authorize URL (with `login_hint`) to get the callback redirect
/// 3. Exchanges the code at signaling server using the same session
#[cfg(feature = "e2e")]
#[tauri::command]
#[vacs_macros::log_err]
pub async fn auth_login_test(
    app: AppHandle,
    http_state: State<'_, HttpState>,
    cid: String,
) -> Result<(), Error> {
    use url::Url;

    let init = http_state
        .http_get::<InitVatsimLogin>(BackendEndpoint::InitAuth, None)
        .await?;

    // Append login_hint so the mock selects the correct test user.
    let mut auth_url =
        Url::parse(&init.url).context("Failed to parse auth URL from init response")?;
    auth_url.query_pairs_mut().append_pair("login_hint", &cid);

    let oauth_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Failed to build OAuth HTTP client")?;

    let oauth_resp = oauth_client
        .get(auth_url.as_str())
        .send()
        .await
        .context("Failed to reach mock OAuth authorize endpoint")?;

    let redirect_url = oauth_resp
        .headers()
        .get(reqwest::header::LOCATION)
        .context("Mock OAuth did not return a redirect")?
        .to_str()
        .context("Redirect URL is not valid UTF-8")?
        .to_owned();

    crate::auth::handle_auth_callback(&app, &redirect_url).await
}
