//! Replay recorder: glues an [`ReplaySource`] to the [`Fsm`], a per-clip [`ClipWriter`],
//! and the [`ClipStore`] rolling deque.
//!
//! The recorder owns one tokio task that consumes [`ReplaySourceEvent`]s, feeds them to
//! the FSM, and turns the resulting [`FsmAction`]s into file operations. A periodic ticker
//! drives hangover-based clip closure.

use crate::replay::fsm::{Fsm, FsmAction};
use crate::replay::source::{ReplaySource, ReplaySourceEvent};
use crate::replay::storage::ClipStore;
use crate::replay::writer::ClipWriter;
use crate::replay::{ClipMeta, ReplayConfig, ReplayError};
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const TICK_INTERVAL_MS: u64 = 100;
const CLIP_RECORDED_EVENT: &str = "replay:clip-recorded";
const CLIP_EVICTED_EVENT: &str = "replay:clip-evicted";

/// Snapshot of an in-flight clip the recorder is currently writing.
struct OpenClip {
    writer: ClipWriter,
    path: PathBuf,
    tap: crate::replay::TapId,
    callsign: Option<String>,
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
    ) -> Result<ClipMeta, (PathBuf, ReplayError)> {
        let OpenClip {
            writer,
            path,
            tap,
            callsign,
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
            tap,
            callsign,
            frequency,
            started_at,
            ended_at,
            duration_ms,
        })
    }
}

/// Public handle to a running recorder.
pub struct ReplayRecorder {
    store: Arc<Mutex<ClipStore>>,
    cancel: CancellationToken,
}

/// Shared, app-managed slot holding the (optionally running) recorder, mirroring
/// [`crate::audio::manager::AudioManagerHandle`] and
/// [`crate::keybinds::engine::KeybindEngineHandle`].
///
/// The slot is created empty at startup and populated by [`crate::replay::ReplayConfig::start`]
/// once the radio integration is up and replay is enabled in config. Replacing the slot
/// shuts down any previous recorder.
pub type ReplayRecorderHandle = Arc<RwLock<Option<ReplayRecorder>>>;

impl ReplayRecorder {
    /// Spawn a recorder. Returns immediately; capture and writing happen on a background task.
    ///
    /// `app` is used to emit `replay:clip-recorded` and `replay:clip-evicted` events to
    /// the frontend whenever the rolling deque changes.
    pub async fn spawn(
        app: AppHandle,
        config: ReplayConfig,
        clip_dir: PathBuf,
        mut source: Box<dyn ReplaySource>,
    ) -> Result<Self, ReplayError> {
        let store = Arc::new(Mutex::new(ClipStore::open(clip_dir, config.max_clips)?));
        let rx = source.start().await?;
        let cancel = CancellationToken::new();

        {
            let store = store.clone();
            let cancel = cancel.clone();
            tokio::spawn(run(app, config, store, source, rx, cancel));
        }

        Ok(Self { store, cancel })
    }

    pub fn list(&self) -> Vec<ClipMeta> {
        self.store.lock().list()
    }

    pub fn delete(&self, id: u64) -> Result<bool, ReplayError> {
        self.store.lock().delete(id)
    }

    pub fn clear(&self) -> Result<(), ReplayError> {
        self.store.lock().clear()
    }

    pub fn export(
        &self,
        id: u64,
        target: Option<&std::path::Path>,
    ) -> Result<PathBuf, ReplayError> {
        self.store.lock().export(id, target)
    }

    pub fn get(&self, id: u64) -> Option<ClipMeta> {
        self.store.lock().get(id)
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

impl Drop for ReplayRecorder {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn run(
    app: AppHandle,
    config: ReplayConfig,
    store: Arc<Mutex<ClipStore>>,
    mut source: Box<dyn ReplaySource>,
    mut rx: mpsc::Receiver<ReplaySourceEvent>,
    cancel: CancellationToken,
) {
    let mut fsm = Fsm::new(&config);
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

    // Finalize any in-flight clips on shutdown so we don't leak partial files.
    for (clip_id, clip) in open.drain() {
        match clip.finalize(clip_id, None, None) {
            Ok(meta) => {
                let _ = app.emit(CLIP_RECORDED_EVENT, &meta);
                let evicted = store.lock().commit(meta);
                for ev in evicted {
                    let _ = app.emit(CLIP_EVICTED_EVENT, &ev);
                }
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
                callsign,
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
                                tap,
                                callsign,
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
                ended_at,
                duration_ms,
            } => {
                let Some(clip) = open.remove(&clip_id) else {
                    continue;
                };

                match clip.finalize(clip_id, Some(ended_at), Some(duration_ms)) {
                    Ok(meta) => {
                        let _ = app.emit(CLIP_RECORDED_EVENT, &meta);
                        let evicted = store.lock().commit(meta);
                        for ev in evicted {
                            log::trace!("evicted clip {} ({})", ev.id, ev.path.display());
                            let _ = app.emit(CLIP_EVICTED_EVENT, &ev);
                        }
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
