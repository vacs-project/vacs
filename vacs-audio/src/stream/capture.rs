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
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

const MAX_OPUS_FRAME_SIZE: usize = 1275; // max size of an Opus frame according to RFC 6716 3.2.1.
const MIN_INPUT_BUFFER_SIZE: usize = 4096;
const RESAMPLER_BUFFER_WAIT: Duration = Duration::from_micros(500);

const INPUT_VOLUME_OPS_CAPACITY: usize = 16;
const INPUT_VOLUME_OPS_PER_DATA_CALLBACK: usize = 16;

type InputVolumeOp = Box<dyn Fn(&mut f32) + Send>;

pub struct CaptureStream {
    _stream: Box<dyn AudioStream>,
    volume_ops: parking_lot::Mutex<ringbuf::HeapProd<InputVolumeOp>>,
    muted: Arc<AtomicBool>,
    cancel: Option<CancellationToken>,
    task: Option<JoinHandle<()>>,
    is_level_meter: bool,
}

impl CaptureStream {
    #[instrument(level = "debug", skip(tx, error_tx), err)]
    pub fn start(
        device: StreamDevice,
        tx: mpsc::Sender<EncodedAudioFrame>,
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

        let mut opus_framer = OpusFramer::new(tx)?;

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

            tracing::trace!("Input capture stream task completed");
        });

        tracing::info!("Input capture stream started");
        Ok(Self {
            _stream: stream,
            volume_ops: Mutex::new(ops_prod),
            muted,
            cancel: Some(cancel),
            task: Some(task),
            is_level_meter: false,
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

        tracing::debug!("Input level meter capture stream started");
        Ok(Self {
            _stream: stream,
            volume_ops: Mutex::new(ops_prod),
            muted: Arc::new(AtomicBool::new(false)),
            cancel: None,
            task: None,
            is_level_meter: true,
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn stop(mut self) {
        tracing::info!("Stopping input capture stream");
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        drop(self._stream);
        if let Some(task) = self.task.take()
            && let Err(err) = task.await
        {
            tracing::warn!(?err, "Input capture stream task failed");
        }
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
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
}

struct OpusFramer {
    frame: [f32; FRAME_SIZE],
    pos: usize,
    processor: MicProcessor,
    encoder: opus::Encoder,
    encoded: Vec<u8>,
    tx: mpsc::Sender<EncodedAudioFrame>,
}

impl OpusFramer {
    fn new(tx: mpsc::Sender<EncodedAudioFrame>) -> Result<Self, AudioError> {
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
        })
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
                self.processor.process_frame(&mut self.frame);

                match self.encoder.encode_float(&self.frame, &mut self.encoded) {
                    Ok(len) => {
                        let bytes = Bytes::copy_from_slice(&self.encoded[..len]);
                        if let Err(err) = self.tx.try_send(bytes) {
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
