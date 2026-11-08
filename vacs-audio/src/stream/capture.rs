use crate::backend::AudioStream;
use crate::device::{DeviceType, StreamDevice};
use crate::dsp::{MicProcessor, downmix_interleaved_to_mono};
use crate::error::AudioError;
use crate::{EncodedAudioFrame, FRAME_SIZE, TARGET_SAMPLE_RATE};
use anyhow::Context;
use bytes::Bytes;
use parking_lot::lock_api::Mutex;
use ringbuf::HeapRb;
use ringbuf::consumer::Consumer;
use ringbuf::producer::Producer;
use ringbuf::traits::Split;
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Indexing, Resampler};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

const MAX_OPUS_FRAME_SIZE: usize = 1275; // max size of an Opus frame according to RFC 6716 3.2.1.
const MIN_INPUT_BUFFER_SIZE: usize = 4096;
const RESAMPLER_BUFFER_WAIT: Duration = Duration::from_micros(500);

const INPUT_VOLUME_OPS_CAPACITY: usize = 16;
const INPUT_VOLUME_OPS_PER_DATA_CALLBACK: usize = 16;
const ENCODED_AUDIO_FRAME_BUFFER_SIZE: usize = 512;
const CAPTURE_STATS_LOG_INTERVAL_FRAMES: u64 = 500;
const SILENT_FRAME_PEAK: f32 = 1e-4;
/// About -30 dBFS: quieter input is room noise the gate is meant to remove.
const SPEECH_FRAME_PEAK: f32 = 0.0316;

type InputVolumeOp = Box<dyn Fn(&mut f32) + Send>;

pub struct CaptureStream {
    _stream: Box<dyn AudioStream>,
    volume_ops: parking_lot::Mutex<ringbuf::HeapProd<InputVolumeOp>>,
    muted: Arc<AtomicBool>,
    cancel: Option<CancellationToken>,
    task: Option<JoinHandle<()>>,
    is_level_meter: bool,
    tx: broadcast::Sender<EncodedAudioFrame>,
}

impl CaptureStream {
    #[instrument(level = "debug", skip(error_tx), err)]
    pub fn start(
        device: StreamDevice,
        mut volume: f32,
        amp: f32,
        error_tx: mpsc::Sender<AudioError>,
        muted: bool,
    ) -> Result<Self, AudioError> {
        debug_assert!(matches!(device.device_type, DeviceType::Input));

        let muted = Arc::new(AtomicBool::new(muted));
        let muted_clone = muted.clone();

        // buffer for ~100ms of input data
        let (mut input_prod, mut input_cons) =
            HeapRb::<f32>::new(((device.sample_rate() / 10) as usize).max(MIN_INPUT_BUFFER_SIZE))
                .split();

        let mut mono_buf: Vec<f32> = Vec::with_capacity(MIN_INPUT_BUFFER_SIZE);

        let stream = device.build_input_stream(
            Box::new(move |input: &[f32]| {
                // downmix to mono if necessary
                let mono: &[f32] = if device.config.channels > 1 {
                    downmix_interleaved_to_mono(
                        input,
                        device.config.channels as usize,
                        &mut mono_buf,
                    );
                    &mono_buf
                } else {
                    input
                };

                let muted = muted_clone.load(Ordering::Relaxed);
                let mut overflows = 0usize;
                for &sample in mono {
                    // apply muting and push into input buffer to audio processing
                    if input_prod
                        .try_push(if muted { 0.0f32 } else { sample })
                        .is_err()
                    {
                        overflows += 1;
                        if overflows % 100 == 1 {
                            tracing::trace!(
                                ?overflows,
                                "Input buffer overflow (tail samples dropped)"
                            );
                        }
                    }
                }
                if overflows > 0 {
                    tracing::warn!(?overflows, "Dropped input samples during this callback");
                }
            }),
            Box::new(move |err| {
                tracing::error!(?err, "CPAL capture stream error");
                if let Err(err) = error_tx.try_send(err) {
                    tracing::warn!(?err, "Failed to send capture stream error");
                }
            }),
        )?;

        tracing::debug!("Starting capture on input stream");
        stream.play()?;

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.child_token();

        let (ops_prod, mut ops_cons) =
            HeapRb::<InputVolumeOp>::new(INPUT_VOLUME_OPS_CAPACITY).split();

        let mut resampler = device.resampler()?;

        let (tx, _) = broadcast::channel(ENCODED_AUDIO_FRAME_BUFFER_SIZE);
        let mut opus_framer = OpusFramer::new(tx.clone(), muted.clone())?;

        let task = tokio::runtime::Handle::current().spawn_blocking(move || {
            tracing::trace!("Input capture stream task started");

            let mut resampler_in_buf = vec![Vec::<f32>::with_capacity(FRAME_SIZE * 2)];
            let mut resampler_out_buf = vec![Vec::<f32>::with_capacity(FRAME_SIZE * 2)];

            // Pre-allocate output buffer to max size to avoid repeated allocations
            if let Some(resampler) = &resampler {
                let max_out = resampler.output_frames_max();
                resampler_out_buf[0].resize(max_out, 0.0f32);
            }

            // Reusable indexing struct to avoid repeated stack allocations
            let mut indexing = Indexing {
                input_offset: 0,
                output_offset: 0,
                active_channels_mask: None,
                partial_len: None,
            };

            while !cancel_clone.is_cancelled() {
                // apply any queued volume ops
                for _ in 0..INPUT_VOLUME_OPS_PER_DATA_CALLBACK {
                    if let Some(op) = ops_cons.try_pop() {
                        op(&mut volume);
                    } else {
                        break;
                    }
                }

                let gain = amp * volume;

                if let Some(resampler) = &mut resampler {
                    // buffer input data until we've reached enough to resample into the next frame
                    let need = resampler.input_frames_next();
                    while resampler_in_buf[0].len() < need {
                        if cancel_clone.is_cancelled() {
                            tracing::trace!("Input capture stream task cancelled");
                            break;
                        }
                        if let Some(sample) = input_cons.try_pop() {
                            resampler_in_buf[0].push(sample);
                        } else {
                            std::thread::sleep(RESAMPLER_BUFFER_WAIT);
                        }
                    }

                    if cancel_clone.is_cancelled() {
                        tracing::trace!("Input capture stream task cancelled");
                        break;
                    }
                    if resampler_in_buf[0].len() < need {
                        // canceled while waiting; exit
                        tracing::trace!("Did not receive enough input data to resample");
                        break;
                    }

                    // Create adapters
                    let input_frames = resampler_in_buf[0].len();
                    let max_out = resampler_out_buf[0].len();
                    let input_adapter =
                        SequentialSliceOfVecs::new(&resampler_in_buf, 1, input_frames).unwrap();
                    let mut output_adapter =
                        SequentialSliceOfVecs::new_mut(&mut resampler_out_buf, 1, max_out).unwrap();

                    // Reset indexing offsets (reuse same struct)
                    indexing.input_offset = 0;
                    indexing.output_offset = 0;

                    // resample the input data
                    let (_frames_in, frames_out) = match resampler.process_into_buffer(
                        &input_adapter,
                        &mut output_adapter,
                        Some(&indexing),
                    ) {
                        Ok(result) => result,
                        Err(err) => {
                            tracing::warn!(?err, "Failed to resample input");
                            continue;
                        }
                    };

                    resampler_in_buf[0].clear();

                    opus_framer.push_slice(&resampler_out_buf[0][..frames_out], gain);
                } else {
                    let mut stash: [f32; 1024] = [0.0; 1024];
                    let mut n = 0usize;

                    while let Some(sample) = input_cons.try_pop() {
                        if n == stash.len() {
                            opus_framer.push_slice(&stash[..n], gain);
                            n = 0;
                        }
                        stash[n] = sample;
                        n += 1;
                    }
                    if n > 0 {
                        opus_framer.push_slice(&stash[..n], gain);
                    } else {
                        std::thread::sleep(RESAMPLER_BUFFER_WAIT);
                    }
                }
            }

            opus_framer.log_partial_stats();
            tracing::trace!("Input capture stream task completed");
        });

        tracing::info!(
            muted = muted.load(Ordering::Relaxed),
            "Input capture stream started"
        );
        Ok(Self {
            _stream: stream,
            volume_ops: Mutex::new(ops_prod),
            muted,
            cancel: Some(cancel),
            task: Some(task),
            is_level_meter: false,
            tx,
        })
    }

    #[instrument(level = "debug", skip(emit, error_tx), err)]
    pub fn start_level_meter(
        device: StreamDevice,
        emit: Box<dyn Fn(InputLevel) + Send>,
        mut volume: f32,
        amp: f32,
        error_tx: mpsc::Sender<AudioError>,
    ) -> Result<Self, AudioError> {
        let mut level_meter = InputLevelMeter::new(device.sample_rate() as f32);

        let (ops_prod, mut ops_cons) =
            HeapRb::<InputVolumeOp>::new(INPUT_VOLUME_OPS_CAPACITY).split();

        let stream = device.build_input_stream(
            Box::new(move |input: &[f32]| {
                for _ in 0..INPUT_VOLUME_OPS_PER_DATA_CALLBACK {
                    if let Some(op) = ops_cons.try_pop() {
                        op(&mut volume);
                    } else {
                        break;
                    }
                }

                let gain = amp * volume;
                for &sample in input {
                    if let Some(level) = level_meter.push_sample(sample * gain) {
                        emit(level);
                    }
                }
            }),
            Box::new(move |err| {
                tracing::error!(?err, "CPAL capture stream level meter error");
                if let Err(err) = error_tx.try_send(err) {
                    tracing::warn!(?err, "Failed to send capture stream level meter error");
                }
            }),
        )?;

        stream.play()?;

        let (tx, _) = broadcast::channel(ENCODED_AUDIO_FRAME_BUFFER_SIZE);

        tracing::debug!("Input level meter capture stream started");
        Ok(Self {
            _stream: stream,
            volume_ops: Mutex::new(ops_prod),
            muted: Arc::new(AtomicBool::new(false)),
            cancel: None,
            task: None,
            is_level_meter: true,
            tx,
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn stop(mut self) {
        tracing::info!("Stopping input capture stream");
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        let task = self.task.take();
        // Drops the cpal stream before the processing task is joined.
        drop(self);
        if let Some(task) = task
            && let Err(err) = task.await
        {
            tracing::warn!(?err, "Input capture stream task failed");
        }
    }

    pub fn set_muted(&self, muted: bool) {
        if self.muted.swap(muted, Ordering::Relaxed) != muted && !self.is_level_meter {
            tracing::debug!(muted, "Input capture mute changed");
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: f32) {
        if self
            .volume_ops
            .lock()
            .try_push(Box::new(move |vol| *vol = volume.min(1.0)))
            .is_err()
        {
            tracing::warn!("Failed to queue volume op");
        }
    }

    pub fn is_level_meter(&self) -> bool {
        self.is_level_meter
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EncodedAudioFrame> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Drop for CaptureStream {
    fn drop(&mut self) {
        // The blocking processing task only exits via this token, and dropping
        // a CancellationToken does not cancel it. stop() takes the token first.
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}

struct OpusFramer {
    frame: [f32; FRAME_SIZE],
    pos: usize,
    processor: MicProcessor,
    encoder: opus::Encoder,
    encoded: Vec<u8>,
    tx: broadcast::Sender<EncodedAudioFrame>,
    muted: Arc<AtomicBool>,
    stats: CaptureStats,
}

impl OpusFramer {
    fn new(
        tx: broadcast::Sender<EncodedAudioFrame>,
        muted: Arc<AtomicBool>,
    ) -> Result<Self, AudioError> {
        let mut encoder = opus::Encoder::new(
            TARGET_SAMPLE_RATE,
            opus::Channels::Mono,
            opus::Application::Voip,
        )
        .context("Failed to create opus encoder")?;
        encoder
            .set_bitrate(opus::Bitrate::Max)
            .context("Failed to set opus bitrate")?;
        encoder
            .set_inband_fec(true)
            .context("Failed to set opus inband fec")?;
        encoder.set_vbr(false).context("Failed to set opus vbr")?;

        Ok(Self {
            frame: [0.0f32; FRAME_SIZE],
            pos: 0usize,
            processor: MicProcessor::default(),
            encoder,
            encoded: vec![0u8; MAX_OPUS_FRAME_SIZE],
            tx,
            muted,
            stats: CaptureStats::default(),
        })
    }

    /// Logs the counters of an interval cut short by the end of the stream, so a
    /// device that stops delivering still leaves a last line behind.
    fn log_partial_stats(&mut self) {
        if let Some(report) = self.stats.take_partial() {
            report.log(true);
        }
    }

    #[inline]
    fn push_slice(&mut self, mut samples: &[f32], gain: f32) {
        while !samples.is_empty() {
            let need = FRAME_SIZE - self.pos;
            let take = need.min(samples.len());

            for (i, sample) in samples.iter().enumerate().take(take) {
                self.frame[self.pos + i] = sample * gain;
            }
            self.pos += take;
            samples = &samples[take..];

            if self.pos == FRAME_SIZE {
                let input_peak = frame_peak(&self.frame);
                self.processor.process_frame(&mut self.frame);
                self.stats.gain = gain;
                self.stats.record(
                    self.muted.load(Ordering::Relaxed),
                    input_peak,
                    frame_peak(&self.frame),
                );
                if let Some(report) = self.stats.take_report() {
                    report.log(false);
                }

                match self.encoder.encode_float(&self.frame, &mut self.encoded) {
                    Ok(len) => {
                        let bytes = Bytes::copy_from_slice(&self.encoded[..len]);
                        if self.tx.receiver_count() > 0
                            && let Err(err) = self.tx.send(bytes)
                        {
                            tracing::warn!(?err, "Failed to send encoded input audio frame");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(?err, "Failed to encode input audio frame");
                    }
                }

                self.pos = 0;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputLevel {
    pub dbfs_rms: f32,  // e.g. -23.4
    pub dbfs_peak: f32, // e.g. -1.2
    pub norm: f32,      // 0..1, for display purposes
    pub clipping: bool,
}

pub struct InputLevelMeter {
    window_samples: usize, // ~10-20ms worth of samples
    sum_sq: f64,
    peak: f32,
    count: usize,
    last_emit: Instant,
    emit_interval: Duration, // e.g. 16ms => ~60fps
    // smoothing (EMA in dB)
    ema_db: f32,
    attack: f32,  // 0..1, (higher = faster rise)
    release: f32, // 0..1, (lower = faster fall)
}

const INPUT_LEVEL_METER_WINDOW_MS: f32 = 15.0;

const INPUT_LEVEL_MIN_DB: f32 = -60.0;
const INPUT_LEVEL_MAX_DB: f32 = 0.0;

impl InputLevelMeter {
    pub fn new(sample_rate: f32) -> Self {
        let window_samples = (sample_rate * (INPUT_LEVEL_METER_WINDOW_MS / 1000.0)) as usize;

        Self {
            window_samples: window_samples.max(1),
            sum_sq: 0.0,
            peak: 0.0,
            count: 0,
            last_emit: Instant::now(),
            emit_interval: Duration::from_millis(16),
            ema_db: -90.0,
            attack: 0.5,
            release: 0.1,
        }
    }

    pub fn push_sample(&mut self, s: f32) -> Option<InputLevel> {
        let a = s.abs();
        self.peak = self.peak.max(a);
        self.sum_sq += (s as f64) * (s as f64);
        self.count += 1;

        if self.count >= self.window_samples && self.last_emit.elapsed() >= self.emit_interval {
            let rms = (self.sum_sq / (self.count as f64)).sqrt() as f32;
            let dbfs_rms = if rms > 0.0 { 20.0 * rms.log10() } else { -90.0 };
            let dbfs_peak = if self.peak > 0.0 {
                20.0 * self.peak.log10()
            } else {
                -90.0
            };

            let alpha = if dbfs_rms > self.ema_db {
                self.attack
            } else {
                self.release
            };
            self.ema_db = self.ema_db + alpha * (dbfs_rms - self.ema_db);

            let mut norm =
                (self.ema_db - INPUT_LEVEL_MIN_DB) / (INPUT_LEVEL_MAX_DB - INPUT_LEVEL_MIN_DB);
            norm = norm.clamp(0.0, 1.0);

            let clipping = self.peak >= 0.999;

            let out = InputLevel {
                dbfs_rms,
                dbfs_peak,
                norm,
                clipping,
            };

            self.sum_sq = 0.0;
            self.peak = 0.0;
            self.count = 0;
            self.last_emit = Instant::now();

            return Some(out);
        }
        None
    }
}

/// Peak magnitude of a frame, or `None` if it holds a NaN or infinite sample.
fn frame_peak(frame: &[f32]) -> Option<f32> {
    let mut peak = 0.0f32;
    for &s in frame {
        if !s.is_finite() {
            return None;
        }
        peak = peak.max(s.abs());
    }
    Some(peak)
}

fn peak_dbfs(peak: f32) -> f32 {
    if peak > 0.0 {
        (200.0 * peak.log10()).round() / 10.0
    } else {
        -120.0
    }
}

/// Per-interval counters over the frames handed to the encoder. Frames are
/// counted, never the samples inside the device callback.
#[derive(Debug, Default, Clone, PartialEq)]
struct CaptureStats {
    frames: u64,
    muted_frames: u64,
    /// Unmuted frames that were exactly zero before processing.
    zero_frames: u64,
    /// Speech-level input that processing (noise gate, limiter) turned silent.
    gated_frames: u64,
    non_finite_input_frames: u64,
    non_finite_output_frames: u64,
    peak_input: f32,
    peak_output: f32,
    /// Input volume times amp of the latest frame; the peaks are measured after it.
    gain: f32,
    warned_non_finite: bool,
}

impl CaptureStats {
    fn record(&mut self, muted: bool, input_peak: Option<f32>, output_peak: Option<f32>) {
        self.frames += 1;
        if muted {
            self.muted_frames += 1;
        }

        match input_peak {
            Some(peak) => {
                self.peak_input = self.peak_input.max(peak);
                if peak == 0.0 && !muted {
                    self.zero_frames += 1;
                }
            }
            None => self.non_finite_input_frames += 1,
        }

        match output_peak {
            Some(peak) => {
                self.peak_output = self.peak_output.max(peak);
                if peak < SILENT_FRAME_PEAK && input_peak.is_some_and(|p| p >= SPEECH_FRAME_PEAK) {
                    self.gated_frames += 1;
                }
            }
            None => self.non_finite_output_frames += 1,
        }

        if !self.warned_non_finite && (input_peak.is_none() || output_peak.is_none()) {
            self.warned_non_finite = true;
            tracing::warn!(
                input_finite = input_peak.is_some(),
                output_finite = output_peak.is_some(),
                "Non-finite samples in captured audio"
            );
        }
    }

    /// Returns the counters once an interval is complete and starts the next one.
    fn take_report(&mut self) -> Option<Self> {
        if self.frames < CAPTURE_STATS_LOG_INTERVAL_FRAMES {
            return None;
        }
        Some(self.reset())
    }

    fn take_partial(&mut self) -> Option<Self> {
        (self.frames > 0).then(|| self.reset())
    }

    fn reset(&mut self) -> Self {
        let warned_non_finite = self.warned_non_finite;
        let report = std::mem::take(self);
        self.warned_non_finite = warned_non_finite;
        report
    }

    fn log(&self, partial: bool) {
        tracing::debug!(
            partial,
            frames = self.frames,
            muted_frames = self.muted_frames,
            zero_frames = self.zero_frames,
            gated_frames = self.gated_frames,
            non_finite_input_frames = self.non_finite_input_frames,
            non_finite_output_frames = self.non_finite_output_frames,
            peak_input_dbfs = peak_dbfs(self.peak_input),
            peak_output_dbfs = peak_dbfs(self.peak_output),
            gain = self.gain,
            "Capture stats"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_peak_rejects_non_finite_samples() {
        assert_eq!(frame_peak(&[0.1, -0.5, 0.2]), Some(0.5));
        assert_eq!(frame_peak(&[0.1, f32::NAN]), None);
        assert_eq!(frame_peak(&[f32::INFINITY]), None);
        assert_eq!(frame_peak(&[0.0; 4]), Some(0.0));
    }

    #[test]
    fn stats_separate_muted_zero_and_gated_frames() {
        let mut stats = CaptureStats::default();
        stats.record(true, Some(0.0), Some(0.0));
        stats.record(false, Some(0.0), Some(0.0));
        stats.record(false, Some(0.01), Some(0.0));
        stats.record(false, Some(0.1), Some(0.0));
        stats.record(false, Some(0.5), Some(0.4));

        assert_eq!(stats.frames, 5);
        assert_eq!(stats.muted_frames, 1);
        assert_eq!(stats.zero_frames, 1);
        assert_eq!(
            stats.gated_frames, 1,
            "only speech-level input silenced by processing counts as gated"
        );
        assert_eq!(stats.peak_input, 0.5);
        assert_eq!(stats.peak_output, 0.4);
    }

    #[test]
    fn stats_count_non_finite_frames_per_stage() {
        let mut stats = CaptureStats::default();
        stats.record(false, Some(0.2), None);
        stats.record(false, None, None);

        assert_eq!(stats.non_finite_input_frames, 1);
        assert_eq!(stats.non_finite_output_frames, 2);
        assert!(stats.warned_non_finite);
    }

    #[test]
    fn report_is_taken_once_per_interval_and_resets_counters() {
        let mut stats = CaptureStats::default();
        stats.record(false, None, None);
        for _ in 1..CAPTURE_STATS_LOG_INTERVAL_FRAMES - 1 {
            stats.record(false, Some(0.1), Some(0.1));
            assert!(stats.take_report().is_none());
        }
        stats.record(false, Some(0.1), Some(0.1));

        let report = stats.take_report().expect("interval complete");
        assert_eq!(report.frames, CAPTURE_STATS_LOG_INTERVAL_FRAMES);
        assert_eq!(report.non_finite_input_frames, 1);

        assert_eq!(stats.frames, 0);
        assert_eq!(stats.peak_input, 0.0);
        assert!(
            stats.warned_non_finite,
            "the non-finite warning stays once per stream"
        );
    }

    #[test]
    fn partial_report_covers_an_unfinished_interval_once() {
        let mut stats = CaptureStats::default();
        assert!(stats.take_partial().is_none());

        stats.gain = 2.0;
        stats.record(false, Some(0.1), Some(0.1));
        stats.record(false, Some(0.1), Some(0.1));

        let report = stats.take_partial().expect("frames recorded");
        assert_eq!(report.frames, 2);
        assert_eq!(report.gain, 2.0);
        assert!(stats.take_partial().is_none());
    }

    #[test]
    fn peak_dbfs_rounds_and_floors_silence() {
        assert_eq!(peak_dbfs(1.0), 0.0);
        assert_eq!(peak_dbfs(0.5), -6.0);
        assert_eq!(peak_dbfs(0.0), -120.0);
    }
}
