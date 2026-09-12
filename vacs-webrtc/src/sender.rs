use anyhow::{Context, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{Instrument, instrument};
use vacs_audio::{EncodedAudioFrame, FRAME_DURATION_MS};
use webrtc::media::Sample;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// Bound on joining the sender task: a write stuck on a dead transport must not
/// hold up the peer close, and with it the input device release.
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Sender {
    shutdown_tx: watch::Sender<()>,
    task: JoinHandle<()>,
}

impl Sender {
    #[instrument(level = "trace", skip_all)]
    pub fn new(
        track: Arc<TrackLocalStaticSample>,
        mut input_rx: broadcast::Receiver<EncodedAudioFrame>,
        sent_frames: Arc<AtomicU64>,
    ) -> Self {
        let (shutdown_tx, mut shutdown_rx) = watch::channel(());

        let task = tokio::runtime::Handle::current().spawn(async move {
            // Frames lost to lag or failed writes are reported on the next
            // sample so the RTP timeline advances past the gap instead of
            // compressing it.
            let mut dropped_frames: u64 = 0;
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => {
                        tracing::trace!("Shutdown signalled, stopping sending");
                        break;
                    }
                    frame = input_rx.recv() => {
                        match frame {
                            Ok(frame) => {
                                let sample = Sample {
                                    data: frame,
                                    duration: std::time::Duration::from_millis(FRAME_DURATION_MS),
                                    prev_dropped_packets: u16::try_from(dropped_frames)
                                        .unwrap_or(u16::MAX),
                                    ..Default::default()
                                };

                                if let Err(err) = track.write_sample(&sample).await {
                                    tracing::warn!(?err, "Failed to write sample to track");
                                    dropped_frames = dropped_frames.saturating_add(1);
                                } else {
                                    sent_frames.fetch_add(1, Ordering::Relaxed);
                                    dropped_frames = 0;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) =>{
                                tracing::warn!(?skipped, "Input receiver lagged");
                                dropped_frames = dropped_frames.saturating_add(skipped);
                            },
                            Err(_) => {
                                break;
                            }
                        }
                    }
                }
            }
        }.instrument(tracing::Span::current()));

        Self { shutdown_tx, task }
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    #[instrument(level = "trace", skip(self), err)]
    pub async fn stop(mut self) -> Result<()> {
        self.shutdown();
        tracing::trace!("Waiting for sender task to finish");
        match tokio::time::timeout(STOP_TIMEOUT, &mut self.task).await {
            Ok(joined) => joined.context("Failed to join sender task"),
            Err(_) => {
                tracing::warn!("Sender task did not stop within {STOP_TIMEOUT:?}, aborting it");
                self.task.abort();
                // Joined so the input subscription is gone when this returns.
                let _ = (&mut self.task).await;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WEBRTC_CHANNELS, WEBRTC_TRACK_ID, WEBRTC_TRACK_STREAM_ID};
    use std::time::Duration;
    use test_log::test;
    use vacs_audio::TARGET_SAMPLE_RATE;
    use webrtc::api::media_engine::MIME_TYPE_OPUS;
    use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

    /// An unbound track has no packetizer, so `write_sample` is a no-op. That
    /// keeps these tests about the pump loop rather than about RTP output.
    fn test_track() -> Arc<TrackLocalStaticSample> {
        Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: TARGET_SAMPLE_RATE,
                channels: WEBRTC_CHANNELS,
                ..Default::default()
            },
            WEBRTC_TRACK_ID.to_owned(),
            WEBRTC_TRACK_STREAM_ID.to_owned(),
        ))
    }

    #[test(tokio::test)]
    async fn drains_input_frames() {
        let (input_tx, input_rx) = broadcast::channel(1);
        let sent_frames = Arc::new(AtomicU64::new(0));
        let sender = Sender::new(test_track(), input_rx, Arc::clone(&sent_frames));

        // Overflowing the single-slot channel makes the receiver lag; the task
        // must keep draining through the `Lagged` arm instead of exiting.
        for _ in 0..8 {
            input_tx
                .send(EncodedAudioFrame::from_static(&[0x01, 0x02, 0x03]))
                .expect("input channel closed unexpectedly");
        }

        tokio::time::timeout(Duration::from_secs(5), async {
            while sent_frames.load(Ordering::Relaxed) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("sender task stopped draining input frames");

        sender.stop().await.expect("failed to stop sender");

        assert!(
            sent_frames.load(Ordering::Relaxed) > 0,
            "drained frames must be counted for the media stats watchdog"
        );
    }

    #[test(tokio::test)]
    async fn stop_joins_task_while_input_stays_open() {
        let (_input_tx, input_rx) = broadcast::channel(1);
        let sender = Sender::new(test_track(), input_rx, Arc::new(AtomicU64::new(0)));

        tokio::time::timeout(Duration::from_secs(5), sender.stop())
            .await
            .expect("sender task ignored the shutdown signal")
            .expect("failed to stop sender");
    }

    /// Without the `Closed` arm the task would spin on a closed channel for the
    /// rest of the process lifetime instead of ending with the call.
    #[test(tokio::test)]
    async fn closed_input_ends_task() {
        let (input_tx, input_rx) = broadcast::channel::<EncodedAudioFrame>(1);
        let sender = Sender::new(test_track(), input_rx, Arc::new(AtomicU64::new(0)));

        drop(input_tx);

        tokio::time::timeout(Duration::from_secs(5), async {
            while !sender.task.is_finished() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("sender task outlived its closed input channel");
    }
}
