//! Playback recorder: glues an [`PlaybackSource`] to the [`Fsm`], a per-clip [`ClipWriter`],
//! and the [`ClipStore`] rolling deque.
//!
//! The recorder owns one tokio task that consumes [`PlaybackSourceEvent`]s, feeds them to
//! the FSM, and turns the resulting [`FsmAction`]s into file operations. A periodic ticker
//! drives hangover-based clip closure.

use crate::audio::PlaybackDeviceType;
use crate::playback::fsm::{Fsm, FsmAction};
use crate::playback::source::{PlaybackSource, PlaybackSourceEvent};
use crate::playback::storage::ClipStore;
use crate::playback::writer::ClipWriter;
use crate::playback::{ClipMeta, PlaybackConfig, PlaybackError, RecordingMode};
use parking_lot::Mutex;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use vacs_audio::sources::AudioSourceId;

const TICK_INTERVAL_MS: u64 = 100;
pub const CLIPS_MODIFIED_EVENT: &str = "playback:clips-modified";

pub const CLIP_PROGRESS_EVENT: &str = "playback:progress";

/// Snapshot of an in-flight clip the recorder is currently writing.
struct OpenClip {
    writer: ClipWriter,
    path: PathBuf,
    callsigns: HashSet<String>,
    frequency: Option<trackaudio::Frequency>,
    started_at: SystemTime,
}

impl OpenClip {
    /// Finalize the writer and assemble a [`ClipMeta`]. On writer error, returns the
    /// clip's path so the caller can discard it from the store.
    ///
    /// `duration_override` of `Some(0)` or `None` falls back to the writer's measured duration.
    fn finalize(
        self,
        id: u64,
        ended_at: Option<SystemTime>,
        duration_override: Option<u64>,
    ) -> Result<ClipMeta, (PathBuf, PlaybackError)> {
        let OpenClip {
            writer,
            path,
            callsigns,
            frequency,
            started_at,
        } = self;

        let actual_ms = match writer.finalize() {
            Ok(ms) => ms,
            Err(err) => return Err((path, err)),
        };
        let duration_ms = match duration_override {
            Some(d) if d != 0 => d,
            _ => actual_ms,
        };
        let ended_at = ended_at.unwrap_or_else(|| {
            started_at
                .checked_add(Duration::from_millis(duration_ms))
                .unwrap_or(started_at)
        });

        Ok(ClipMeta {
            id,
            path,
            callsigns,
            frequency,
            started_at,
            ended_at,
            duration_ms,
        })
    }
}

/// Public handle to a running recorder.
pub struct PlaybackRecorder {
    store: Arc<Mutex<ClipStore>>,
    playing_source_id: Option<(AudioSourceId, PlaybackDeviceType)>,
    cancel: CancellationToken,
    join: Option<JoinHandle<()>>,
}

/// Shared, app-managed slot holding the (optionally running) recorder, mirroring
/// [`crate::audio::manager::AudioManagerHandle`] and
/// [`crate::keybinds::engine::KeybindEngineHandle`].
///
/// The slot is created empty at startup and populated by [`crate::playback::PlaybackConfig::start`]
/// once the radio integration is up and playback is enabled in config. Replacing the slot
/// shuts down any previous recorder.
pub type PlaybackRecorderHandle = Arc<RwLock<Option<PlaybackRecorder>>>;

impl PlaybackRecorder {
    /// Spawn a recorder. Returns immediately; capture and writing happen on a background task.
    ///
    /// `app` is used to emit `playback:clip-recorded` and `playback:clip-evicted` events to
    /// the frontend whenever the rolling deque changes.
    pub async fn spawn(
        app: AppHandle,
        config: PlaybackConfig,
        clip_dir: PathBuf,
        mut source: Box<dyn PlaybackSource>,
    ) -> Result<Self, PlaybackError> {
        let store = Arc::new(Mutex::new(ClipStore::open(clip_dir, config.max_clips)?));
        let rx = source.start().await?;
        let cancel = CancellationToken::new();

        let join = {
            let store = store.clone();
            let cancel = cancel.clone();
            tokio::spawn(run(app, config, store, source, rx, cancel))
        };

        Ok(Self {
            store,
            playing_source_id: None,
            cancel,
            join: Some(join),
        })
    }

    pub fn list(&self) -> Vec<ClipMeta> {
        self.store.lock().list()
    }

    pub fn delete(&self, id: u64) -> Result<bool, PlaybackError> {
        self.store.lock().delete(id)
    }

    pub fn clear(&self) -> Result<(), PlaybackError> {
        self.store.lock().clear()
    }

    pub fn export(
        &self,
        id: u64,
        target: Option<&std::path::Path>,
    ) -> Result<PathBuf, PlaybackError> {
        self.store.lock().export(id, target)
    }

    pub fn get(&self, id: u64) -> Option<ClipMeta> {
        self.store.lock().get(id)
    }

    /// The most recently finished clip, if any.
    pub fn latest(&self) -> Option<ClipMeta> {
        self.store.lock().latest()
    }

    pub fn set_playing_source_id(&mut self, id: Option<(AudioSourceId, PlaybackDeviceType)>) {
        self.playing_source_id = id;
    }

    pub fn take_playing_source_id(&mut self) -> Option<(AudioSourceId, PlaybackDeviceType)> {
        self.playing_source_id.take()
    }

    pub fn get_playing_source_id(&self) -> Option<(AudioSourceId, PlaybackDeviceType)> {
        self.playing_source_id
    }

    /// Cancel the recorder's background task and wait for it to fully exit, including its
    /// underlying [`PlaybackSource`]. Once this returns, any resources the source held (e.g.
    /// a radio's `Arc`) are guaranteed to have been released.
    pub async fn shutdown(mut self) {
        self.cancel.cancel();
        if let Some(join) = self.join.take()
            && let Err(err) = join.await
        {
            log::warn!("playback recorder task panicked: {err}");
        }
    }
}

/// Best-effort fallback for recorders dropped without an explicit [`PlaybackRecorder::shutdown`]
/// call. `drop` can't await the background task's exit, so this only requests cancellation -
/// callers that need the underlying [`PlaybackSource`] to have released its resources by the
/// time they continue must call `shutdown` instead.
impl Drop for PlaybackRecorder {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn run(
    app: AppHandle,
    config: PlaybackConfig,
    store: Arc<Mutex<ClipStore>>,
    mut source: Box<dyn PlaybackSource>,
    mut rx: mpsc::Receiver<PlaybackSourceEvent>,
    cancel: CancellationToken,
) {
    let mode = cfg_select! {
        target_os = "linux" => {
            RecordingMode::PerTap
        }
        _ => {
            RecordingMode::Mixed
        }
    };

    let mut fsm = Fsm::new(&config, mode);
    let mut open: HashMap<u64, OpenClip> = HashMap::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_INTERVAL_MS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                log::debug!("recorder shutting down");
                break;
            }
            _ = ticker.tick() => {
                let actions = fsm.tick(Instant::now(), SystemTime::now());
                apply_actions(&app, actions, &store, &mut open);
            }
            event = rx.recv() => {
                let Some(event) = event else {
                    log::warn!("audio source closed; recorder exiting");
                    break;
                };
                let actions = fsm.on_event(event, Instant::now(), SystemTime::now());
                apply_actions(&app, actions, &store, &mut open);
            }
        }
    }

    // Drop the receiver before awaiting the source's shutdown below: the source's forwarder
    // task(s) may still be mid-`send` on this channel, and with nobody left to drain it a full
    // channel would block that send forever. A closed receiver makes `send` fail fast instead.
    drop(rx);

    // Finalize any in-flight clips on shutdown so we don't leak partial files.
    for (clip_id, clip) in open.drain() {
        match clip.finalize(clip_id, None, None) {
            Ok(meta) => {
                let recorded = meta.clone();
                let evicted = store.lock().commit(meta);
                let _ = app.emit(
                    CLIPS_MODIFIED_EVENT,
                    ClipsModifiedEvent { recorded, evicted },
                );
            }
            Err((path, err)) => {
                log::warn!("failed to finalize clip during shutdown: {err}");
                store.lock().discard(&path);
            }
        }
    }

    source.stop().await;
}

fn apply_actions(
    app: &AppHandle,
    actions: Vec<FsmAction>,
    store: &Mutex<ClipStore>,
    open: &mut HashMap<u64, OpenClip>,
) {
    for action in actions {
        match action {
            FsmAction::OpenClip {
                clip_id,
                tap,
                sample_rate,
                channels,
                callsigns,
                frequency,
                started_at,
            } => {
                let path = store.lock().allocate(tap, started_at);
                match ClipWriter::create(&path, sample_rate, channels) {
                    Ok(writer) => {
                        log::trace!("opened clip {clip_id} at {}", path.display());
                        open.insert(
                            clip_id,
                            OpenClip {
                                writer,
                                path,
                                callsigns,
                                frequency,
                                started_at,
                            },
                        );
                    }
                    Err(err) => {
                        log::warn!("failed to open clip: {err}");
                        store.lock().discard(&path);
                    }
                }
            }
            FsmAction::WriteFrame { clip_id, samples } => {
                if let Some(clip) = open.get_mut(&clip_id)
                    && let Err(err) = clip.writer.write_frame(&samples)
                {
                    log::warn!("failed writing frame: {err}");
                }
            }
            FsmAction::CloseClip {
                clip_id,
                callsigns,
                ended_at,
                duration_ms,
            } => {
                let Some(mut clip) = open.remove(&clip_id) else {
                    continue;
                };

                clip.callsigns = callsigns;

                match clip.finalize(clip_id, Some(ended_at), Some(duration_ms)) {
                    Ok(meta) => {
                        let recorded = meta.clone();
                        let evicted = store.lock().commit(meta);
                        let _ = app.emit(
                            CLIPS_MODIFIED_EVENT,
                            ClipsModifiedEvent { recorded, evicted },
                        );
                    }
                    Err((path, err)) => {
                        log::error!("failed to finalize clip: {err}");
                        store.lock().discard(&path);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClipsModifiedEvent {
    recorded: ClipMeta,
    evicted: Vec<ClipMeta>,
}
