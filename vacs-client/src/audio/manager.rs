use crate::app::state::signaling::AppStateSignalingExt;
use crate::app::state::webrtc::AppStateWebrtcExt;
use crate::app::state::{AppState, AppStateInner};
use crate::audio::source_type::SourceType;
use crate::audio::{AudioConfig, PlaybackDeviceType};
use crate::error::{Error, FrontendError};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use vacs_audio::EncodedAudioFrame;
use vacs_audio::device::{DeviceSelector, DeviceType, StreamDevice};
use vacs_audio::error::AudioError;
use vacs_audio::sources::opus::OpusSource;
use vacs_audio::sources::{AudioSource, AudioSourceId};
use vacs_audio::stream::capture::{CaptureStream, InputLevel};
use vacs_audio::stream::playback::PlaybackStream;
use vacs_signaling::protocol::ws::shared::CallErrorReason;

const AUDIO_STREAM_ERROR_CHANNEL_SIZE: usize = 32;

const RESTART_COOLDOWN: Duration = Duration::from_secs(2);

type SourceMap = HashMap<SourceType, AudioSourceId>;

pub struct AudioManager {
    output: PlaybackStream,
    speaker: Option<PlaybackStream>,
    input: Option<CaptureStream>,
    output_source_ids: SourceMap,
    speaker_source_ids: SourceMap,
    call_output_source_ids: HashSet<AudioSourceId>,
}

pub type AudioManagerHandle = Arc<RwLock<AudioManager>>;

impl AudioManager {
    pub fn new(app: AppHandle, audio_config: &AudioConfig) -> Result<Self, Error> {
        let (output_device, is_fallback) = DeviceSelector::open(
            DeviceType::Output,
            audio_config.host_name.as_deref(),
            audio_config.output_device_id.as_deref(),
            audio_config.output_device_name.as_deref(),
        )?;
        let (output, output_source_ids) = Self::create_playback_stream(
            app.clone(),
            output_device,
            is_fallback,
            audio_config,
            None,
            PlaybackDeviceType::Output,
        )?;

        let (speaker, speaker_source_ids) = if audio_config.speaker_enabled {
            let (speaker_device, is_fallback) = DeviceSelector::open(
                DeviceType::Output,
                audio_config.host_name.as_deref(),
                audio_config.speaker_device_id.as_deref(),
                audio_config.speaker_device_name.as_deref(),
            )?;
            let (speaker, speaker_source_ids) = Self::create_playback_stream(
                app,
                speaker_device,
                is_fallback,
                audio_config,
                None,
                PlaybackDeviceType::Speaker,
            )?;
            (Some(speaker), speaker_source_ids)
        } else {
            (None, HashMap::new())
        };

        Ok(Self {
            output,
            input: None,
            speaker,
            output_source_ids,
            speaker_source_ids,
            call_output_source_ids: HashSet::new(),
        })
    }

    pub fn output_device_name(&self) -> String {
        self.output.device_name()
    }

    pub fn speaker_device_name(&self) -> Option<String> {
        self.speaker.as_ref().map(|s| s.device_name())
    }

    pub fn switch_playback_device(
        &mut self,
        app: AppHandle,
        audio_config: &AudioConfig,
        device_type: PlaybackDeviceType,
        restarted_at: Option<Instant>,
    ) -> Result<(), Error> {
        if device_type == PlaybackDeviceType::Speaker && !audio_config.speaker_enabled {
            self.speaker = None;
            self.speaker_source_ids = HashMap::new();
            return Ok(());
        }

        let (device_id, device_name) = match device_type {
            PlaybackDeviceType::Output => (
                audio_config.output_device_id.as_deref(),
                audio_config.output_device_name.as_deref(),
            ),
            PlaybackDeviceType::Speaker => (
                audio_config.speaker_device_id.as_deref(),
                audio_config.speaker_device_name.as_deref(),
            ),
        };

        let (output_device, is_fallback) = DeviceSelector::open(
            DeviceType::Output,
            audio_config.host_name.as_deref(),
            device_id,
            device_name,
        )?;
        let (stream, source_ids) = Self::create_playback_stream(
            app,
            output_device,
            is_fallback,
            audio_config,
            restarted_at,
            device_type,
        )?;

        match device_type {
            PlaybackDeviceType::Output => {
                self.output = stream;
                self.output_source_ids = source_ids;
            }
            PlaybackDeviceType::Speaker => {
                self.speaker = Some(stream);
                self.speaker_source_ids = source_ids;
            }
        }

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

                end_call_on_stream_failure(&app, &mut state, "capture").await;

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
        emit: Arc<dyn Fn(InputLevel) + Send + Sync>,
        restarted_at: Option<Instant>,
    ) -> Result<(), Error> {
        let (device, _) = DeviceSelector::open(
            DeviceType::Input,
            audio_config.host_name.as_deref(),
            audio_config.input_device_id.as_deref(),
            audio_config.input_device_name.as_deref(),
        )?;

        let (error_tx, mut error_rx) = mpsc::channel(AUDIO_STREAM_ERROR_CHANNEL_SIZE);

        let audio_config_clone = audio_config.clone();
        let emit_clone = emit.clone();
        tauri::async_runtime::spawn(async move {
            // Handle only the first error event: every recovery path either
            // replaces this stream (making any further events from it stale)
            // or gives up for good.
            let Some(err) = error_rx.recv().await else {
                log::debug!("Input level meter error receiver closed");
                return;
            };

            let device_changed = matches!(err, AudioError::StreamInvalidated);
            let in_restart_cooldown = restarted_at.is_some_and(|t| t.elapsed() < RESTART_COOLDOWN);

            let give_up = |app: &AppHandle| {
                app.state::<AudioManagerHandle>()
                    .write()
                    .detach_input_device();
                app.emit("audio:stop-input-level-meter", Value::Null).ok();

                app.emit::<FrontendError>("error", Error::AudioDevice(Box::from(AudioError::Other(
                    anyhow::anyhow!("Audio input level meter failed to start irrecoverably, check your audio settings and reopen the settings page.")
                ))).into()).ok();
            };

            if in_restart_cooldown {
                if !device_changed {
                    log::error!(
                        "Restarting input level meter after failure errored again within {RESTART_COOLDOWN:?}, cannot recover: {:?}",
                        err
                    );
                    give_up(&app);
                    return;
                }

                // The default device changed again right after a restart.
                // Rebuilding is the only recovery whatever the cause; a
                // genuinely broken device still fails the rebuild below and
                // hard-errors there. Rate-limit the rebuild instead of
                // giving up.
                if let Some(remaining) =
                    restarted_at.and_then(|t| RESTART_COOLDOWN.checked_sub(t.elapsed()))
                {
                    tokio::time::sleep(remaining).await;
                }
            }

            let res = app
                .state::<AudioManagerHandle>()
                .write()
                .attach_input_level_meter(
                    app.clone(),
                    &audio_config_clone,
                    emit_clone.clone(),
                    Some(Instant::now()),
                );

            if let Err(err) = res {
                log::error!(
                    "Failed to switch input level meter after failure: {:?}",
                    err
                );
                give_up(&app);
                return;
            }

            // A successful recovery needs no user-facing notification: cpal
            // also reports transient backend hiccups on live streams (e.g.
            // ALSA EIO during plugin spin-up), and a restart onto a fallback
            // device already emits its own toast via the is_fallback path.
            log::info!(
                "Successfully restarted input level meter after failure, continuing capture"
            );
        });

        self.input = Some(CaptureStream::start_level_meter(
            device,
            Box::new(move |level| emit(level)),
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
        if let Some(speaker) = &self.speaker
            && let Some(source_id) = self.speaker_source_ids.get(&source_type)
        {
            speaker.start_audio_source(*source_id);
        } else if let Some(source_id) = self.output_source_ids.get(&source_type) {
            self.output.start_audio_source(*source_id);
        }
    }

    pub fn restart(&self, source_type: SourceType) {
        if let Some(speaker) = &self.speaker
            && let Some(source_id) = self.speaker_source_ids.get(&source_type)
        {
            speaker.restart_audio_source(*source_id);
        } else if let Some(source_id) = self.output_source_ids.get(&source_type) {
            self.output.restart_audio_source(*source_id);
        }
    }

    pub fn stop(&self, source_type: SourceType) {
        if let Some(speaker) = &self.speaker
            && let Some(source_id) = self.speaker_source_ids.get(&source_type)
        {
            speaker.stop_audio_source(*source_id);
        } else if let Some(source_id) = self.output_source_ids.get(&source_type) {
            self.output.stop_audio_source(*source_id);
        }
    }

    pub fn set_output_volume(&self, source_type: SourceType, volume: f32) {
        if let Some(source_id) = self.output_source_ids.get(&source_type) {
            self.output.set_volume(*source_id, volume);

            match source_type {
                SourceType::RingbackOneshot => {
                    self.output.restart_audio_source(*source_id);
                }
                SourceType::Ring | SourceType::Click if self.speaker.is_none() => {
                    self.output.restart_audio_source(*source_id);
                }
                _ => {}
            }
        }

        if let Some(speaker) = &self.speaker
            && let Some(source_id) = self.speaker_source_ids.get(&source_type)
        {
            speaker.set_volume(*source_id, volume);

            match source_type {
                SourceType::Ring | SourceType::Click => {
                    speaker.restart_audio_source(*source_id);
                }
                _ => {}
            }
        } else if !self.output_source_ids.contains_key(&source_type) {
            log::trace!(
                "Tried to set output volume {volume} for missing audio source {source_type:?}, skipping"
            );
        }
    }

    pub fn set_call_output_volumes(&self, volume: f32) {
        for source_id in &self.call_output_source_ids {
            self.output.set_volume(*source_id, volume);
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
    ) -> Result<AudioSourceId, Error> {
        let source_id = self.output.add_audio_source(Box::new(OpusSource::new(
            webrtc_rx,
            self.output.resampler()?,
            self.output.channels(),
            volume,
            amp,
        )?));
        log::info!("Attached call with source ID {source_id}");

        self.call_output_source_ids.insert(source_id);

        Ok(source_id)
    }

    pub fn detach_call_output(&mut self, source_id: AudioSourceId) {
        self.output.remove_audio_source(source_id);
        self.call_output_source_ids.remove(&source_id);
        log::info!("Detached call output with source ID {source_id}");
    }

    pub fn detach_all_call_outputs(&mut self) {
        for source_id in self.call_output_source_ids.drain() {
            self.output.remove_audio_source(source_id);
            log::info!("Detached call output with source ID {source_id}");
        }
    }

    fn create_playback_stream(
        app: AppHandle,
        device: StreamDevice,
        is_fallback: bool,
        audio_config: &AudioConfig,
        restarted_at: Option<Instant>,
        device_type: PlaybackDeviceType,
    ) -> Result<(PlaybackStream, SourceMap), Error> {
        if is_fallback {
            app.emit::<FrontendError>("error", FrontendError::from(Error::AudioDevice(Box::from(AudioError::Other(
                anyhow::anyhow!("Selected audio output device is not available, falling back to next best option. Check your audio settings.")
            )))).non_critical()).ok();
        }

        let sample_rate = device.sample_rate() as f32;
        let channels = device.channels() as usize;

        let (error_tx, mut error_rx) = mpsc::channel(AUDIO_STREAM_ERROR_CHANNEL_SIZE);
        let output = PlaybackStream::start(device, error_tx)?;

        let audio_config_clone = audio_config.clone();
        tauri::async_runtime::spawn(async move {
            // Handle only the first error event: every recovery path either
            // replaces this stream (making any further events from it stale,
            // e.g. queued duplicates from Windows default-role changes) or
            // gives up for good.
            if let Some(err) = error_rx.recv().await {
                handle_playback_stream_error(
                    err,
                    restarted_at,
                    &audio_config_clone,
                    device_type,
                    app.clone(),
                )
                .await;
            }
            log::debug!("Playback stream error receiver closed");
        });

        let mut source_ids = HashMap::new();

        let insert_waveform_source =
            |source_ids: &mut SourceMap, source_type: SourceType, volume: f32| {
                source_ids.insert(
                    source_type,
                    output.add_audio_source(Box::new(SourceType::into_waveform_source(
                        source_type,
                        sample_rate,
                        channels,
                        volume,
                    ))),
                );
            };

        insert_waveform_source(&mut source_ids, SourceType::Ring, audio_config.chime_volume);
        insert_waveform_source(
            &mut source_ids,
            SourceType::PriorityRing,
            audio_config.chime_volume,
        );
        insert_waveform_source(
            &mut source_ids,
            SourceType::Click,
            audio_config.click_volume,
        );

        if device_type == PlaybackDeviceType::Output {
            insert_waveform_source(
                &mut source_ids,
                SourceType::Ringback,
                audio_config.output_device_volume,
            );
            insert_waveform_source(
                &mut source_ids,
                SourceType::RingbackOneshot,
                audio_config.output_device_volume,
            );
            insert_waveform_source(
                &mut source_ids,
                SourceType::CallStart,
                audio_config.output_device_volume,
            );
            insert_waveform_source(
                &mut source_ids,
                SourceType::CallEnd,
                audio_config.output_device_volume,
            );
        }

        Ok((output, source_ids))
    }

    pub fn add_audio_source(
        &self,
        source_fn: impl FnOnce(u32, u16) -> Result<Box<dyn AudioSource>, Error>,
        device_type: PlaybackDeviceType,
    ) -> Result<(AudioSourceId, PlaybackDeviceType), Error> {
        let actual_type = if device_type == PlaybackDeviceType::Speaker && self.speaker.is_some() {
            PlaybackDeviceType::Speaker
        } else {
            PlaybackDeviceType::Output
        };
        let stream = self.get_stream_for_playback(device_type);
        Ok((
            stream.add_audio_source(source_fn(stream.sample_rate(), stream.channels())?),
            actual_type,
        ))
    }

    pub fn start_audio_source(&self, source_id: AudioSourceId, device_type: PlaybackDeviceType) {
        self.get_stream_for_playback(device_type)
            .start_audio_source(source_id);
    }

    pub fn stop_audio_source(&self, source_id: AudioSourceId, device_type: PlaybackDeviceType) {
        self.get_stream_for_playback(device_type)
            .stop_audio_source(source_id);
    }

    pub fn remove_audio_source(&self, source_id: AudioSourceId, device_type: PlaybackDeviceType) {
        self.get_stream_for_playback(device_type)
            .remove_audio_source(source_id);
    }

    pub fn skip_in_audio_source(
        &self,
        source_id: AudioSourceId,
        duration: Duration,
        device_type: PlaybackDeviceType,
    ) {
        self.get_stream_for_playback(device_type)
            .skip_in_audio_source(source_id, duration);
    }

    pub fn rewind_in_audio_source(
        &self,
        source_id: AudioSourceId,
        duration: Duration,
        device_type: PlaybackDeviceType,
    ) {
        self.get_stream_for_playback(device_type)
            .rewind_in_audio_source(source_id, duration);
    }

    fn get_stream_for_playback(&self, device_type: PlaybackDeviceType) -> &PlaybackStream {
        match (device_type, self.speaker.as_ref()) {
            (PlaybackDeviceType::Output, _) | (PlaybackDeviceType::Speaker, None) => &self.output,
            (PlaybackDeviceType::Speaker, Some(speaker)) => speaker,
        }
    }
}

async fn end_call_on_stream_failure(app: &AppHandle, state: &mut AppStateInner, cause: &str) {
    if let Some(call_id) = state.current_call_id() {
        log::debug!("Ending active call {call_id} due to {cause} stream error");

        state.cleanup_current_call(call_id).await;
        state
            .try_send_call_error_with_client_id(call_id, CallErrorReason::AudioFailure, None)
            .await;

        app.emit("signaling:force-call-end", &call_id).ok();
    }
}

async fn handle_playback_stream_error(
    err: AudioError,
    restarted_at: Option<Instant>,
    audio_config: &AudioConfig,
    device_type: PlaybackDeviceType,
    app: AppHandle,
) {
    if app.try_state::<AppState>().is_none() {
        log::warn!(
            "Dropping {device_type} stream error, app startup has not finished yet: {err:?}"
        );
        return;
    }

    let device_changed = matches!(err, AudioError::StreamInvalidated);
    let in_restart_cooldown = restarted_at.is_some_and(|t| t.elapsed() < RESTART_COOLDOWN);

    let give_up = |app: &AppHandle| {
        app.emit::<FrontendError>("error", Error::AudioDevice(Box::from(AudioError::Other(
            anyhow::anyhow!("Audio {device_type} device failed to start irrecoverably, check your audio settings and restart the application.")
        ))).into()).ok();
    };

    if in_restart_cooldown {
        if !device_changed {
            log::error!(
                "Restarting {device_type} device after failure errored again within {RESTART_COOLDOWN:?}, cannot recover: {:?}",
                err
            );
            give_up(&app);
            return;
        }

        // The default device changed again right after a restart. The event
        // does not tell us why (user switching quickly, a flapping device,
        // Windows role-change bursts) - rebuilding is the only recovery either
        // way, and a genuinely broken device still fails the rebuild below and
        // hard-errors there. Rate-limit the rebuild instead of giving up.
        if let Some(remaining) =
            restarted_at.and_then(|t| RESTART_COOLDOWN.checked_sub(t.elapsed()))
        {
            tokio::time::sleep(remaining).await;
        }
    }

    let state = app.state::<AppState>();
    let mut state = state.lock().await;

    // Calls attach their audio to the output stream only, so no speaker
    // failure can affect call audio and none must end the call.
    if device_type == PlaybackDeviceType::Output {
        end_call_on_stream_failure(&app, &mut state, "playback").await;
    }

    let res = {
        let audio_manager = app.state::<AudioManagerHandle>();
        let mut audio_manager = audio_manager.write();

        audio_manager.switch_playback_device(
            app.clone(),
            audio_config,
            device_type,
            Some(Instant::now()),
        )
    };

    if let Err(err) = res {
        log::error!(
            "Failed to switch {device_type} device after failure: {:?}",
            err
        );

        give_up(&app);
        return;
    }

    // A successful recovery needs no user-facing notification: cpal also
    // reports transient backend hiccups on live streams (e.g. ALSA EIO during
    // plugin spin-up), and a restart onto a fallback device already emits its
    // own toast via the is_fallback path.
    log::info!("Successfully restarted {device_type} device after failure, continuing playback");
}
