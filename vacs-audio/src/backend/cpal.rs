use super::SampleFormat;
use super::{
    AudioBackend, AudioDevice, AudioHost, AudioStream, BufferSize, DeviceDescription,
    DeviceDirection, ErrorCallback, InputDataCallback, OutputDataCallback, StreamConfig,
    StreamConfigRange,
};
use crate::error::AudioError;
use ::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ::cpal::{Sample, SizedSample};
use anyhow::Context;
use std::cell::{RefCell, RefMut};

impl From<::cpal::SampleFormat> for SampleFormat {
    fn from(format: ::cpal::SampleFormat) -> Self {
        match format {
            ::cpal::SampleFormat::F32 => Self::F32,
            ::cpal::SampleFormat::I16 => Self::I16,
            ::cpal::SampleFormat::U16 => Self::U16,
            _ => Self::Other,
        }
    }
}

/// Real audio backend backed by cpal.
pub struct CpalBackend;

impl CpalBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for CpalBackend {
    fn available_hosts(&self) -> Vec<Box<dyn AudioHost>> {
        ::cpal::available_hosts()
            .into_iter()
            .filter_map(|id| {
                ::cpal::host_from_id(id)
                    .ok()
                    .map(|h| Box::new(CpalHost(h)) as Box<dyn AudioHost>)
            })
            .collect()
    }

    fn default_host(&self) -> Box<dyn AudioHost> {
        Box::new(CpalHost(::cpal::default_host()))
    }

    fn host_by_name(&self, name: &str) -> Option<Box<dyn AudioHost>> {
        let hosts = ::cpal::available_hosts();
        let id = hosts
            .iter()
            .find(|id| id.name().eq_ignore_ascii_case(name))
            .or_else(|| {
                hosts
                    .iter()
                    .find(|id| id.name().to_lowercase().contains(&name.to_lowercase()))
            })?;
        ::cpal::host_from_id(*id)
            .ok()
            .map(|h| Box::new(CpalHost(h)) as Box<dyn AudioHost>)
    }

    fn host_names(&self) -> Vec<String> {
        // Host IDs carry their names statically; listing names must not
        // instantiate the host backends themselves.
        ::cpal::available_hosts()
            .iter()
            .map(|id| id.name().to_string())
            .collect()
    }
}

struct CpalHost(::cpal::Host);

impl AudioHost for CpalHost {
    fn name(&self) -> &str {
        self.0.id().name()
    }

    fn input_devices(&self) -> Result<Vec<Box<dyn AudioDevice>>, AudioError> {
        Ok(self
            .0
            .input_devices()
            .context("Failed to enumerate input devices")?
            .map(|d| Box::new(CpalDevice(d)) as Box<dyn AudioDevice>)
            .collect())
    }

    fn output_devices(&self) -> Result<Vec<Box<dyn AudioDevice>>, AudioError> {
        Ok(self
            .0
            .output_devices()
            .context("Failed to enumerate output devices")?
            .map(|d| Box::new(CpalDevice(d)) as Box<dyn AudioDevice>)
            .collect())
    }

    fn default_input_device(&self) -> Option<Box<dyn AudioDevice>> {
        self.0
            .default_input_device()
            .map(|d| Box::new(CpalDevice(d)) as Box<dyn AudioDevice>)
    }

    fn default_output_device(&self) -> Option<Box<dyn AudioDevice>> {
        self.0
            .default_output_device()
            .map(|d| Box::new(CpalDevice(d)) as Box<dyn AudioDevice>)
    }

    fn device_by_id(&self, id: &str) -> Option<Box<dyn AudioDevice>> {
        let parsed = id.parse::<::cpal::DeviceId>().ok()?;
        self.0
            .device_by_id(&parsed)
            .map(|d| Box::new(CpalDevice(d)) as Box<dyn AudioDevice>)
    }
}

struct CpalDevice(::cpal::Device);

impl AudioDevice for CpalDevice {
    fn name(&self) -> String {
        cpal_device_display_name(&self.0)
    }

    fn id(&self) -> Option<String> {
        self.0.id().ok().map(|id| id.to_string())
    }

    fn description(&self) -> Result<DeviceDescription, AudioError> {
        let desc = self
            .0
            .description()
            .map_err(|e| AudioError::Other(anyhow::anyhow!(e)))?;
        let direction = match desc.direction() {
            ::cpal::device_description::DeviceDirection::Input => DeviceDirection::Input,
            ::cpal::device_description::DeviceDirection::Output => DeviceDirection::Output,
            ::cpal::device_description::DeviceDirection::Duplex => DeviceDirection::Duplex,
            _ => DeviceDirection::Unknown,
        };
        Ok(DeviceDescription {
            name: desc.name().to_string(),
            driver: desc.driver().map(|s| s.to_string()),
            direction,
        })
    }

    fn supported_input_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError> {
        Ok(self
            .0
            .supported_input_configs()
            .context("Failed to get supported input configs")?
            .map(|r| cpal_range_to_config_range(&r))
            .collect())
    }

    fn supported_output_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError> {
        Ok(self
            .0
            .supported_output_configs()
            .context("Failed to get supported output configs")?
            .map(|r| cpal_range_to_config_range(&r))
            .collect())
    }

    fn build_input_stream_f32(
        &self,
        config: &StreamConfig,
        data_callback: Box<dyn FnMut(&[f32]) + Send + 'static>,
        error_callback: Box<dyn FnMut(AudioError) + Send + 'static>,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let cpal_config = to_cpal_config(config);
        let stream = match config.sample_format {
            SampleFormat::F32 => {
                build_cpal_input_f32_native(&self.0, &cpal_config, data_callback, error_callback)?
            }
            SampleFormat::I16 => build_cpal_input_f32_convert::<i16>(
                &self.0,
                &cpal_config,
                config.buffer_size,
                data_callback,
                error_callback,
            )?,
            SampleFormat::U16 => build_cpal_input_f32_convert::<u16>(
                &self.0,
                &cpal_config,
                config.buffer_size,
                data_callback,
                error_callback,
            )?,
            other => {
                return Err(AudioError::Other(anyhow::anyhow!(
                    "Unsupported input sample format: {other:?}"
                )));
            }
        };
        Ok(Box::new(CpalStream(stream)))
    }

    fn build_output_stream_f32(
        &self,
        config: &StreamConfig,
        data_callback: Box<dyn FnMut(&mut [f32]) + Send + 'static>,
        error_callback: Box<dyn FnMut(AudioError) + Send + 'static>,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let cpal_config = to_cpal_config(config);
        let stream = match config.sample_format {
            SampleFormat::F32 => {
                build_cpal_output_f32_native(&self.0, &cpal_config, data_callback, error_callback)?
            }
            SampleFormat::I16 => build_cpal_output_f32_convert::<i16>(
                &self.0,
                &cpal_config,
                config.buffer_size,
                data_callback,
                error_callback,
            )?,
            SampleFormat::U16 => build_cpal_output_f32_convert::<u16>(
                &self.0,
                &cpal_config,
                config.buffer_size,
                data_callback,
                error_callback,
            )?,
            other => {
                return Err(AudioError::Other(anyhow::anyhow!(
                    "Unsupported output sample format: {other:?}"
                )));
            }
        };
        Ok(Box::new(CpalStream(stream)))
    }

    fn clone_boxed(&self) -> Box<dyn AudioDevice> {
        Box::new(CpalDevice(self.0.clone()))
    }

    fn identifiers(&self) -> Vec<String> {
        cpal_device_identifiers(&self.0)
    }
}

struct CpalStream(::cpal::Stream);

impl AudioStream for CpalStream {
    fn play(&self) -> Result<(), AudioError> {
        self.0.play()?;
        Ok(())
    }
}

fn cpal_range_to_config_range(range: &::cpal::SupportedStreamConfigRange) -> StreamConfigRange {
    StreamConfigRange {
        channels: range.channels(),
        min_sample_rate: range.min_sample_rate(),
        max_sample_rate: range.max_sample_rate(),
        sample_format: range.sample_format().into(),
    }
}

fn to_cpal_config(config: &StreamConfig) -> ::cpal::StreamConfig {
    ::cpal::StreamConfig {
        channels: config.channels,
        sample_rate: config.sample_rate,
        buffer_size: match config.buffer_size {
            BufferSize::Default => ::cpal::BufferSize::Default,
            BufferSize::Fixed(n) => ::cpal::BufferSize::Fixed(n),
        },
    }
}

/// Xruns are transient (samples dropped on a live stream); surfacing them to
/// the stream's recovery logic would restart the stream and only drop more
/// audio, so they are filtered out before the backend-agnostic callback.
fn filter_xruns(mut error_callback: ErrorCallback) -> impl FnMut(::cpal::Error) + Send + 'static {
    move |err| {
        if matches!(err.kind(), ::cpal::ErrorKind::Xrun) {
            tracing::debug!("Stream xrun, samples dropped");
            return;
        }
        error_callback(err.into());
    }
}

fn build_cpal_input_f32_native(
    device: &::cpal::Device,
    config: &::cpal::StreamConfig,
    mut data_callback: InputDataCallback,
    error_callback: ErrorCallback,
) -> Result<::cpal::Stream, AudioError> {
    Ok(device.build_input_stream::<f32, _, _>(
        *config,
        move |input, _info| data_callback(input),
        filter_xruns(error_callback),
        None,
    )?)
}

fn build_cpal_input_f32_convert<T>(
    device: &::cpal::Device,
    config: &::cpal::StreamConfig,
    buffer_size: BufferSize,
    mut data_callback: InputDataCallback,
    error_callback: ErrorCallback,
) -> Result<::cpal::Stream, AudioError>
where
    T: ::cpal::Sample<Float = f32> + SizedSample + 'static,
{
    let buf = conversion_buffer(buffer_size);

    Ok(device.build_input_stream::<T, _, _>(
        *config,
        move |input: &[T], _info| {
            let mut b = conversion_buffer_for_len(&buf, input.len());
            for (dst, &src) in b.iter_mut().zip(input.iter()) {
                *dst = src.to_float_sample();
            }
            data_callback(&b);
        },
        filter_xruns(error_callback),
        None,
    )?)
}

fn build_cpal_output_f32_native(
    device: &::cpal::Device,
    config: &::cpal::StreamConfig,
    mut data_callback: OutputDataCallback,
    error_callback: ErrorCallback,
) -> Result<::cpal::Stream, AudioError> {
    Ok(device.build_output_stream::<f32, _, _>(
        *config,
        move |output, _info| data_callback(output),
        filter_xruns(error_callback),
        None,
    )?)
}

fn build_cpal_output_f32_convert<T>(
    device: &::cpal::Device,
    config: &::cpal::StreamConfig,
    buffer_size: BufferSize,
    mut data_callback: OutputDataCallback,
    error_callback: ErrorCallback,
) -> Result<::cpal::Stream, AudioError>
where
    T: SizedSample + ::cpal::FromSample<f32> + 'static,
{
    let buf = conversion_buffer(buffer_size);

    Ok(device.build_output_stream::<T, _, _>(
        *config,
        move |output: &mut [T], _info| {
            let mut b = conversion_buffer_for_len(&buf, output.len());
            data_callback(&mut b);
            for (dst, &src) in output.iter_mut().zip(b.iter()) {
                *dst = src.to_sample::<T>();
            }
        },
        filter_xruns(error_callback),
        None,
    )?)
}

/// Creates the scratch buffer reused across stream callbacks for sample
/// format conversion, pre-reserved when the buffer size is known.
fn conversion_buffer(buffer_size: BufferSize) -> RefCell<Vec<f32>> {
    let buf = RefCell::new(Vec::new());
    if let BufferSize::Fixed(n) = buffer_size {
        buf.borrow_mut().reserve(n as usize);
    }
    buf
}

/// Borrows the scratch buffer resized to the current callback length.
fn conversion_buffer_for_len(buf: &RefCell<Vec<f32>>, len: usize) -> RefMut<'_, Vec<f32>> {
    let mut b = buf.borrow_mut();
    if b.len() != len {
        b.resize(len, 0.0f32);
    }
    b
}

/// Returns the human-readable display name for an audio device via its description.
/// Includes the driver name in parentheses when available and different from the
/// device name, which helps disambiguate devices that share the same generic name
/// (e.g. multiple "USB Audio, USB Audio" entries on ALSA).
pub(crate) fn cpal_device_display_name(device: &::cpal::Device) -> String {
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
pub(crate) fn cpal_device_identifiers(device: &::cpal::Device) -> Vec<String> {
    let mut ids = Vec::new();

    let display = cpal_device_display_name(device);
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
            let line = line.to_string();
            if !ids.contains(&line) {
                ids.push(line);
            }
        }
    }
    ids
}
