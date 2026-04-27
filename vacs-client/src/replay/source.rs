//! Audio source abstraction for the replay recorder.
//!
//! Implementations produce a stream of [`ReplaySourceEvent`] values containing both PCM
//! frames and gating signals. The recorder is backend-agnostic and consumes whatever
//! `TapId` variants the source emits.
//!
//! Concrete sources live in OS-specific submodules. They differ in *both* the gating
//! signal source (e.g. TrackAudio's WebSocket events) and the PCM capture backend
//! (PipeWire on Linux, WASAPI loopback on Windows, ScreenCaptureKit-Audio on macOS),
//! and there is no meaningful overlap to share.

use crate::replay::{ReplayError, TapId};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use trackaudio::Frequency;

/// One unit of work emitted by an [`ReplaySource`].
#[derive(Debug, Clone)]
pub enum ReplaySourceEvent {
    TapOpened {
        tap: TapId,
        sample_rate: u32,
        channels: u16,
    },
    TapClosed {
        tap: TapId,
    },
    Frame {
        tap: TapId,
        samples: Arc<[f32]>,
        captured_at: Instant,
    },
    RxBegin {
        tap: TapId,
        callsign: String,
        frequency: Frequency,
    },
    RxEnd {
        callsign: String,
        frequency: Frequency,
        active_transmitters: Option<Vec<String>>,
    },
}

/// A source of replay audio + gating events.
#[async_trait::async_trait]
pub trait ReplaySource: Send {
    async fn start(&mut self) -> Result<mpsc::Receiver<ReplaySourceEvent>, ReplayError>;
    async fn stop(&mut self);
}

pub mod capture;

#[cfg(target_os = "linux")]
pub mod track_audio;

#[cfg(target_os = "linux")]
pub use track_audio::TrackAudioLoopbackSource;
