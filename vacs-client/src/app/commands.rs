use crate::app::state::AppState;
use crate::app::{AppFolder, UpdateInfo, get_update, open_app_folder, open_fatal_error_dialog};
use crate::audio::manager::{AudioManagerHandle, SourceType};
use crate::build::VersionInfo;
use crate::config::{
    AppConfig, CLIENT_SETTINGS_FILE_NAME, ClientConfig, FrontendCallConfig,
    FrontendClientPageSettings, Persistable, PersistedClientConfig,
};
use crate::error::{Error, FrontendError};
use crate::platform::Capabilities;
use anyhow::Context;
use notify_debouncer_full::notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use vacs_vatsim::coverage::profile::Profile;

#[tauri::command]
pub async fn app_frontend_ready(
    app: AppHandle,
    app_state: State<'_, AppState>,
    window: WebviewWindow,
) -> Result<(), Error> {
    log::info!("Frontend ready");
    let capabilities = Capabilities::default();

    #[cfg(target_os = "linux")]
    window.eval("document.body.classList.add('linux')").ok();

    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        crate::platform::windows_touch_handler::install(hwnd.0 as usize);
    } else {
        log::error!("Failed to get HWND for Input Shield");
    }

    let state = app_state.lock().await;
    if let Err(err) = state.config.client.restore_window_state(&app) {
        log::warn!("Failed to restore saved window state: {err}");
    }

    if state.config.client.always_on_top {
        if capabilities.always_on_top {
            if let Err(err) = window.set_always_on_top(true) {
                log::warn!("Failed to set main window to be always on top: {err}");
            } else {
                log::debug!("Set main window to be always on top");
            }
        } else {
            log::warn!(
                "Your platform ({}) does not support always on top windows, setting is ignored.",
                capabilities.platform
            );
        }
    }

    if state.config.client.fullscreen {
        if let Err(err) = window.set_fullscreen(true) {
            log::warn!("Failed to set main window to be fullscreen: {err}");
        } else {
            log::debug!("Set main window to be fullscreen");
        }
    }

    if let Err(err) = window.show() {
        log::error!("Failed to show window: {err}");

        open_fatal_error_dialog(
            &app,
            "Failed to show main window. Check your logs for further details.",
        );

        app.exit(1);
    };

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub fn app_open_folder(app: AppHandle, folder: AppFolder) -> Result<(), Error> {
    open_app_folder(&app, folder).context("Failed to open folder")?;
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_check_for_update(app: AppHandle) -> Result<UpdateInfo, Error> {
    let current_version = VersionInfo::gather().version.to_string();

    if cfg!(debug_assertions) {
        log::info!("Debug build, skipping update check");
        return Ok(UpdateInfo {
            current_version,
            new_version: None,
            required: false,
        });
    }

    let update_info = if let Some(update) = get_update(&app).await? {
        let required = update
            .raw_json
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        log::info!("Update available. Required: {required}");

        UpdateInfo {
            current_version,
            new_version: Some(update.version),
            required,
        }
    } else {
        log::info!("No update available");
        UpdateInfo {
            current_version,
            new_version: None,
            required: false,
        }
    };

    Ok(update_info)
}

#[tauri::command]
pub fn app_quit(app: AppHandle, window: WebviewWindow) {
    log::info!("Quitting");
    if let Err(err) = window.close() {
        log::error!("Failed to close window: {err}");
        app.exit(1);
    }
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_update(app: AppHandle) -> Result<(), Error> {
    if cfg!(debug_assertions) {
        log::info!("Debug build, skipping update");
        return Ok(());
    }

    if let Some(update) = get_update(&app).await? {
        log::info!(
            "Downloading and installing update. Version: {}",
            &update.version
        );
        let mut downloaded = 0;
        update
            .download_and_install(
                |chunk_length, content_length| {
                    downloaded += chunk_length;
                    if let Some(content_length) = content_length {
                        let progress = (downloaded / (content_length as usize)) * 100;
                        app.emit("update:progress", progress.clamp(0, 100)).ok();
                    }
                },
                || {
                    log::debug!("Download finished");
                },
            )
            .await
            .context("Failed to download and install the update")?;

        log::info!("Update installed. Restarting...");
        app.restart();
    } else {
        log::warn!("Tried to update without an update being available");
    }

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_platform_capabilities() -> Result<Capabilities, Error> {
    Ok(Capabilities::default())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_set_always_on_top(
    window: WebviewWindow,
    app: AppHandle,
    app_state: State<'_, AppState>,
    always_on_top: bool,
) -> Result<bool, Error> {
    let capabilities = Capabilities::default();
    if !capabilities.always_on_top {
        return Err(Error::CapabilityNotAvailable("Always on top".to_string()));
    }

    let persisted_client_config: PersistedClientConfig = {
        window
            .set_always_on_top(always_on_top)
            .context("Failed to change window always on top behaviour")?;

        let mut state = app_state.lock().await;
        state.config.client.always_on_top = always_on_top;
        state.config.client.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(persisted_client_config.client.always_on_top)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_set_fullscreen(
    window: WebviewWindow,
    app: AppHandle,
    app_state: State<'_, AppState>,
    fullscreen: bool,
) -> Result<bool, Error> {
    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        state.config.client.fullscreen = fullscreen;

        if fullscreen {
            state
                .config
                .client
                .update_window_state(&app)
                .context("Failed to update window state")?;
            window
                .set_fullscreen(true)
                .context("Failed to enable fullscreen")?;
        } else {
            window
                .set_fullscreen(false)
                .context("Failed to disable fullscreen")?;
            state
                .config
                .client
                .restore_window_state(&app)
                .context("Failed to restore window state")?;
        }

        state.config.client.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(persisted_client_config.client.fullscreen)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_reset_window_size(
    app: AppHandle,
    app_state: State<'_, AppState>,
    window: WebviewWindow,
) -> Result<(), Error> {
    log::debug!("Resetting window size");
    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        if state.config.client.fullscreen {
            state.config.client.fullscreen = false;
            window
                .set_fullscreen(false)
                .context("Failed to disable fullscreen")?;

            // Give window manager some time to update window size after disabling fullscreen to
            // avoid slight shrinking due to the way decorations apply (mainly under Wayland/KDE Plasma).
            #[cfg(target_os = "linux")]
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        window
            .set_size(ClientConfig::default_window_size(&window)?)
            .context("Failed to reset window size")?;

        window
            .set_zoom(1.0)
            .context("Failed to reset window zoom")?;

        #[cfg(target_os = "linux")]
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        state
            .config
            .client
            .update_window_state(&app)
            .context("Failed to update window state")?;

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
pub async fn app_get_call_config(
    app_state: State<'_, AppState>,
) -> Result<FrontendCallConfig, Error> {
    Ok(app_state.lock().await.config.client.call.clone().into())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_set_call_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
    audio_manager: State<'_, AudioManagerHandle>,
    call_config: FrontendCallConfig,
) -> Result<(), Error> {
    let persisted_client_config: PersistedClientConfig = {
        let mut state = app_state.lock().await;

        if call_config.enable_call_start_sound && !state.config.client.call.enable_call_start_sound
        {
            audio_manager.read().restart(SourceType::CallStart);
        } else if call_config.enable_call_end_sound
            && !state.config.client.call.enable_call_end_sound
        {
            audio_manager.read().restart(SourceType::CallEnd);
        }

        state.config.client.call = call_config.into();
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
pub async fn app_load_test_profile(
    app: AppHandle,
    app_state: State<'_, AppState>,
    path: Option<String>,
) -> Result<Option<PathBuf>, Error> {
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => {
            match rfd::AsyncFileDialog::new()
                .set_title("Select a custom profile file")
                .add_filter("vacs Profile", &["toml", "json"])
                .pick_file()
                .await
                .map(|p| p.path().to_path_buf())
            {
                Some(path) => path,
                None => {
                    let state = app.state::<AppState>();
                    let mut state = state.lock().await;
                    if state.test_profile_watcher.take().is_some() {
                        log::debug!("Stopped watching test profile");
                    }
                    return Ok(None);
                }
            }
        }
    };

    match Profile::load(&path) {
        Ok(profile) => {
            log::debug!("Loaded test profile: {:?}", profile);
            let profile = vacs_signaling::protocol::profile::Profile::from(&profile);
            app.emit("signaling:test-profile", profile).ok();
        }
        Err(err) => {
            log::warn!("Failed to load test profile: {err}");
            app.emit(
                "error",
                FrontendError::new(
                    "Profile error",
                    format!("Failed to load test profile: {err}"),
                ),
            )
            .ok();
        }
    };

    let mut state = app_state.lock().await;
    if state.config.client.test_profile_watcher_delay_ms > 0
        && let Some(parent) = path.parent()
    {
        let path_clone = path.clone();
        let app = app.clone();

        let debouncer = new_debouncer(
            Duration::from_millis(state.config.client.test_profile_watcher_delay_ms),
            None,
            move |res: DebounceEventResult| match res {
                Ok(events) => {
                    if events.iter().any(|e| {
                        matches!(e.kind, EventKind::Create(_) | EventKind::Modify(_))
                            && e.paths.iter().any(|p| p == &path_clone)
                    }) {
                        log::debug!("Test profile changed, reloading");
                        match Profile::load(&path_clone) {
                            Ok(profile) => {
                                log::debug!("Reloaded test profile: {:?}", profile);
                                let profile =
                                    vacs_signaling::protocol::profile::Profile::from(&profile);
                                app.emit("signaling:test-profile", profile).ok();
                            }
                            Err(err) => {
                                log::warn!("Failed to reload test profile: {err}");
                                app.emit(
                                    "error",
                                    FrontendError::new(
                                        "Profile error",
                                        format!("Failed to reload test profile: {err}"),
                                    ),
                                )
                                .ok();
                            }
                        }
                    }
                }
                Err(err) => log::warn!(
                    "Received error watching test profile parent directory for changes: {err:?}"
                ),
            },
        );

        match debouncer {
            Ok(mut debouncer) => {
                if let Err(err) = debouncer.watch(parent, RecursiveMode::NonRecursive) {
                    log::warn!("Failed to start watcher for test profile: {err}");
                } else {
                    log::trace!(
                        "Started watching parent directory {parent:?} for changes to test profile {path:?}"
                    );
                    state.test_profile_watcher = Some(debouncer);
                }
            }
            Err(err) => {
                log::error!("Failed to create debouncer: {err}");
            }
        }
    }

    Ok(Some(path))
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_unload_test_profile(app_state: State<'_, AppState>) -> Result<(), Error> {
    let mut state = app_state.lock().await;
    if state.test_profile_watcher.take().is_some() {
        log::debug!("Stopped watching test profile");
    }
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_get_client_page_settings(
    app_state: State<'_, AppState>,
) -> Result<FrontendClientPageSettings, Error> {
    let config = {
        let state = app_state.lock().await;
        FrontendClientPageSettings::from(&state.config)
    };
    Ok(config)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_set_selected_client_page_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
    config_name: Option<String>,
) -> Result<(), Error> {
    let config: PersistedClientConfig = {
        let mut state = app_state.lock().await;
        state.config.client.selected_client_page_config = config_name;

        state.config.client.clone().into()
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn app_load_extra_client_page_config(
    app: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<Option<PathBuf>, Error> {
    log::debug!("Loading extra client page config");

    let path = match rfd::AsyncFileDialog::new()
        .set_title("Select a client page configuration file")
        .add_filter("vacs client page config", &["toml"])
        .pick_file()
        .await
        .map(|p| p.path().to_path_buf())
    {
        Some(path) => path,
        None => return Ok(None),
    };

    log::debug!("Picked extra client page config file: {path:?}");

    let persisted_client_config = {
        let mut state = app_state.lock().await;
        if state
            .config
            .client
            .extra_client_page_config
            .as_ref()
            .is_some_and(|p| p == &path)
        {
            return Ok(Some(path));
        }

        state.config.client.extra_client_page_config = Some(path.to_string_lossy().to_string());
        PersistedClientConfig::from(state.config.client.clone())
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    log::debug!("Reloading configuration");
    let new_config = AppConfig::parse(&config_dir).context("Failed to reload configuration")?;

    app_state.lock().await.config = new_config.clone();

    let client_page_config = FrontendClientPageSettings::from(&new_config);
    app.emit("signaling:client-page-config", client_page_config)
        .ok();

    Ok(Some(path))
}
