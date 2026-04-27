//! TrackAudio replay source: combines a [`LoopbackCapture`] backend (PCM frames from
//! afv-native's output streams) with TrackAudio's WebSocket events (RX gating and
//! per-frequency speaker/headset routing) into a single [`ReplaySource`].
//!
//! The TrackAudio gating logic (callsign/frequency tracking, mid-RX tap migration
//! when the user toggles speaker/headset) is platform-agnostic. Only the underlying
//! [`LoopbackCapture`] implementation differs per OS, selected via
//! [`super::capture::DefaultLoopbackCapture`].

use super::capture::{DefaultLoopbackCapture, LoopbackCapture, LoopbackEvent};
use super::{ReplaySource, ReplaySourceEvent};
use crate::radio::track_audio::TrackAudioRadio;
use crate::replay::{ReplayError, TapId, Transmitter};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// ReplaySource backed by:
/// - A platform [`LoopbackCapture`] for PCM frames from afv-native's output streams.
/// - `TrackAudioRadio` event broadcast for `RxBegin`/`RxEnd` gating, with the
///   tap's `headset` flag mapping each event to either [`TapId::Headset`]
///   or [`TapId::Speaker`].
pub struct TrackAudioLoopbackSource {
    radio: Arc<TrackAudioRadio>,
    capture: Option<DefaultLoopbackCapture>,
    cancel: CancellationToken,
}

impl TrackAudioLoopbackSource {
    pub fn new(radio: Arc<TrackAudioRadio>) -> Self {
        Self {
            radio,
            capture: None,
            cancel: CancellationToken::new(),
        }
    }
}

#[async_trait::async_trait]
impl ReplaySource for TrackAudioLoopbackSource {
    async fn start(&mut self) -> Result<mpsc::Receiver<ReplaySourceEvent>, ReplayError> {
        let (tx, rx) = mpsc::channel::<ReplaySourceEvent>(EVENT_CHANNEL_CAPACITY);

        let (capture, mut capture_rx) = DefaultLoopbackCapture::start()?;

        let mut events = self.radio.subscribe_events();
        let radio = self.radio.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            // Track which tap each currently-active receiver was opened on so that:
            //   1. RxEnd emits on the same tap RxBegin used (even if routing has since
            //      flipped), keeping book-keeping symmetric.
            //   2. When the user toggles speaker/headset for a frequency mid-RX, we
            //      synthesize RxEnd on the old tap + RxBegin on the new tap so the
            //      remainder of the transmission lands in a fresh clip on the new tap.
            let mut active_rx: HashMap<Transmitter, TapId> = HashMap::new();

            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        log::trace!("forwarder cancelled");
                        break;
                    }
                    evt = capture_rx.recv() => {
                        let Some(evt) = evt else {
                            log::warn!("loopback capture channel closed");
                            break;
                        };
                        let mapped = match evt {
                            LoopbackEvent::Opened { tap, sample_rate, channels } => {
                                ReplaySourceEvent::TapOpened { tap, sample_rate, channels }
                            }
                            LoopbackEvent::Closed { tap } => {
                                ReplaySourceEvent::TapClosed { tap }
                            }
                            LoopbackEvent::Frame { tap, samples, captured_at } => {
                                ReplaySourceEvent::Frame { tap, samples, captured_at }
                            }
                        };
                        if tx.send(mapped).await.is_err() {
                            break;
                        }
                    }
                    evt = events.recv() => {
                        match evt {
                            Ok(trackaudio::Event::RxBegin(rx)) => {
                                let tap = tap_for(&radio, rx.frequency);
                                log::trace!(
                                    "RxBegin callsign={} freq={:?} -> {tap:?}",
                                    rx.callsign,
                                    rx.frequency,
                                );

                                active_rx.insert(
                                    Transmitter {
                                        callsign: rx.callsign.clone(),
                                        frequency: rx.frequency,
                                    },
                                    tap,
                                );
                                let _ = tx.send(ReplaySourceEvent::RxBegin {
                                    tap,
                                    callsign: rx.callsign,
                                    frequency: rx.frequency,
                                }).await;
                            }
                            Ok(trackaudio::Event::RxEnd(rx)) => {
                                // Always emit on the tap RxBegin used; falls back to
                                // current routing if we somehow missed the begin.
                                let tap = active_rx
                                    .remove(&Transmitter {
                                        callsign: rx.callsign.clone(),
                                        frequency: rx.frequency,
                                    })
                                    .unwrap_or_else(|| tap_for(&radio, rx.frequency));
                                log::trace!(
                                    "RxEnd callsign={} freq={:?} -> {tap:?} active={:?}",
                                    rx.callsign,
                                    rx.frequency,
                                    rx.active_transmitters
                                );
                                let _ = tx.send(ReplaySourceEvent::RxEnd {
                                    callsign: rx.callsign,
                                    frequency: rx.frequency,
                                    active_transmitters: rx.active_transmitters,
                                }).await;
                            }
                            Ok(trackaudio::Event::StationStateUpdate(state)) => {
                                let (Some(frequency), Some(headset)) =
                                    (state.frequency, state.headset)
                                else {
                                    continue;
                                };
                                let new_tap = if headset { TapId::Headset } else { TapId::Speaker };

                                // Find every active receiver on this frequency whose
                                // tap differs from the new routing and migrate it.
                                let migrate: Vec<(String, TapId)> = active_rx
                                    .iter()
                                    .filter(|(t, old_tap)| {
                                        t.frequency == frequency && **old_tap != new_tap
                                    })
                                    .map(|(t, old_tap)| (t.callsign.clone(), *old_tap))
                                    .collect();
                                for (callsign, old_tap) in migrate {
                                    log::debug!(
                                        "routing changed mid-RX: {callsign} on {:?} {old_tap:?} -> {new_tap:?}",
                                        frequency,
                                    );
                                    let _ = tx.send(ReplaySourceEvent::RxEnd {
                                        callsign: callsign.clone(),
                                        frequency,
                                        active_transmitters: None,
                                    }).await;
                                    let _ = tx.send(ReplaySourceEvent::RxBegin {
                                        tap: new_tap,
                                        callsign: callsign.clone(),
                                        frequency,
                                    }).await;
                                    active_rx.insert(
                                        Transmitter { callsign, frequency },
                                        new_tap,
                                    );
                                }
                            }
                            Ok(_) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                log::warn!("lagged {n} TrackAudio events");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                log::warn!("TrackAudio event broadcast closed");
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.capture = Some(capture);
        Ok(rx)
    }

    async fn stop(&mut self) {
        self.cancel.cancel();
        if let Some(mut capture) = self.capture.take() {
            capture.stop();
        }
    }
}

fn tap_for(radio: &TrackAudioRadio, frequency: trackaudio::Frequency) -> TapId {
    match radio.headset_for_frequency(frequency) {
        Some(true) => TapId::Headset,
        Some(false) => TapId::Speaker,
        // Station unknown: fall back to headset which is afv-native's default routing.
        None => TapId::Headset,
    }
}
