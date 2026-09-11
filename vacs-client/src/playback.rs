pub mod commands;
pub mod fsm;
pub mod recorder;
pub mod source;
pub mod storage;
pub mod writer;

use crate::playback::recorder::PlaybackRecorderHandle;
use crate::radio::DynRadio;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;
use tauri::{AppHandle, Manager};
use thiserror::Error;
use trackaudio::Frequency;

/// Identifies a virtual capture tap that the recorder treats as an independent stream.
///
/// A "tap" is the conceptual point where audio is intercepted from the host application.
///
/// Different audio sources produce different variants:
/// - `Frequency(_)` is reserved for a future native vacs radio stack that taps individual
///   radio stations directly.
/// - `Headset` and `Speaker` correspond to the two output buses exposed by radio clients
///   (e.g., using afv-native) that can be captured separately.
/// - `Merged` is produced by sources that cannot separate streams (e.g. WASAPI process
///   loopback on Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TapId {
    Frequency(Frequency),
    Headset,
    Speaker,
    Merged,
}

impl TapId {
    /// Token suitable for embedding in clip filenames.
    pub fn filename_token(&self) -> String {
        match self {
            TapId::Frequency(f) => format!("freq-{}", f.as_khz()),
            TapId::Headset => "headset".to_owned(),
            TapId::Speaker => "speaker".to_owned(),
            TapId::Merged => "merged".to_owned(),
        }
    }
}

/// Metadata for a single recorded clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipMeta {
    pub id: u64,
    pub path: PathBuf,
    pub callsigns: HashSet<String>,
    pub frequency: Option<Frequency>,
    pub started_at: SystemTime,
    pub ended_at: SystemTime,
    pub duration_ms: u64,
}

/// One station receiving on a specific frequency. The same callsign can be active on
/// multiple frequencies simultaneously, so both fields are required to identify it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Transmitter {
    pub callsign: String,
    pub frequency: Frequency,
}

/// Whether the recorder keeps per-tap clips or collapses everything into one virtual stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    /// Collapse all incoming `TapId` values into [`TapId::Merged`] before recording.
    Mixed,
    /// Keep each `TapId` as its own clip stream.
    #[default]
    PerTap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackConfig {
    pub enabled: bool,
    pub max_clips: usize,
    pub hangover_ms: u64,
    pub max_clip_duration_s: u64,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_clips: 15,
            hangover_ms: 250,
            max_clip_duration_s: 90,
        }
    }
}

impl PlaybackConfig {
    pub async fn start(&self, app: &AppHandle, radio: DynRadio) -> Result<(), PlaybackError> {
        if !self.enabled {
            log::info!("playback disabled by config");
            return Ok(());
        }

        let source = match make_source(radio) {
            Ok(s) => s,
            Err(PlaybackError::Unsupported) => {
                log::warn!(
                    "playback enabled in config, but capture is not supported on this platform"
                );
                return Err(PlaybackError::Unsupported);
            }
            Err(err) => {
                log::error!("failed to build playback source: {err}");
                return Err(err);
            }
        };

        let app_data_dir = match app.path().app_local_data_dir() {
            Ok(d) => d,
            Err(err) => {
                log::error!("failed to resolve app_data_dir: {err}");
                return Err(PlaybackError::Other(err.into()));
            }
        };
        let clip_dir = app_data_dir.join("playback");

        log::info!("starting recorder, clip dir = {}", clip_dir.display());

        match recorder::PlaybackRecorder::spawn(app.clone(), self.clone(), clip_dir, source).await {
            Ok(recorder) => {
                let handle = app.state::<PlaybackRecorderHandle>();
                // Swap the new recorder in before awaiting the old one's shutdown: leaving the
                // slot empty across the await lets a concurrent `start` install a recorder that
                // the write below would silently clobber without shutting it down.
                let existing = (*handle.write()).replace(recorder);
                if let Some(existing) = existing {
                    existing.shutdown().await;
                }
                log::debug!("recorder running");
                Ok(())
            }
            Err(err) => {
                log::error!("failed to start recorder: {err}");
                Err(err)
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("Playback I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("WAV writer error: {0}")]
    Wav(String),
    #[error("Audio source error: {0}")]
    Source(String),
    #[error("Playback capture not supported on this platform")]
    #[cfg_attr(
        any(target_os = "linux", target_os = "windows"),
        allow(
            dead_code,
            reason = "make_source returns Ok on Linux/Windows; only macOS uses this"
        )
    )]
    Unsupported,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Build the platform-specific playback source. Returns [`PlaybackError::Unsupported`]
/// on platforms where no loopback capture backend is implemented yet.
fn make_source(
    #[cfg_attr(target_os = "macos", allow(unused_variables))] radio: DynRadio,
) -> Result<Box<dyn source::PlaybackSource>, PlaybackError> {
    cfg_select! {
        target_os = "linux" => {
            match radio.as_any().downcast::<crate::radio::track_audio::TrackAudioRadio>() {
                Ok(radio) => Ok(Box::new(source::TrackAudioLoopbackSource::new(radio.events(), radio.state_handle()))),
                Err(_) => Err(PlaybackError::Unsupported),
            }
        }
        target_os = "windows" => {
            let radio = match radio.as_any().downcast::<crate::radio::track_audio::TrackAudioRadio>() {
                Ok(radio) => return Ok(Box::new(source::TrackAudioLoopbackSource::new(radio.events(), radio.state_handle()))),
                Err(radio) => radio,
            };

            match radio.downcast::<crate::radio::push_to_talk::PushToTalkRadio>() {
                Ok(_) => Ok(Box::new(source::AudioForVatsimLoopbackSource::new())),
                Err(_) => Err(PlaybackError::Unsupported),
            }
        }
        _ => {
            Err(PlaybackError::Unsupported)
        }
    }
}
