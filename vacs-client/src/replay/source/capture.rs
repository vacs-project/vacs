//! Loopback capture: tap the audio output of an external application (TrackAudio /
//! afv-native) and forward raw frames into the replay pipeline.
//!
//! This is OS-specific and intentionally does not go through any virtual sink:
//! - **Linux**: PipeWire link-factory wires afv-native's output streams directly into
//!   our own capture streams ([`linux`] submodule).
//! - **Windows**: WASAPI process loopback (planned, [`windows`] submodule).
//! - **macOS**: ScreenCaptureKit-Audio (planned, [`macos`] submodule).
//!
//! Per-platform implementations all funnel into the platform-agnostic [`LoopbackEvent`]
//! type so the rest of the replay pipeline stays portable.

use crate::replay::{ReplayError, TapId};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Lower-level event emitted by a platform loopback capture backend.
#[derive(Debug, Clone)]
pub enum LoopbackEvent {
    Opened {
        tap: TapId,
        sample_rate: u32,
        channels: u16,
    },
    Closed {
        tap: TapId,
    },
    Frame {
        tap: TapId,
        samples: Arc<[f32]>,
        captured_at: Instant,
    },
}

/// Platform-agnostic interface to a loopback capture backend.
///
/// Implementations spawn whatever OS resources they need (PipeWire main loop on Linux,
/// WASAPI process loopback on Windows, etc.) and forward [`LoopbackEvent`]s through the
/// returned channel until [`LoopbackCapture::stop`] is called or the handle is dropped.
pub trait LoopbackCapture: Send + 'static {
    /// Start capturing. Returns the handle and the event receiver.
    fn start() -> Result<(Self, mpsc::Receiver<LoopbackEvent>), ReplayError>
    where
        Self: Sized;

    /// Stop capturing. Should be idempotent.
    fn stop(&mut self);
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::AfvNativePipewireCapture;

/// The default [`LoopbackCapture`] backend for the current platform.
#[cfg(target_os = "linux")]
pub type DefaultLoopbackCapture = AfvNativePipewireCapture;
