use crate::sources::AudioSource;
use crate::{EncodedAudioFrame, FRAME_SIZE, TARGET_SAMPLE_RATE};
use anyhow::{Context, Result};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Async, Indexing, Resampler};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{Instrument, instrument};

const RESAMPLER_BUFFER_SIZE: usize = 8192;

pub struct OpusSource {
    cons: HeapCons<f32>,
    decoder_task: JoinHandle<()>,
    output_channels: u16, // >= 1
    volume: f32,          // 0.0 - 1.0
    amp: f32,             // >= 0.1
}

impl OpusSource {
    #[instrument(level = "debug", skip(rx, resampler), err)]
    pub fn new(
        mut rx: mpsc::Receiver<EncodedAudioFrame>,
        mut resampler: Option<Async<f32>>,
        output_channels: u16,
        volume: f32,
        amp: f32,
    ) -> Result<Self> {
        tracing::trace!("Creating Opus source");

        // We buffer 10 frames, which equals a total buffer of 200 ms at 48_000 Hz and 20 ms intervals
        let (mut prod, cons): (HeapProd<f32>, HeapCons<f32>) = HeapRb::new(FRAME_SIZE * 10).split();

        // Our captured input audio will always be in mono and is transmitted via a webrtc mono stream,
        // so we can safely default to a mono Opus decoder here. Interleaving to stereo output devices
        // is handled by `AudioSource` implementation.
        let mut decoder = opus::Decoder::new(TARGET_SAMPLE_RATE, opus::Channels::Mono)
            .context("Failed to create Opus decoder")?;

        let decoder_task = tokio::runtime::Handle::current().spawn(
            async move {
                tracing::debug!("Starting Opus decoder task");

                let mut decoded = vec![0.0f32; FRAME_SIZE];
                let mut buf = Vec::<f32>::with_capacity(RESAMPLER_BUFFER_SIZE);
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

                let mut overflows = 0usize;

                while let Some(frame) = rx.recv().await {
                    match decoder.decode_float(&frame, &mut decoded, false) {
                        Ok(n) => {
                            let samples = if let Some(resampler) = &mut resampler {
                                let need = resampler.input_frames_next();

                                buf.extend_from_slice(&decoded[..n]);

                                if buf.len() < need {
                                    continue;
                                }

                                resampler_in_buf[0].clear();
                                resampler_in_buf[0].extend_from_slice(&buf[..need]);
                                buf.drain(..need);

                                // Create adapters
                                let input_frames = resampler_in_buf[0].len();
                                let max_out = resampler_out_buf[0].len();
                                let input_adapter =
                                    SequentialSliceOfVecs::new(&resampler_in_buf, 1, input_frames)
                                        .unwrap();
                                let mut output_adapter = SequentialSliceOfVecs::new_mut(
                                    &mut resampler_out_buf,
                                    1,
                                    max_out,
                                )
                                .unwrap();

                                // Reset indexing offsets (reuse same struct)
                                indexing.input_offset = 0;
                                indexing.output_offset = 0;

                                // resample opus data
                                let (_frames_in, frames_out) = match resampler.process_into_buffer(
                                    &input_adapter,
                                    &mut output_adapter,
                                    Some(&indexing),
                                ) {
                                    Ok(result) => result,
                                    Err(err) => {
                                        tracing::warn!(?err, "Failed to resample opus data");
                                        continue;
                                    }
                                };

                                &resampler_out_buf[0][..frames_out]
                            } else {
                                &decoded[..n]
                            };

                            let written = prod.push_slice(samples);
                            if written < samples.len() {
                                overflows += 1;
                                if overflows % 100 == 1 {
                                    tracing::debug!(
                                        ?written,
                                        needed = ?samples.len(),
                                        ?overflows,
                                        "Opus ring overflow (tail samples dropped)"
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!(?err, "Failed to decode Opus frame");
                        }
                    }
                }

                tracing::debug!("Opus decoder task ended");
            }
            .instrument(tracing::Span::current()),
        );

        Ok(Self {
            cons,
            decoder_task,
            output_channels: output_channels.max(1),
            volume: volume.clamp(0.0, 1.0),
            amp: amp.max(0.1),
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn stop(self) {
        tracing::trace!("Aborting Opus decoder task");
        self.decoder_task.abort();
    }
}

impl AudioSource for OpusSource {
    fn mix_into(&mut self, output: &mut [f32]) {
        // Only a single output channel --> no interleaving required, just copy samples
        if self.output_channels == 1 {
            for (out_s, s) in output.iter_mut().zip(self.cons.pop_iter()) {
                *out_s += s * self.amp * self.volume;
            }

            // Do not backfill tail samples, as output buffer is already initialized with EQUILIBRIUM
            // and other AudioSources might have already added their samples to the buffer.
            return;
        }

        // Interleaved multi-channel: duplicate mono sample across channels
        // Limit by frames so we don’t overrun the output
        for (frame, s) in output
            .chunks_mut(self.output_channels as usize)
            .zip(self.cons.pop_iter())
        {
            for x in frame {
                *x += s * self.amp * self.volume;
            }
        }
    }

    fn start(&mut self) {
        // Nothing to do here, the webrtc source must start webrtc stream used as opus input data
    }

    fn stop(&mut self) {
        // Nothing to do here, the webrtc source must stop webrtc stream used as opus input data
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }
}
