mod app;
mod audio;
mod auth;
mod build;
mod config;
mod error;
mod keybinds;
mod platform;
mod radio;
mod remote;
mod secrets;
mod signaling;

use crate::app::open_fatal_error_dialog;
use crate::app::state::audio::AppStateAudioExt;
use crate::app::state::http::HttpState;
use crate::app::state::keybinds::AppStateKeybindsExt;
use crate::app::state::{AppState, AppStateInner};
use crate::audio::manager::AudioManagerHandle;
use crate::build::VersionInfo;
use crate::config::{CLIENT_SETTINGS_FILE_NAME, Persistable, PersistedClientConfig};
use crate::error::{StartupError, StartupErrorExt};
use crate::keybinds::engine::KeybindEngineHandle;
use crate::platform::Capabilities;
use crate::remote::{RemoteServer, RemoteServerHandle};
use tauri::{App, Manager, RunEvent, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tokio::sync::Mutex as TokioMutex;

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .max_file_size(1_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .level(log::LevelFilter::Warn)
                .level_for("vacs_client_lib", log::LevelFilter::Trace)
                .level_for("vacs_audio", log::LevelFilter::Trace)
                .level_for("vacs_signaling", log::LevelFilter::Trace)
                .level_for("vacs_vatsim", log::LevelFilter::Trace)
                .level_for("vacs_webrtc", log::LevelFilter::Trace)
                .level_for("trackaudio", log::LevelFilter::Trace)
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            if let Some(url) = argv.get(1) {
                app::handle_deep_link(app.clone(), url.to_string());
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::default().build())
        .plugin(tauri_plugin_prevent_default::debug())
        .setup(|app| {
            log::info!("{:?}", VersionInfo::gather());

            if rustls::crypto::aws_lc_rs::default_provider().install_default().is_err()  {
                log::error!("Failed to install rustls crypto provider");
                open_fatal_error_dialog(app.handle(), "Failed to install rustls crypto provider");
                return Err(anyhow::anyhow!("Failed to install rustls crypto provider").into());
            }

            #[cfg(target_os = "macos")]
            {
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    if let Some(url) = event.urls().first() {
                        app::handle_deep_link(handle.clone(), url.to_string());
                    }
                });
            }

            async fn setup(app: &mut App) -> Result<(), StartupError> {
                #[cfg(not(target_os = "macos"))]
                {
                    use anyhow::Context;

                    app.deep_link()
                        .register_all()
                        .context("Failed to register deep link")
                        .map_startup_err(StartupError::Other)?;
                }

                let capabilities = Capabilities::default();

                let state = AppStateInner::new(app.handle())?;

                let transmit_config = state.config.client.transmit_config.clone();
                let call_control_config = state.config.client.keybinds.clone();
                let keybind_engine = state.keybind_engine_handle();
                let remote_config = state.config.client.remote.clone();

                app.manage::<HttpState>(HttpState::new(app.handle())?);
                app.manage::<AudioManagerHandle>(state.audio_manager_handle());
                app.manage::<AppState>(TokioMutex::new(state));

                if capabilities.keybind_listener || capabilities.keybind_emitter {
                    keybind_engine
                        .write()
                        .await
                        .set_config(&transmit_config, &call_control_config)
                        .await
                        .map_startup_err(StartupError::Keybinds)?;
                } else {
                    log::warn!("Your platform ({}) does not support keybind listener and emitter, skipping registration", capabilities.platform);
                }

                app.manage::<KeybindEngineHandle>(keybind_engine);

                let mut remote_handle = RemoteServer::new(app.handle().clone());
                if remote_config.enabled {
                    remote_handle.start(remote_config.listen_addr, remote_config.serve_frontend);
                } else {
                    log::info!("Remote control server is disabled");
                }
                app.manage::<RemoteServerHandle>(TokioMutex::new(remote_handle));

                Ok(())
            }

            if let Err(err) = tauri::async_runtime::block_on(setup(app)) {
                log::error!("Startup failed. Err: {err:?}");

                open_fatal_error_dialog(app.handle(), &err.to_string());

                return Err(anyhow::anyhow!("{err}").into());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::commands::app_check_for_update,
            app::commands::app_frontend_ready,
            app::commands::app_get_call_config,
            app::commands::app_get_client_page_settings,
            app::commands::app_get_clock_mode,
            app::commands::app_get_version,
            app::commands::app_load_extra_client_page_config,
            app::commands::app_load_test_profile,
            app::commands::app_open_folder,
            app::commands::app_platform_capabilities,
            app::commands::app_quit,
            app::commands::app_reset_window_size,
            app::commands::app_set_always_on_top,
            app::commands::app_set_call_config,
            app::commands::app_set_clock_mode,
            app::commands::app_set_fullscreen,
            app::commands::app_set_selected_client_page_config,
            app::commands::app_change_zoom_level,
            app::commands::app_unload_test_profile,
            app::commands::app_update,
            audio::commands::audio_get_devices,
            audio::commands::audio_get_hosts,
            audio::commands::audio_get_volumes,
            audio::commands::audio_play_ui_click,
            audio::commands::audio_set_device,
            audio::commands::audio_set_host,
            audio::commands::audio_set_radio_prio,
            audio::commands::audio_set_volume,
            audio::commands::audio_start_input_level_meter,
            audio::commands::audio_stop_input_level_meter,
            auth::commands::auth_check_session,
            auth::commands::auth_logout,
            auth::commands::auth_open_oauth_url,
            keybinds::commands::keybinds_get_external_binding,
            keybinds::commands::keybinds_get_keybinds_config,
            keybinds::commands::keybinds_get_radio_config,
            keybinds::commands::keybinds_get_radio_state,
            keybinds::commands::keybinds_get_transmit_config,
            keybinds::commands::keybinds_open_system_shortcuts_settings,
            keybinds::commands::keybinds_reconnect_radio,
            keybinds::commands::keybinds_set_binding,
            keybinds::commands::keybinds_set_radio_config,
            keybinds::commands::keybinds_set_transmit_config,
            signaling::commands::signaling_accept_call,
            signaling::commands::signaling_add_ignored_client,
            signaling::commands::signaling_connect,
            signaling::commands::signaling_disconnect,
            signaling::commands::signaling_end_call,
            signaling::commands::signaling_get_ignored_clients,
            signaling::commands::signaling_remove_ignored_client,
            signaling::commands::signaling_start_call,
            signaling::commands::signaling_terminate,
            remote::commands::remote_broadcast_store_sync,
            remote::commands::remote_get_config,
            remote::commands::remote_is_enabled,
            remote::commands::remote_set_config,
        ])
        .build(tauri::generate_context!())
        .expect("Failed to build tauri application")
        .run(move |app_handle, event| {
            if let RunEvent::WindowEvent {event: WindowEvent::CloseRequested {..}, ..} = event {
                let app_handle = app_handle.clone();
                tauri::async_runtime::block_on(async move {
                    app_handle
                        .state::<HttpState>()
                        .persist()
                        .expect("Failed to persist http state");

                    let mut client_config = app_handle.state::<AppState>().lock().await.config.client.clone();
                    if !client_config.fullscreen {
                        match client_config.update_window_state(&app_handle) {
                            Ok(()) => {
                                let config_dir = app_handle
                                    .path()
                                    .app_config_dir()
                                    .expect("Cannot get config directory");
                                let persisted_config: PersistedClientConfig = client_config.into();
                                persisted_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)
                                    .expect("Failed to persist client config");
                            }
                            Err(err) => log::warn!("Failed to update window state, window position and size will not be persisted: {err}")
                        }
                    }

                    app_handle.state::<KeybindEngineHandle>().write().await.shutdown();

                    app_handle
                        .state::<TokioMutex<RemoteServer>>()
                        .lock()
                        .await
                        .stop();

                    app_handle.state::<AppState>().lock().await.shutdown();
                });
            }
        });
}
