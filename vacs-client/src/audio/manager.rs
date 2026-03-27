use crate::app::state::AppState;
use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::webrtc::AppStateWebrtcExt;
use crate::config::AudioConfig;
use crate::error::{Error, FrontendError};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use vacs_audio::EncodedAudioFrame;
use vacs_audio::device::{DeviceSelector, DeviceType};
use vacs_audio::error::AudioError;
use vacs_audio::sources::AudioSourceId;
use vacs_audio::sources::opus::OpusSource;
use vacs_audio::sources::waveform::{Waveform, WaveformSource, WaveformTone};
use vacs_audio::stream::capture::{CaptureStream, InputLevel};
use vacs_audio::stream::playback::PlaybackStream;
use vacs_signaling::protocol::ws::shared;
use vacs_signaling::protocol::ws::shared::CallErrorReason;

const AUDIO_STREAM_ERROR_CHANNEL_SIZE: usize = 32;

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum SourceType {
    Opus,
    Ring,
    PriorityRing,
    Ringback,
    RingbackOneshot,
    Click,
    CallStart,
    CallEnd,
}

impl SourceType {
    fn into_waveform_source(
        self,
        sample_rate: f32,
        output_channels: usize,
        volume: f32,
    ) -> WaveformSource {
        match self {
            SourceType::Opus => {
                unimplemented!("Cannot create waveform source for Opus SourceType")
            }
            SourceType::Ring => WaveformSource::single(
                WaveformTone::new(497.0, Waveform::Triangle, 0.2),
                Duration::from_secs_f32(1.69),
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::PriorityRing => WaveformSource::new(
                [
                    (
                        WaveformTone::new(769.0, Waveform::Sine, 0.2),
                        Duration::from_millis(120),
                    ),
                    (
                        WaveformTone::new(628.0, Waveform::Triangle, 0.13),
                        Duration::from_millis(80),
                    ),
                    (
                        WaveformTone::new(492.0, Waveform::Triangle, 0.08),
                        Duration::from_millis(90),
                    ),
                ]
                .repeat(4),
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::Ringback => WaveformSource::single(
                WaveformTone::new(425.0, Waveform::Sine, 0.2),
                Duration::from_secs(1),
                Some(Duration::from_secs(4)),
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::RingbackOneshot => WaveformSource::single(
                WaveformTone::new(425.0, Waveform::Sine, 0.2),
                Duration::from_secs(1),
                None,
                Duration::from_millis(10),
                sample_rate,
                2,
                volume,
            ),
            SourceType::Click => WaveformSource::single(
                WaveformTone::new(4000.0, Waveform::Sine, 0.2),
                Duration::from_millis(20),
                None,
                Duration::from_millis(1),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::CallStart => WaveformSource::new(
                vec![
                    (
                        WaveformTone::new(600.0, Waveform::Sine, 0.2),
                        Duration::from_millis(100),
                    ),
                    (
                        WaveformTone::new(900.0, Waveform::Sine, 0.15),
                        Duration::from_millis(100),
                    ),
                ],
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
            SourceType::CallEnd => WaveformSource::new(
                vec![
                    (
                        WaveformTone::new(650.0, Waveform::Sine, 0.2),
                        Duration::from_millis(100),
                    ),
                    (
                        WaveformTone::new(450.0, Waveform::Sine, 0.15),
                        Duration::from_millis(100),
                    ),
                ],
                None,
                Duration::from_millis(10),
                sample_rate,
                output_channels,
                volume,
            ),
        }
    }
}

pub struct AudioManager {
    output: PlaybackStream,
    input: Option<CaptureStream>,
    source_ids: HashMap<SourceType, AudioSourceId>,
}

pub type AudioManagerHandle = Arc<RwLock<AudioManager>>;

impl AudioManager {
    pub fn new(app: AppHandle, audio_config: &AudioConfig) -> Result<Self, Error> {
        let (output, source_ids) = Self::create_playback_stream(app, audio_config, false)?;

        Ok(Self {
            output,
            input: None,
            source_ids,
        })
    }

    pub fn output_device_name(&self) -> String {
        self.output.device_name()
    }

    pub fn switch_output_device(
        &mut self,
        app: AppHandle,
        audio_config: &AudioConfig,
        restarting: bool,
    ) -> Result<(), Error> {
        let (output, source_ids) = Self::create_playback_stream(app, audio_config, restarting)?;
        self.output = output;
        self.source_ids = source_ids;
        Ok(())
    }

    pub fn attach_input_device(
        &mut self,
        app: AppHandle,
        audio_config: &AudioConfig,
        tx: mpsc::Sender<EncodedAudioFrame>,
        muted: bool,
    ) -> Result<(), Error> {
        let (device, is_fallback) = DeviceSelector::open(
            DeviceType::Input,
            audio_config.host_name.as_deref(),
            audio_config.input_device_id.as_deref(),
            audio_config.input_device_name.as_deref(),
        )?;
        if is_fallback {
            app.emit::<FrontendError>("error", FrontendError::from(Error::AudioDevice(Box::from(AudioError::Other(
                anyhow::anyhow!("Selected audio input device is not available, falling back to next best option. End your call to check your audio settings.")
            )))).non_critical()).ok();
        }

        let (error_tx, mut error_rx) = mpsc::channel(AUDIO_STREAM_ERROR_CHANNEL_SIZE);

        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(err) = error_rx.recv().await {
                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if let Some(call_id) = state.active_call_id().cloned() {
                    log::debug!("Ending active call {call_id} due to capture stream error");

                    state.cleanup_call(&call_id).await;
                    if let Err(err) = state
                        .send_signaling_message(shared::CallError {
                            call_id,
                            reason: CallErrorReason::AudioFailure,
                            message: None,
                        })
                        .await
                    {
                        log::warn!("Failed to send call end signaling message: {:?}", err);
                    };
                    state.set_outgoing_call(None);
                    app.state::<AudioManagerHandle>()
                        .read()
                        .stop(SourceType::Ringback);

                    app.emit("signaling:call-end", &call_id).ok();
                }

                app.emit::<FrontendError>("error", Error::from(err).into())
                    .ok();
            }
            log::debug!("Playback capture error receiver closed");
        });

        let capture = CaptureStream::start(
            device,
            tx,
            audio_config.input_device_volume,
            audio_config.input_device_volume_amp,
            error_tx,
            muted,
        )?;

        app_clone
            .emit("audio:stop-input-level-meter", Value::Null)
            .ok();

        self.input = Some(capture);
        Ok(())
    }

    pub fn attach_input_level_meter(
        &mut self,
        app: AppHandle,
        audio_config: &AudioConfig,
        emit: Box<dyn Fn(InputLevel) + Send>,
    ) -> Result<(), Error> {
        let (device, _) = DeviceSelector::open(
            DeviceType::Input,
            audio_config.host_name.as_deref(),
            audio_config.input_device_id.as_deref(),
            audio_config.input_device_name.as_deref(),
        )?;

        let (error_tx, mut error_rx) = mpsc::channel(AUDIO_STREAM_ERROR_CHANNEL_SIZE);

        tauri::async_runtime::spawn(async move {
            while let Some(err) = error_rx.recv().await {
                app.state::<AudioManagerHandle>()
                    .write()
                    .detach_input_device();

                app.emit("audio:stop-input-level-meter", Value::Null).ok();
                app.emit::<FrontendError>("error", Error::from(err).into())
                    .ok();
            }
            log::debug!("Playback capture error receiver closed");
        });

        self.input = Some(CaptureStream::start_level_meter(
            device,
            emit,
            audio_config.input_device_volume,
            audio_config.input_device_volume_amp,
            error_tx,
        )?);
        Ok(())
    }

    pub fn is_input_device_attached(&self) -> bool {
        self.input.is_some()
    }

    pub fn is_input_level_meter_attached(&self) -> bool {
        self.input
            .as_ref()
            .map(CaptureStream::is_level_meter)
            .unwrap_or(false)
    }

    pub fn detach_input_device(&mut self) {
        self.input = None;
        log::debug!("Detached input device");
    }

    pub fn start(&self, source_type: SourceType) {
        self.output
            .start_audio_source(self.source_ids[&source_type]);
    }

    pub fn restart(&self, source_type: SourceType) {
        self.output
            .restart_audio_source(self.source_ids[&source_type]);
    }

    pub fn stop(&self, source_type: SourceType) {
        self.output.stop_audio_source(self.source_ids[&source_type]);
    }

    pub fn set_output_volume(&self, source_type: SourceType, volume: f32) {
        if !self.source_ids.contains_key(&source_type) {
            log::trace!(
                "Tried to set output volume {volume} for missing audio source {source_type:?}, skipping"
            );
            return;
        }

        self.output
            .set_volume(self.source_ids[&source_type], volume);

        match source_type {
            SourceType::Ring | SourceType::Click | SourceType::RingbackOneshot => {
                self.output
                    .restart_audio_source(self.source_ids[&source_type]);
            }
            _ => {}
        }
    }

    pub fn set_input_volume(&self, volume: f32) {
        if let Some(input) = &self.input {
            input.set_volume(volume);
        }
    }

    pub fn set_input_muted(&self, muted: bool) {
        if let Some(input) = &self.input {
            input.set_muted(muted);
        }
    }

    pub fn attach_call_output(
        &mut self,
        webrtc_rx: mpsc::Receiver<EncodedAudioFrame>,
        volume: f32,
        amp: f32,
    ) -> Result<(), Error> {
        if self.source_ids.contains_key(&SourceType::Opus) {
            log::warn!("Tried to attach call but a call was already attached");
            return Err(AudioError::Other(anyhow::anyhow!(
                "Tried to attach call but a call was already attached"
            ))
            .into());
        }

        self.source_ids.insert(
            SourceType::Opus,
            self.output.add_audio_source(Box::new(OpusSource::new(
                webrtc_rx,
                self.output.resampler()?,
                self.output.channels(),
                volume,
                amp,
            )?)),
        );
        log::info!("Attached call");

        Ok(())
    }

    pub fn detach_call_output(&mut self) {
        if let Some(source_id) = self.source_ids.remove(&SourceType::Opus) {
            self.output.remove_audio_source(source_id);
            log::info!("Detached call output");
        } else {
            log::debug!("Tried to detach call output but no call was attached");
        }
    }

    fn create_playback_stream(
        app: AppHandle,
        audio_config: &AudioConfig,
        restarting: bool,
    ) -> Result<(PlaybackStream, HashMap<SourceType, AudioSourceId>), Error> {
        let (output_device, is_fallback) = DeviceSelector::open(
            DeviceType::Output,
            audio_config.host_name.as_deref(),
            audio_config.output_device_id.as_deref(),
            audio_config.output_device_name.as_deref(),
        )?;
        if is_fallback {
            app.emit::<FrontendError>("error", FrontendError::from(Error::AudioDevice(Box::from(AudioError::Other(
                anyhow::anyhow!("Selected audio output device is not available, falling back to next best option. Check your audio settings.")
            )))).non_critical()).ok();
        }

        let sample_rate = output_device.sample_rate() as f32;
        let channels = output_device.channels() as usize;

        let (error_tx, mut error_rx) = mpsc::channel(AUDIO_STREAM_ERROR_CHANNEL_SIZE);
        let output = PlaybackStream::start(output_device, error_tx)?;

        let audio_config_clone = audio_config.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(err) = error_rx.recv().await {
                let state = app.state::<AppState>();
                let mut state = state.lock().await;

                if restarting {
                    log::error!(
                        "Restarting output device after failure errored, cannot recover: {:?}",
                        err
                    );
                    app.emit::<FrontendError>("error", Error::AudioDevice(Box::from(AudioError::Other(
                        anyhow::anyhow!("Audio output device failed to start irrecoverably, check your audio settings and restart the application.")
                    ))).into()).ok();
                } else {
                    if let Some(call_id) = state.active_call_id().cloned() {
                        log::debug!("Ending active call {call_id} due to playback stream error");

                        state.cleanup_call(&call_id).await;
                        if let Err(err) = state
                            .send_signaling_message(shared::CallError {
                                call_id,
                                reason: CallErrorReason::AudioFailure,
                                message: None,
                            })
                            .await
                        {
                            log::warn!("Failed to send call end signaling message: {:?}", err);
                        };
                        state.set_outgoing_call(None);
                        app.state::<AudioManagerHandle>()
                            .read()
                            .stop(SourceType::Ringback);

                        app.emit("signaling:call-end", &call_id).ok();
                    }

                    if let Err(err) = app
                        .state::<AudioManagerHandle>()
                        .write()
                        .switch_output_device(app.clone(), &audio_config_clone, true)
                    {
                        log::error!("Failed to switch output device after failure: {:?}", err);

                        app.emit::<FrontendError>("error", Error::AudioDevice(Box::from(AudioError::Other(
                            anyhow::anyhow!("Audio output device failed to start irrecoverably, check your audio settings and restart the application.")
                        ))).into()).ok();

                        return;
                    } else {
                        log::info!(
                            "Successfully restarted output device after failure, continuing playback"
                        );
                    }

                    app.emit::<FrontendError>(
                        "error",
                        FrontendError::from(Error::from(err)).non_critical(),
                    )
                    .ok();
                }
            }
            log::debug!("Playback stream error receiver closed");
        });

        let mut source_ids = HashMap::new();
        source_ids.insert(
            SourceType::Ring,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::Ring,
                sample_rate,
                channels,
                audio_config.chime_volume,
            ))),
        );
        source_ids.insert(
            SourceType::PriorityRing,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::PriorityRing,
                sample_rate,
                channels,
                audio_config.chime_volume,
            ))),
        );
        source_ids.insert(
            SourceType::Ringback,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::Ringback,
                sample_rate,
                channels,
                audio_config.output_device_volume,
            ))),
        );
        source_ids.insert(
            SourceType::RingbackOneshot,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::RingbackOneshot,
                sample_rate,
                channels,
                audio_config.output_device_volume,
            ))),
        );
        source_ids.insert(
            SourceType::Click,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::Click,
                sample_rate,
                channels,
                audio_config.click_volume,
            ))),
        );
        source_ids.insert(
            SourceType::CallStart,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::CallStart,
                sample_rate,
                channels,
                audio_config.output_device_volume,
            ))),
        );
        source_ids.insert(
            SourceType::CallEnd,
            output.add_audio_source(Box::new(SourceType::into_waveform_source(
                SourceType::CallEnd,
                sample_rate,
                channels,
                audio_config.output_device_volume,
            ))),
        );

        Ok((output, source_ids))
    }
}
