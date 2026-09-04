use crate::device::{DeviceType, StreamDevice};
use crate::error::AudioError;
use crate::mixer::Mixer;
use crate::sources::{AudioSource, AudioSourceId};
use cpal::traits::StreamTrait;
use parking_lot::Mutex;
use ringbuf::HeapRb;
use ringbuf::consumer::Consumer;
use ringbuf::producer::Producer;
use ringbuf::traits::Split;
use rubato::Async;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, atomic};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::instrument;

type MixerOp = Box<dyn FnOnce(&mut Mixer) + Send>;

const MIXER_OPS_CAPACITY: usize = 256;
const MIXER_OPS_PER_DATA_CALLBACK: usize = 32;

pub struct PlaybackStream {
    _stream: cpal::Stream,
    mixer_ops: Mutex<ringbuf::HeapProd<MixerOp>>,
    removed_sources: Mutex<ringbuf::HeapCons<Box<dyn AudioSource>>>,
    next_audio_source_id: atomic::AtomicUsize,
    deafened: Arc<AtomicBool>,
    device: StreamDevice,
}

impl PlaybackStream {
    #[instrument(level = "debug", skip(error_tx), err)]
    pub fn start(
        device: StreamDevice,
        error_tx: mpsc::Sender<AudioError>,
    ) -> Result<Self, AudioError> {
        debug_assert!(matches!(device.device_type, DeviceType::Output));

        let (removed_prod, removed_cons) =
            HeapRb::<Box<dyn AudioSource>>::new(MIXER_OPS_CAPACITY).split();
        let mut mixer = Mixer::with_deferred_drop(removed_prod);
        let (ops_prod, mut ops_cons) = HeapRb::<MixerOp>::new(MIXER_OPS_CAPACITY).split();

        let deafened = Arc::new(AtomicBool::new(false));
        let deafened_clone = deafened.clone();

        let stream = device.build_output_stream(
            move |output, _| {
                for _ in 0..MIXER_OPS_PER_DATA_CALLBACK {
                    if let Some(op) = ops_cons.try_pop() {
                        op(&mut mixer);
                    } else {
                        break;
                    }
                }
                mixer.mix(output);
            },
            move |err| {
                // Xruns are transient (samples dropped on a live stream);
                // restarting the stream for them would only drop more audio.
                if matches!(err.kind(), cpal::ErrorKind::Xrun) {
                    tracing::debug!("Playback stream xrun, samples dropped");
                    return;
                }
                tracing::error!(?err, "CPAL playback stream error");
                if let Err(err) = error_tx.try_send(err.into()) {
                    tracing::warn!(?err, "Failed to send playback stream error");
                }
            },
        )?;

        stream.play()?;

        Ok(Self {
            _stream: stream,
            mixer_ops: Mutex::new(ops_prod),
            removed_sources: Mutex::new(removed_cons),
            next_audio_source_id: atomic::AtomicUsize::new(0),
            deafened: deafened_clone,
            device,
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn stop(self) {
        tracing::info!("Stopping output playback stream");
        let Self {
            _stream,
            removed_sources,
            ..
        } = self;
        drop(_stream);

        let mut removed_sources = removed_sources.into_inner();
        while removed_sources.try_pop().is_some() {}
    }

    /// Frees sources the audio callback handed back; see [`Mixer::with_deferred_drop`].
    fn drain_removed_sources(&self) {
        let mut removed_sources = self.removed_sources.lock();
        while removed_sources.try_pop().is_some() {}
    }

    pub fn set_deafened(&self, muted: bool) {
        self.deafened.store(muted, Ordering::Relaxed);
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened.load(Ordering::Relaxed)
    }

    #[instrument(level = "trace", skip_all)]
    pub fn add_audio_source(&self, source: Box<dyn AudioSource>) -> AudioSourceId {
        self.drain_removed_sources();

        let id = self
            .next_audio_source_id
            .fetch_add(1, atomic::Ordering::SeqCst);

        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.add_source(id, source);
            }))
            .is_err()
        {
            tracing::warn!(?id, "Failed to add audio source to mixer");
        }

        id
    }

    #[instrument(level = "trace", skip(self))]
    pub fn remove_audio_source(&self, id: AudioSourceId) {
        self.drain_removed_sources();

        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| mixer.remove_source(id)))
            .is_err()
        {
            tracing::warn!("Failed to remove audio source from mixer");
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn start_audio_source(&self, id: AudioSourceId) {
        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.start_source(id);
            }))
            .is_err()
        {
            tracing::warn!("Failed to start audio source");
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn stop_audio_source(&self, id: AudioSourceId) {
        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.stop_source(id);
            }))
            .is_err()
        {
            tracing::warn!("Failed to stop audio source");
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn restart_audio_source(&self, id: AudioSourceId) {
        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.restart_source(id);
            }))
            .is_err()
        {
            tracing::warn!("Failed to restart audio source");
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn set_volume(&self, id: AudioSourceId, volume: f32) {
        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.set_source_volume(id, volume);
            }))
            .is_err()
        {
            tracing::warn!("Failed to set volume for audio source");
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn skip_in_audio_source(&self, id: AudioSourceId, duration: Duration) {
        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.skip_in_source(id, duration);
            }))
            .is_err()
        {
            tracing::warn!("Failed to skip duration for audio source");
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn rewind_in_audio_source(&self, id: AudioSourceId, duration: Duration) {
        if self
            .mixer_ops
            .lock()
            .try_push(Box::new(move |mixer: &mut Mixer| {
                mixer.rewind_in_source(id, duration);
            }))
            .is_err()
        {
            tracing::warn!("Failed to rewind duration for audio source");
        }
    }

    pub fn resampler(&self) -> Result<Option<Async<f32>>, AudioError> {
        self.device.resampler()
    }

    pub fn channels(&self) -> u16 {
        self.device.channels()
    }

    pub fn sample_rate(&self) -> u32 {
        self.device.sample_rate()
    }

    pub fn device_name(&self) -> String {
        self.device.name()
    }
}
