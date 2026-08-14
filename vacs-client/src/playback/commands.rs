use crate::app::PersistedClientConfig;
use crate::app::state::AppState;
use crate::audio::PlaybackDeviceType;
use crate::audio::manager::AudioManagerHandle;
use crate::config::{CLIENT_SETTINGS_FILE_NAME, Persistable};
use crate::error::Error;
use crate::playback::recorder::{CLIP_PROGRESS_EVENT, PlaybackRecorderHandle};
use crate::playback::{ClipMeta, PlaybackError};
use crate::radio::{RadioHandle, RadioState};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use vacs_audio::sources::wav::WavSource;

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_get_enabled(app_state: State<'_, AppState>) -> Result<bool, Error> {
    Ok(app_state.lock().await.config.client.playback.enabled)
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_set_enabled(
    app: AppHandle,
    app_state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), Error> {
    let (persisted_client_config, playback_config) = {
        let mut state = app_state.lock().await;

        if state.config.client.playback.enabled == enabled {
            return Ok(());
        }

        state.config.client.playback.enabled = enabled;
        let playback_config = state.config.client.playback.clone();
        (
            PersistedClientConfig::from(state.config.client.clone()),
            playback_config,
        )
    };

    let config_dir = app
        .path()
        .app_config_dir()
        .expect("Cannot get config directory");
    persisted_client_config.persist(&config_dir, CLIENT_SETTINGS_FILE_NAME)?;

    if enabled {
        // Start the recorder live if the radio is currently active. If not, the
        // recorder will be started the next time the radio integration comes up.
        let radio = app.state::<RadioHandle>().read().clone();
        if let Some(radio) = radio
            && !matches!(
                radio.state(),
                RadioState::NotConfigured | RadioState::Disconnected
            )
        {
            playback_config.start(&app, radio).await?;
        } else {
            log::info!("playback enabled in config but no radio is active");
        }
    } else {
        // Stop any currently running recorder. The slot stays in place; future
        // PlaybackConfig::start calls will be no-ops while playback is disabled.
        let handle = app.state::<PlaybackRecorderHandle>();
        let existing = handle.write().take();
        if let Some(recorder) = existing {
            recorder.shutdown().await;
            log::info!("playback disabled; stopped active recorder");
        }
    }

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_list(
    recorder: State<'_, PlaybackRecorderHandle>,
) -> Result<Vec<ClipMeta>, Error> {
    Ok(recorder
        .read()
        .as_ref()
        .map(|r| r.list())
        .unwrap_or_default())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_delete(
    recorder: State<'_, PlaybackRecorderHandle>,
    id: u64,
) -> Result<bool, Error> {
    Ok(recorder
        .read()
        .as_ref()
        .map(|r| r.delete(id))
        .transpose()?
        .unwrap_or_default())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_clear(recorder: State<'_, PlaybackRecorderHandle>) -> Result<(), Error> {
    if let Some(r) = recorder.read().as_ref() {
        r.clear()?;
    }
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_start(
    app: AppHandle,
    recorder: State<'_, PlaybackRecorderHandle>,
    audio_manager: State<'_, AudioManagerHandle>,
    id: u64,
    device_type: PlaybackDeviceType,
    initial_progress: Option<f64>, // 0.0 to 1.0
    start_paused: Option<bool>,
) -> Result<(), Error> {
    stop_playing_source(&recorder, &audio_manager);

    let clip = recorder.read().as_ref().and_then(|r| r.get(id));
    let Some(clip) = clip else {
        return Err(PlaybackError::Other(anyhow::anyhow!("clip {id} not found")).into());
    };

    let audio_manager = audio_manager.read();

    let (source_id, actual_device_type) = audio_manager.add_audio_source(
        move |sample_rate, channels| {
            let source = WavSource::from_file(
                clip.path,
                sample_rate,
                channels as usize,
                1.0,
                None,
                Some(Box::new(move |progress| {
                    app.emit(CLIP_PROGRESS_EVENT, progress).ok();
                    if progress >= 1.0 {
                        let recorder = app.state::<PlaybackRecorderHandle>();
                        if let Some(r) = recorder.write().as_mut() {
                            r.set_playing_source_id(None);
                        }
                    }
                })),
            )
            .map_err(PlaybackError::Other)?;
            Ok(Box::new(source))
        },
        device_type,
    )?;

    let initial_progress = initial_progress.unwrap_or(0.0);
    if initial_progress > 0.0 {
        audio_manager.skip_in_audio_source(
            source_id,
            Duration::from_millis((clip.duration_ms as f64 * initial_progress).round() as u64),
            actual_device_type,
        );
    }

    let start_paused = start_paused.unwrap_or(false);
    if !start_paused {
        audio_manager.start_audio_source(source_id, actual_device_type);
    }

    if let Some(r) = recorder.write().as_mut() {
        r.set_playing_source_id(Some((source_id, actual_device_type)));
    }

    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_pause(
    recorder: State<'_, PlaybackRecorderHandle>,
    audio_manager: State<'_, AudioManagerHandle>,
) -> Result<(), Error> {
    if let Some((source_id, is_speaker)) = recorder
        .read()
        .as_ref()
        .and_then(|r| r.get_playing_source_id())
    {
        audio_manager
            .read()
            .stop_audio_source(source_id, is_speaker);
    } else {
        log::warn!("playback pause called but no clip is playing");
    }
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_continue(
    recorder: State<'_, PlaybackRecorderHandle>,
    audio_manager: State<'_, AudioManagerHandle>,
) -> Result<(), Error> {
    if let Some((source_id, is_speaker)) = recorder
        .read()
        .as_ref()
        .and_then(|r| r.get_playing_source_id())
    {
        audio_manager
            .read()
            .start_audio_source(source_id, is_speaker);
    } else {
        log::warn!("playback continue called but no clip is paused");
    }
    Ok(())
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_stop(
    recorder: State<'_, PlaybackRecorderHandle>,
    audio_manager: State<'_, AudioManagerHandle>,
) -> Result<(), Error> {
    if !stop_playing_source(&recorder, &audio_manager) {
        log::warn!("playback stop called but no clip is playing");
    }

    Ok(())
}

fn stop_playing_source(
    recorder: &State<PlaybackRecorderHandle>,
    audio_manager: &State<AudioManagerHandle>,
) -> bool {
    if let Some((source_id, is_speaker)) = recorder
        .write()
        .as_mut()
        .and_then(|r| r.take_playing_source_id())
    {
        audio_manager
            .read()
            .remove_audio_source(source_id, is_speaker);
        return true;
    }
    false
}

#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_seek(
    recorder: State<'_, PlaybackRecorderHandle>,
    audio_manager: State<'_, AudioManagerHandle>,
    millis: i64,
) -> Result<(), Error> {
    if millis == 0 {
        return Ok(());
    }

    if let Some((source_id, is_speaker)) = recorder
        .read()
        .as_ref()
        .and_then(|r| r.get_playing_source_id())
    {
        let millis_abs = millis.unsigned_abs();
        if millis < 0 {
            audio_manager.read().rewind_in_audio_source(
                source_id,
                Duration::from_millis(millis_abs),
                is_speaker,
            );
        } else {
            audio_manager.read().skip_in_audio_source(
                source_id,
                Duration::from_millis(millis_abs),
                is_speaker,
            );
        }
    } else {
        log::warn!("playback skip called but no clip is playing");
    }

    Ok(())
}

/// Copy a clip to the saved directory within the app data dir. Saved clips are exempt
/// from rolling-deque eviction. Returns the destination path.
#[tauri::command]
#[vacs_macros::log_err]
pub async fn playback_export(
    recorder: State<'_, PlaybackRecorderHandle>,
    id: u64,
) -> Result<PathBuf, Error> {
    let Some(path) = recorder
        .read()
        .as_ref()
        .map(|r| r.export(id, None))
        .transpose()?
    else {
        return Err(Error::Other(Box::new(anyhow::anyhow!(
            "recorder not running"
        ))));
    };

    // The export itself succeeded at this point, so the error must say so and carry the
    // destination: the frontend discards the returned path and this overlay is the only surface
    // that can still tell the user where the clip went.
    if let Err(err) = crate::external::open_path_detached(path.clone()).await {
        return Err(PlaybackError::Other(anyhow::anyhow!(
            "Exported the clip to {} but could not open the folder: {err:#}",
            path.display()
        ))
        .into());
    }

    Ok(path)
}
