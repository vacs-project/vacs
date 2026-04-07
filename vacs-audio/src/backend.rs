pub mod cpal;

#[cfg(feature = "mock-audio")]
pub mod mock;

use crate::error::AudioError;
use ::cpal::SampleFormat;

/// Boxed callback for receiving input audio data (f32 samples).
pub type InputDataCallback = Box<dyn FnMut(&[f32]) + Send + 'static>;
/// Boxed callback for providing output audio data (f32 samples).
pub type OutputDataCallback = Box<dyn FnMut(&mut [f32]) + Send + 'static>;
/// Boxed callback for receiving stream errors.
pub type ErrorCallback = Box<dyn FnMut(AudioError) + Send + 'static>;

/// Device description without depending on cpal types.
pub struct DeviceDescription {
    pub name: String,
    pub driver: Option<String>,
    pub direction: DeviceDirection,
}

/// The direction a device supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDirection {
    Input,
    Output,
    Duplex,
    Unknown,
}

/// Represents a supported stream configuration range.
pub struct StreamConfigRange {
    pub channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub sample_format: SampleFormat,
}

impl StreamConfigRange {
    pub fn with_sample_rate(&self, sample_rate: u32) -> StreamConfig {
        StreamConfig {
            channels: self.channels,
            sample_rate,
            sample_format: self.sample_format,
            buffer_size: BufferSize::Default,
        }
    }
}

/// A concrete stream configuration.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: SampleFormat,
    pub buffer_size: BufferSize,
}

/// Buffer size for audio streams.
#[derive(Debug, Clone, Copy)]
pub enum BufferSize {
    Default,
    Fixed(u32),
}

/// Abstraction over the audio host system (e.g. cpal).
pub trait AudioBackend: Send + Sync + 'static {
    fn available_hosts(&self) -> Vec<Box<dyn AudioHost>>;
    fn default_host(&self) -> Box<dyn AudioHost>;
    fn host_by_name(&self, name: &str) -> Option<Box<dyn AudioHost>>;
}

/// An audio host that can enumerate devices (e.g. ALSA, PulseAudio, WASAPI).
pub trait AudioHost: Send + Sync {
    fn name(&self) -> &str;
    fn input_devices(&self) -> Result<Vec<Box<dyn AudioDevice>>, AudioError>;
    fn output_devices(&self) -> Result<Vec<Box<dyn AudioDevice>>, AudioError>;
    fn default_input_device(&self) -> Option<Box<dyn AudioDevice>>;
    fn default_output_device(&self) -> Option<Box<dyn AudioDevice>>;
    fn device_by_id(&self, id: &str) -> Option<Box<dyn AudioDevice>>;
}

/// An audio device that can build streams.
pub trait AudioDevice: Send + Sync {
    fn name(&self) -> String;
    fn id(&self) -> Option<String>;
    fn description(&self) -> Result<DeviceDescription, AudioError>;
    fn supported_input_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError>;
    fn supported_output_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError>;

    /// Build an input stream that always delivers f32 samples.
    /// Sample format conversion (i16/u16 -> f32) is the implementation's responsibility.
    fn build_input_stream_f32(
        &self,
        config: &StreamConfig,
        data_callback: InputDataCallback,
        error_callback: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError>;

    /// Build an output stream that always works with f32 samples.
    fn build_output_stream_f32(
        &self,
        config: &StreamConfig,
        data_callback: OutputDataCallback,
        error_callback: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError>;

    fn clone_boxed(&self) -> Box<dyn AudioDevice>;

    /// Returns all possible identifiers for this device (display name, raw name, driver, etc.)
    fn identifiers(&self) -> Vec<String>;
}

/// A running audio stream. Drop to stop.
pub trait AudioStream: Send + Sync {
    fn play(&self) -> Result<(), AudioError>;
}
