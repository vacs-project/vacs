use crate::TARGET_SAMPLE_RATE;
use crate::error::AudioError;
use anyhow::Context;
use cpal::device_description::DeviceDirection;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{DeviceId, Sample, SampleFormat, SupportedStreamConfig, SupportedStreamConfigRange};
use rubato::{
    Async, FixedAsync, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
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
    pub(crate) device: cpal::Device,
    pub(crate) config: cpal::StreamConfig,
    pub(crate) sample_format: SampleFormat,
}

impl StreamDevice {
    #[inline]
    pub fn device_type(&self) -> DeviceType {
        self.device_type
    }

    #[inline]
    pub fn name(&self) -> String {
        device_display_name(&self.device)
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
    pub(crate) fn build_input_stream<D, E>(
        &self,
        data_callback: D,
        error_callback: E,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        D: FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        debug_assert!(matches!(self.device_type, DeviceType::Input));

        match self.sample_format {
            SampleFormat::F32 => self.device.build_input_stream::<f32, _, _>(
                &self.config,
                data_callback,
                error_callback,
                None,
            ),
            SampleFormat::I16 => {
                self.build_f32_input_stream::<i16, _, _>(data_callback, error_callback)
            }
            SampleFormat::U16 => {
                self.build_f32_input_stream::<u16, _, _>(data_callback, error_callback)
            }
            other => Err(cpal::BuildStreamError::BackendSpecific {
                err: cpal::BackendSpecificError {
                    description: format!("Unsupported input sample format: {other:?}"),
                },
            }),
        }
    }

    fn build_f32_input_stream<T, D, E>(
        &self,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample<Float = f32> + cpal::SizedSample + 'static,
        D: FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        let buf: RefCell<Vec<f32>> = RefCell::new(Vec::new());
        if let cpal::BufferSize::Fixed(n) = self.config.buffer_size {
            buf.borrow_mut().reserve(n as usize);
        }

        self.device.build_input_stream::<T, _, _>(
            &self.config,
            move |input: &[T], info| {
                let mut b = buf.borrow_mut();
                if b.len() != input.len() {
                    b.resize(input.len(), 0.0f32);
                }
                for (dst, &src) in b.iter_mut().zip(input.iter()) {
                    *dst = src.to_float_sample();
                }
                data_callback(&b, info);
            },
            error_callback,
            None,
        )
    }

    #[instrument(level = "trace", skip(data_callback, error_callback), err)]
    pub(crate) fn build_output_stream<D, E>(
        &self,
        data_callback: D,
        error_callback: E,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        D: FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        debug_assert!(matches!(self.device_type, DeviceType::Output));

        match self.sample_format {
            SampleFormat::F32 => self.device.build_output_stream::<f32, _, _>(
                &self.config,
                data_callback,
                error_callback,
                None,
            ),
            SampleFormat::I16 => {
                self.build_f32_output_stream::<i16, _, _>(data_callback, error_callback)
            }
            SampleFormat::U16 => {
                self.build_f32_output_stream::<u16, _, _>(data_callback, error_callback)
            }
            other => Err(cpal::BuildStreamError::BackendSpecific {
                err: cpal::BackendSpecificError {
                    description: format!("Unsupported output sample format: {other:?}"),
                },
            }),
        }
    }

    fn build_f32_output_stream<T, D, E>(
        &self,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: cpal::SizedSample + cpal::FromSample<f32> + 'static,
        D: FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        let buf: RefCell<Vec<f32>> = RefCell::new(Vec::new());
        if let cpal::BufferSize::Fixed(n) = self.config.buffer_size {
            buf.borrow_mut().reserve(n as usize);
        }

        self.device.build_output_stream::<T, _, _>(
            &self.config,
            move |output: &mut [T], info| {
                let mut b = buf.borrow_mut();
                if b.len() != output.len() {
                    b.resize(output.len(), 0.0f32);
                }
                data_callback(&mut b, info);
                for (dst, &src) in output.iter_mut().zip(b.iter()) {
                    *dst = src.to_sample::<T>();
                }
            },
            error_callback,
            None,
        )
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

            Ok(Some(
                Async::<f32>::new_sinc(
                    resample_ratio,
                    2.0,
                    &resampler_params,
                    if let cpal::BufferSize::Fixed(n) = self.config.buffer_size {
                        n as usize
                    } else {
                        1024usize
                    },
                    1,
                    FixedAsync::Input,
                )
                .context("Failed to create resampler")?,
            ))
        }
    }
}

impl Debug for StreamDevice {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StreamDevice {{ device_type: {}, device: {} (id: {}), config: {:?}, sample_format: {:?} }}",
            self.device_type,
            device_display_name(&self.device),
            self.device
                .id()
                .map(|id| id.to_string())
                .unwrap_or_default(),
            self.config,
            self.sample_format
        )
    }
}

pub struct DeviceSelector {}

impl DeviceSelector {
    #[instrument(level = "debug", err)]
    pub fn open(
        device_type: DeviceType,
        preferred_host: Option<&str>,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<(StreamDevice, bool), AudioError> {
        let host = Self::select_host(preferred_host);
        let (device, stream_config, is_fallback) = Self::pick_device_with_stream_config(
            device_type,
            &host,
            preferred_device_id,
            preferred_device_name,
        )?;

        Ok((
            StreamDevice {
                device_type,
                device,
                config: stream_config.config(),
                sample_format: stream_config.sample_format(),
            },
            is_fallback,
        ))
    }

    #[instrument(level = "debug")]
    pub fn all_host_names() -> Vec<String> {
        cpal::available_hosts()
            .iter()
            .map(|id| id.name().to_string())
            .collect::<Vec<_>>()
    }

    #[instrument(level = "debug")]
    pub fn default_host_name() -> String {
        cpal::default_host().id().name().to_string()
    }

    #[instrument(level = "debug", err)]
    pub fn all_device_names(
        device_type: DeviceType,
        preferred_host: Option<&str>,
    ) -> Result<Vec<String>, AudioError> {
        let host = Self::select_host(preferred_host);
        let devices = Self::host_devices(device_type, &host)?;

        let device_names = devices
            .into_iter()
            .filter_map(|device| {
                let name = device_display_name(&device);
                if name.is_empty() {
                    return None;
                }
                if !Self::has_supported_configs(device_type, &device) {
                    return None;
                }
                Some(name)
            })
            .collect::<Vec<_>>();

        Ok(device_names)
    }

    #[instrument(level = "debug", err)]
    pub fn default_device_name(
        device_type: DeviceType,
        preferred_host: Option<&str>,
    ) -> Result<String, AudioError> {
        tracing::debug!("Retrieving device name for default device");

        let host = Self::select_host(preferred_host);
        let (device, _) = Self::select_device(device_type, &host, None, None)?;

        Ok(device_display_name(&device))
    }

    #[instrument(level = "debug", err)]
    pub fn picked_device_name(
        device_type: DeviceType,
        preferred_host: Option<&str>,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<String, AudioError> {
        let host = Self::select_host(preferred_host);
        let (device, _) = Self::select_device(
            device_type,
            &host,
            preferred_device_id,
            preferred_device_name,
        )?;

        Ok(device_display_name(&device))
    }

    /// Resolves the stable device ID for a device identified by display name.
    /// Returns `None` if the device cannot be found or has no ID.
    pub fn resolve_device_id(
        device_type: DeviceType,
        preferred_host: Option<&str>,
        device_name: &str,
    ) -> Option<String> {
        let host = Self::select_host(preferred_host);
        let devices = Self::host_devices(device_type, &host).ok()?;
        let device = devices.iter().find(|d| {
            device_identifiers(d)
                .iter()
                .any(|n| n.eq_ignore_ascii_case(device_name))
        })?;
        device_id_string(device)
    }

    #[instrument(level = "trace")]
    fn select_host(preferred_host: Option<&str>) -> cpal::Host {
        let hosts = cpal::available_hosts();

        if let Some(name) = preferred_host {
            if let Some(id) = hosts.iter().find(|id| id.name().eq_ignore_ascii_case(name)) {
                tracing::trace!(?id, "Selected preferred audio host");
                return cpal::host_from_id(*id).unwrap_or(cpal::default_host());
            }
            if let Some(id) = hosts
                .iter()
                .find(|id| id.name().to_lowercase().contains(&name.to_lowercase()))
            {
                tracing::trace!(
                    ?id,
                    "Selected preferred audio host (based on substring match)"
                );
                return cpal::host_from_id(*id).unwrap_or(cpal::default_host());
            }
        }

        tracing::trace!("Selected default audio host");
        cpal::default_host()
    }

    #[instrument(level = "trace", err, skip(host), fields(host = ?HostDebug(host)))]
    fn pick_device_with_stream_config(
        device_type: DeviceType,
        host: &cpal::Host,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<(cpal::Device, SupportedStreamConfig, bool), AudioError> {
        let (mut device, mut is_fallback) = Self::select_device(
            device_type,
            host,
            preferred_device_id,
            preferred_device_name,
        )?;

        let (stream_config, _) = match Self::pick_best_stream_config(device_type, &device) {
            Ok(stream_config) => stream_config,
            Err(err) => {
                tracing::warn!(?err, device = ?DeviceDebug(&device), "Failed to pick stream config for preferred device, picking best fallback device");

                let devices = Self::host_devices(device_type, host)?;
                let mut best_fallback: Option<(
                    cpal::Device,
                    SupportedStreamConfig,
                    StreamConfigScore,
                )> = None;

                for dev in devices {
                    if let Ok((config, score)) = Self::pick_best_stream_config(device_type, &dev) {
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
                    tracing::info!(device = ?DeviceDebug(&dev), ?config, "Selected fallback device");
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

    #[instrument(level = "trace", err, skip(host), fields(host = ?HostDebug(host)))]
    fn host_devices(
        device_type: DeviceType,
        host: &cpal::Host,
    ) -> Result<Vec<cpal::Device>, AudioError> {
        match device_type {
            DeviceType::Input => Ok(host
                .input_devices()
                .context("Failed to enumerate input devices")?
                .collect()),
            DeviceType::Output => Ok(host
                .output_devices()
                .context("Failed to enumerate output devices")?
                .collect()),
        }
    }

    #[instrument(level = "trace", err, skip(host), fields(host = ?HostDebug(host)))]
    fn select_device(
        device_type: DeviceType,
        host: &cpal::Host,
        preferred_device_id: Option<&str>,
        preferred_device_name: Option<&str>,
    ) -> Result<(cpal::Device, bool), AudioError> {
        if let Some(id_str) = preferred_device_id {
            if let Ok(id) = id_str.parse::<DeviceId>()
                && let Some(device) = host.device_by_id(&id)
            {
                tracing::trace!(device = ?DeviceDebug(&device), "Selected device by ID");
                return Ok((device, false));
            }
            tracing::debug!(
                id = id_str,
                "Stored device ID no longer available, falling back to name matching"
            );
        }

        // Fall back to name-based matching (backwards compat with old configs)
        if let Some(name) = preferred_device_name {
            let devices = Self::host_devices(device_type, host)?;

            // Exact case-insensitive match against all device identifiers.
            // Checking multiple identifiers ensures backwards compatibility with
            // configs that stored names from older cpal versions (e.g. ALSA pcm_id
            // or WASAPI FriendlyName).
            if let Some(device) = devices.iter().find(|d| {
                device_identifiers(d)
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(name))
            }) {
                tracing::trace!(device = ?DeviceDebug(device), "Selected preferred device");
                return Ok((device.clone(), false));
            }

            if let Some(device) = devices.iter().find(|d| {
                let name_lower = name.to_lowercase();
                device_identifiers(d)
                    .iter()
                    .any(|n| n.to_lowercase().contains(&name_lower))
            }) {
                tracing::trace!(device = ?DeviceDebug(device), "Selected preferred device (based on substring match)");
                return Ok((device.clone(), false));
            }
        }

        let device = match device_type {
            DeviceType::Input => host
                .default_input_device()
                .context("Failed to get default input device")?,
            DeviceType::Output => host
                .default_output_device()
                .context("Failed to get default output device")?,
        };
        tracing::trace!(device = ?DeviceDebug(&device), "Selected default device");
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
    fn has_supported_configs(device_type: DeviceType, device: &cpal::Device) -> bool {
        if let Ok(desc) = device.description() {
            match (device_type, desc.direction()) {
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
            .is_ok_and(|mut i| i.next().is_some())
    }

    #[instrument(level = "trace", err, skip(device), fields(device = ?DeviceDebug(device)))]
    fn pick_best_stream_config(
        device_type: DeviceType,
        device: &cpal::Device,
    ) -> Result<(SupportedStreamConfig, StreamConfigScore), AudioError> {
        let (configs, preferred_channels): (Vec<SupportedStreamConfigRange>, u16) =
            match device_type {
                DeviceType::Input => (
                    device
                        .supported_input_configs()
                        .context("Failed to get supported input configs")?
                        .collect(),
                    1,
                ),
                DeviceType::Output => (
                    device
                        .supported_output_configs()
                        .context("Failed to get supported output configs")?
                        .collect(),
                    2,
                ),
            };

        let mut best: Option<(SupportedStreamConfigRange, StreamConfigScore)> = None;

        for range in configs {
            let score = Self::score_stream_config_range(&range, preferred_channels);
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
            best.ok_or_else(|| anyhow::anyhow!("No supported stream config found"))?;
        let sample_rate =
            Self::closest_sample_rate(range.min_sample_rate(), range.max_sample_rate());

        tracing::trace!(?range, ?score, ?sample_rate, "Picked best stream config");
        Ok((range.with_sample_rate(sample_rate), score))
    }

    fn score_stream_config_range(
        range: &SupportedStreamConfigRange,
        preferred_channels: u16,
    ) -> StreamConfigScore {
        let sample_rate_distance =
            Self::sample_rate_distance(range.min_sample_rate(), range.max_sample_rate());

        let channels_distance = range.channels().abs_diff(preferred_channels);

        let format_preference = match range.sample_format() {
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
}

/// Returns the human-readable display name for an audio device via its description.
/// Includes the driver name in parentheses when available and different from the
/// device name, which helps disambiguate devices that share the same generic name
/// (e.g. multiple "USB Audio, USB Audio" entries on ALSA).
fn device_display_name(device: &cpal::Device) -> String {
    let Ok(desc) = device.description() else {
        return String::new();
    };
    let name = desc.name();
    match desc.driver() {
        Some(driver) if !driver.eq_ignore_ascii_case(name) => {
            format!("{name} ({driver})")
        }
        _ => name.to_string(),
    }
}

/// Returns all identifying strings for a device, used for backwards-compatible name matching.
/// Includes the display name, description name, driver name, and any extended description lines.
/// This ensures that device names stored in older configurations (which may have used
/// different naming schemes per platform) can still match the correct device, and that
/// new display names (which combine name + driver) also match correctly.
fn device_identifiers(device: &cpal::Device) -> Vec<String> {
    let mut ids = Vec::new();

    // Include the composite display name first so exact match hits it directly.
    let display = device_display_name(device);
    if !display.is_empty() {
        ids.push(display);
    }

    if let Ok(desc) = device.description() {
        let name = desc.name().to_string();
        if !ids.contains(&name) {
            ids.push(name);
        }
        if let Some(driver) = desc.driver() {
            let driver = driver.to_string();
            if !ids.contains(&driver) {
                ids.push(driver);
            }
        }
        for line in desc.extended() {
            if !ids.contains(line) {
                ids.push(line.clone());
            }
        }
    }
    ids
}

/// Returns the stable device ID as a string, if available.
pub fn device_id_string(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
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

struct DeviceDebug<'a>(&'a cpal::Device);

impl<'a> Debug for DeviceDebug<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("name", &device_display_name(self.0))
            .field(
                "id",
                &self.0.id().map(|id| id.to_string()).unwrap_or_default(),
            )
            .finish()
    }
}

struct HostDebug<'a>(&'a cpal::Host);

impl<'a> Debug for HostDebug<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Host").field(&self.0.id().name()).finish()
    }
}
