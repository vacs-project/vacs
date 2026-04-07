use crate::TARGET_SAMPLE_RATE;
use crate::backend::{
    AudioBackend, AudioDevice, AudioHost, AudioStream, BufferSize, DeviceDirection, StreamConfig,
    StreamConfigRange,
};
use crate::error::AudioError;
use cpal::SampleFormat;
use rubato::{
    Async, FixedAsync, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use tracing::instrument;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DeviceType {
    Input,
    Output,
}

impl Display for DeviceType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceType::Input => write!(f, "input"),
            DeviceType::Output => write!(f, "output"),
        }
    }
}

pub struct StreamDevice {
    pub(crate) device_type: DeviceType,
    pub(crate) device: Box<dyn AudioDevice>,
    pub(crate) config: StreamConfig,
}

impl StreamDevice {
    #[inline]
    pub fn device_type(&self) -> DeviceType {
        self.device_type
    }

    #[inline]
    pub fn name(&self) -> String {
        self.device.name()
    }

    #[inline]
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    #[inline]
    pub fn channels(&self) -> u16 {
        self.config.channels
    }

    #[instrument(level = "trace", skip(data_callback, error_callback), err)]
    pub(crate) fn build_input_stream(
        &self,
        data_callback: crate::backend::InputDataCallback,
        error_callback: crate::backend::ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        debug_assert!(matches!(self.device_type, DeviceType::Input));
        self.device
            .build_input_stream_f32(&self.config, data_callback, error_callback)
    }

    #[instrument(level = "trace", skip(data_callback, error_callback), err)]
    pub(crate) fn build_output_stream(
        &self,
        data_callback: crate::backend::OutputDataCallback,
        error_callback: crate::backend::ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        debug_assert!(matches!(self.device_type, DeviceType::Output));
        self.device
            .build_output_stream_f32(&self.config, data_callback, error_callback)
    }

    pub(crate) fn resampler(&self) -> Result<Option<Async<f32>>, AudioError> {
        if self.sample_rate() == TARGET_SAMPLE_RATE {
            Ok(None)
        } else {
            let resampler_params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Cubic,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            };

            let resample_ratio = match self.device_type {
                DeviceType::Input => TARGET_SAMPLE_RATE as f64 / self.sample_rate() as f64,
                DeviceType::Output => self.sample_rate() as f64 / TARGET_SAMPLE_RATE as f64,
            };

            let chunk_size = match self.config.buffer_size {
                BufferSize::Fixed(n) => n as usize,
                BufferSize::Default => 1024usize,
            };

            Ok(Some(
                Async::<f32>::new_sinc(
                    resample_ratio,
                    2.0,
                    &resampler_params,
                    chunk_size,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|e| {
                    AudioError::Other(anyhow::anyhow!("Failed to create resampler: {e}"))
                })?,
            ))
        }
    }
}

impl Debug for StreamDevice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StreamDevice {{ device_type: {}, device: {} (id: {}), config: {:?} }}",
            self.device_type,
            self.device.name(),
            self.device.id().unwrap_or_default(),
            self.config,
        )
    }
}

/// Extension trait for [`AudioBackend`] that adds device selection,
/// config scoring, and host/device enumeration logic.
pub trait AudioBackendExt: AudioBackend {
    fn open(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<(StreamDevice, bool), AudioError>;

    fn all_host_names(&self) -> Vec<String>;
    fn default_host_name(&self) -> String;

    fn all_device_names(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
    ) -> Result<Vec<String>, AudioError>;

    fn default_device_name(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
    ) -> Result<String, AudioError>;

    fn picked_device_name(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<String, AudioError>;

    /// Resolves the stable device ID for a device identified by display name.
    /// Returns `None` if the device cannot be found or has no ID.
    fn resolve_device_id(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
        device_name: &str,
    ) -> Option<String>;
}

impl<T: AudioBackend + ?Sized> AudioBackendExt for T {
    #[instrument(level = "debug", skip(self), err)]
    fn open(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<(StreamDevice, bool), AudioError> {
        let host = select_host(self, preferred_host);
        let (device, config, is_fallback) = pick_device_with_stream_config(
            device_type,
            host.as_ref(),
            preferred_device_id,
            preferred_device_name,
        )?;

        Ok((
            StreamDevice {
                device_type,
                device,
                config,
            },
            is_fallback,
        ))
    }

    #[instrument(level = "debug", skip(self))]
    fn all_host_names(&self) -> Vec<String> {
        self.available_hosts()
            .iter()
            .map(|h| h.name().to_string())
            .collect()
    }

    #[instrument(level = "debug", skip(self))]
    fn default_host_name(&self) -> String {
        self.default_host().name().to_string()
    }

    #[instrument(level = "debug", skip(self), err)]
    fn all_device_names(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
    ) -> Result<Vec<String>, AudioError> {
        let host = select_host(self, preferred_host);
        let devices = host_devices(device_type, host.as_ref())?;

        let device_names = devices
            .into_iter()
            .filter_map(|device| {
                let name = device.name();
                if name.is_empty() {
                    return None;
                }
                if !has_supported_configs(device_type, device.as_ref()) {
                    return None;
                }
                Some(name)
            })
            .collect::<Vec<_>>();

        Ok(device_names)
    }

    #[instrument(level = "debug", skip(self), err)]
    fn default_device_name(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
    ) -> Result<String, AudioError> {
        tracing::debug!("Retrieving device name for default device");

        let host = select_host(self, preferred_host);
        let (device, _) = select_device(device_type, host.as_ref(), None, None)?;

        Ok(device.name())
    }

    #[instrument(level = "debug", skip(self), err)]
    fn picked_device_name(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<String, AudioError> {
        let host = select_host(self, preferred_host);
        let (device, _) = select_device(
            device_type,
            host.as_ref(),
            preferred_device_id,
            preferred_device_name,
        )?;

        Ok(device.name())
    }

    /// Resolves the stable device ID for a device identified by display name.
    /// Returns `None` if the device cannot be found or has no ID.
    fn resolve_device_id(
        &self,
        device_type: DeviceType,
        preferred_host: Option<&str>,
        device_name: &str,
    ) -> Option<String> {
        let host = select_host(self, preferred_host);
        let devices = host_devices(device_type, host.as_ref()).ok()?;
        let device = devices.iter().find(|d| {
            d.identifiers()
                .iter()
                .any(|n| n.eq_ignore_ascii_case(device_name))
        })?;
        device.id()
    }
}

#[instrument(level = "trace", skip(backend))]
fn select_host<B: AudioBackend + ?Sized>(
    backend: &B,
    preferred_host: Option<&str>,
) -> Box<dyn AudioHost> {
    if let Some(name) = preferred_host
        && let Some(host) = backend.host_by_name(name)
    {
        tracing::trace!(host_name = host.name(), "Selected preferred audio host");
        return host;
    }

    tracing::trace!("Selected default audio host");
    backend.default_host()
}

#[instrument(level = "trace", err, skip(host), fields(host = host.name()))]
fn pick_device_with_stream_config(
    device_type: DeviceType,
    host: &dyn AudioHost,
    preferred_device_id: Option<&str>,
    preferred_device_name: Option<&str>,
) -> Result<(Box<dyn AudioDevice>, StreamConfig, bool), AudioError> {
    let (mut device, mut is_fallback) = select_device(
        device_type,
        host,
        preferred_device_id,
        preferred_device_name,
    )?;

    let (stream_config, _) = match pick_best_stream_config(device_type, device.as_ref()) {
        Ok(stream_config) => stream_config,
        Err(err) => {
            tracing::warn!(?err, device_name = %device.name(), "Failed to pick stream config for preferred device, picking best fallback device");

            let devices = host_devices(device_type, host)?;
            let mut best_fallback: Option<(Box<dyn AudioDevice>, StreamConfig, StreamConfigScore)> =
                None;

            for dev in devices {
                if let Ok((config, score)) = pick_best_stream_config(device_type, dev.as_ref()) {
                    match &mut best_fallback {
                        None => best_fallback = Some((dev, config, score)),
                        Some((_, _, best_score)) => {
                            if score < *best_score {
                                *best_score = score;
                                best_fallback = Some((dev, config, score));
                            }
                        }
                    }
                }
            }

            if let Some((dev, config, score)) = best_fallback {
                tracing::info!(device_name = %dev.name(), ?config, "Selected fallback device");
                device = dev;
                is_fallback = true;
                (config, score)
            } else {
                return Err(AudioError::Other(anyhow::anyhow!(
                    "No supported stream config found for any device"
                )));
            }
        }
    };

    Ok((device, stream_config, is_fallback))
}

#[instrument(level = "trace", err, skip(host), fields(host = host.name()))]
fn host_devices(
    device_type: DeviceType,
    host: &dyn AudioHost,
) -> Result<Vec<Box<dyn AudioDevice>>, AudioError> {
    match device_type {
        DeviceType::Input => host.input_devices(),
        DeviceType::Output => host.output_devices(),
    }
}

#[instrument(level = "trace", err, skip(host), fields(host = host.name()))]
fn select_device(
    device_type: DeviceType,
    host: &dyn AudioHost,
    preferred_device_id: Option<&str>,
    preferred_device_name: Option<&str>,
) -> Result<(Box<dyn AudioDevice>, bool), AudioError> {
    if let Some(id_str) = preferred_device_id {
        if let Some(device) = host.device_by_id(id_str) {
            tracing::trace!(device_name = %device.name(), "Selected device by ID");
            return Ok((device, false));
        }
        tracing::debug!(
            id = id_str,
            "Stored device ID no longer available, falling back to name matching"
        );
    }

    // Fall back to name-based matching (backwards compat with old configs)
    if let Some(name) = preferred_device_name {
        let devices = host_devices(device_type, host)?;

        // Exact case-insensitive match against all device identifiers.
        // Checking multiple identifiers ensures backwards compatibility with
        // configs that stored names from older cpal versions (e.g. ALSA pcm_id
        // or WASAPI FriendlyName).
        if let Some(device) = devices
            .iter()
            .find(|d| d.identifiers().iter().any(|n| n.eq_ignore_ascii_case(name)))
        {
            tracing::trace!(device_name = %device.name(), "Selected preferred device");
            return Ok((device.clone_boxed(), false));
        }

        if let Some(device) = devices.iter().find(|d| {
            let name_lower = name.to_lowercase();
            d.identifiers()
                .iter()
                .any(|n| n.to_lowercase().contains(&name_lower))
        }) {
            tracing::trace!(device_name = %device.name(), "Selected preferred device (based on substring match)");
            return Ok((device.clone_boxed(), false));
        }
    }

    let device = match device_type {
        DeviceType::Input => host
            .default_input_device()
            .ok_or_else(|| AudioError::Other(anyhow::anyhow!("No default input device")))?,
        DeviceType::Output => host
            .default_output_device()
            .ok_or_else(|| AudioError::Other(anyhow::anyhow!("No default output device")))?,
    };
    tracing::trace!(device_name = %device.name(), "Selected default device");
    Ok((
        device,
        preferred_device_id.is_some() || preferred_device_name.is_some(),
    ))
}

/// Checks whether a device actually supports the requested stream direction.
///
/// ALSA hint devices with NULL IOID are tagged as `Duplex` even when they
/// only support one direction (e.g. `surround71:` appearing in the input
/// list). This method filters those out.
///
/// To avoid opening PCM devices unnecessarily (which on ALSA can leak file
/// descriptors and poison the backend for the process lifetime), we first
/// consult the direction metadata from the device description. Only
/// ambiguous input devices fall through to an actual config query; for
/// output we trust the metadata because an output stream might aready be
/// active during enumeration and probing would try to reopen the same
/// hardware via dmix, leaking FDs.
fn has_supported_configs(device_type: DeviceType, device: &dyn AudioDevice) -> bool {
    if let Ok(desc) = device.description() {
        match (device_type, desc.direction) {
            // Clear mismatch: exclude.
            (DeviceType::Input, DeviceDirection::Output)
            | (DeviceType::Output, DeviceDirection::Input) => return false,
            // Clear match: include.
            (DeviceType::Input, DeviceDirection::Input)
            | (DeviceType::Output, DeviceDirection::Output) => return true,
            // Duplex/Unknown output: include without probing. A device
            // listed by host.output_devices() that claims Duplex is a
            // legitimate output device.
            (DeviceType::Output, _) => return true,
            // Duplex/Unknown input: fall through to probe. Surround-only
            // output devices often claim Duplex via ALSA hints and must
            // be verified. No capture stream is typically open during
            // enumeration, so the probe is safe.
            (DeviceType::Input, _) => {}
        }
    }

    // Probe actual input config support for ambiguous devices.
    device
        .supported_input_configs()
        .is_ok_and(|configs| !configs.is_empty())
}

#[instrument(level = "trace", err, skip(device), fields(device_name = %device.name()))]
fn pick_best_stream_config(
    device_type: DeviceType,
    device: &dyn AudioDevice,
) -> Result<(StreamConfig, StreamConfigScore), AudioError> {
    let (configs, preferred_channels): (Vec<StreamConfigRange>, u16) = match device_type {
        DeviceType::Input => (device.supported_input_configs()?, 1),
        DeviceType::Output => (device.supported_output_configs()?, 2),
    };

    let mut best: Option<(StreamConfigRange, StreamConfigScore)> = None;

    for range in configs {
        let score = score_stream_config_range(&range, preferred_channels);
        match &mut best {
            None => best = Some((range, score)),
            Some((_, best_score)) => {
                if score < *best_score {
                    *best_score = score;
                    best = Some((range, score));
                }
            }
        }
    }

    let (range, score) =
        best.ok_or_else(|| AudioError::Other(anyhow::anyhow!("No supported stream config found")))?;
    let sample_rate = closest_sample_rate(range.min_sample_rate, range.max_sample_rate);

    tracing::trace!(?score, ?sample_rate, "Picked best stream config");
    Ok((range.with_sample_rate(sample_rate), score))
}

fn score_stream_config_range(
    range: &StreamConfigRange,
    preferred_channels: u16,
) -> StreamConfigScore {
    let sample_rate_distance = sample_rate_distance(range.min_sample_rate, range.max_sample_rate);

    let channels_distance = range.channels.abs_diff(preferred_channels);

    let format_preference = match range.sample_format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::U16 => 2,
        _ => 3,
    };

    StreamConfigScore(sample_rate_distance, channels_distance, format_preference)
}

fn sample_rate_distance(min: u32, max: u32) -> u32 {
    if min <= TARGET_SAMPLE_RATE && max >= TARGET_SAMPLE_RATE {
        0
    } else if TARGET_SAMPLE_RATE < min {
        min - TARGET_SAMPLE_RATE
    } else {
        TARGET_SAMPLE_RATE - max
    }
}

fn closest_sample_rate(min: u32, max: u32) -> u32 {
    if min <= TARGET_SAMPLE_RATE && max >= TARGET_SAMPLE_RATE {
        TARGET_SAMPLE_RATE
    } else if TARGET_SAMPLE_RATE < min {
        min
    } else {
        max
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct StreamConfigScore(u32, u16, u8); // sample_rate_distance, channels_distance, format_preference

impl Ord for StreamConfigScore {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0, self.1, self.2).cmp(&(other.0, other.1, other.2))
    }
}
impl PartialOrd for StreamConfigScore {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
