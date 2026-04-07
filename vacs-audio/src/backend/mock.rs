use super::{
    AudioBackend, AudioDevice, AudioHost, AudioStream, DeviceDescription, DeviceDirection,
    StreamConfig, StreamConfigRange,
};
use crate::error::AudioError;
use ::cpal::SampleFormat;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

const MOCK_TICK_MS: u64 = 20;

/// A mock audio backend that provides fake devices producing silence.
/// Useful for running tests without real audio hardware.
#[derive(Default)]
pub struct MockBackend {
    config: MockBackendConfig,
}

/// Configuration for the mock backend.
pub struct MockBackendConfig {
    pub host_name: String,
    pub input_devices: Vec<MockDeviceConfig>,
    pub output_devices: Vec<MockDeviceConfig>,
}

/// Configuration for a single mock device.
pub struct MockDeviceConfig {
    pub name: String,
    pub id: String,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub channels: u16,
}

impl Default for MockBackendConfig {
    fn default() -> Self {
        Self {
            host_name: "MockHost".to_string(),
            input_devices: vec![MockDeviceConfig {
                name: "Mock Microphone".to_string(),
                id: "mock-input-0".to_string(),
                min_sample_rate: 48000,
                max_sample_rate: 48000,
                channels: 1,
            }],
            output_devices: vec![MockDeviceConfig {
                name: "Mock Speaker".to_string(),
                id: "mock-output-0".to_string(),
                min_sample_rate: 48000,
                max_sample_rate: 48000,
                channels: 2,
            }],
        }
    }
}

impl MockBackend {
    pub fn new(config: MockBackendConfig) -> Self {
        Self { config }
    }
}

impl AudioBackend for MockBackend {
    fn available_hosts(&self) -> Vec<Box<dyn AudioHost>> {
        vec![self.default_host()]
    }

    fn default_host(&self) -> Box<dyn AudioHost> {
        Box::new(MockHost {
            name: self.config.host_name.clone(),
            input_devices: self
                .config
                .input_devices
                .iter()
                .map(MockDevice::new)
                .collect(),
            output_devices: self
                .config
                .output_devices
                .iter()
                .map(MockDevice::new)
                .collect(),
        })
    }

    fn host_by_name(&self, name: &str) -> Option<Box<dyn AudioHost>> {
        if self.config.host_name.eq_ignore_ascii_case(name) {
            Some(self.default_host())
        } else {
            None
        }
    }
}

struct MockHost {
    name: String,
    input_devices: Vec<MockDevice>,
    output_devices: Vec<MockDevice>,
}

impl AudioHost for MockHost {
    fn name(&self) -> &str {
        &self.name
    }

    fn input_devices(&self) -> Result<Vec<Box<dyn AudioDevice>>, AudioError> {
        Ok(self
            .input_devices
            .iter()
            .map(|d| Box::new(d.clone()) as Box<dyn AudioDevice>)
            .collect())
    }

    fn output_devices(&self) -> Result<Vec<Box<dyn AudioDevice>>, AudioError> {
        Ok(self
            .output_devices
            .iter()
            .map(|d| Box::new(d.clone()) as Box<dyn AudioDevice>)
            .collect())
    }

    fn default_input_device(&self) -> Option<Box<dyn AudioDevice>> {
        self.input_devices
            .first()
            .map(|d| Box::new(d.clone()) as Box<dyn AudioDevice>)
    }

    fn default_output_device(&self) -> Option<Box<dyn AudioDevice>> {
        self.output_devices
            .first()
            .map(|d| Box::new(d.clone()) as Box<dyn AudioDevice>)
    }

    fn device_by_id(&self, id: &str) -> Option<Box<dyn AudioDevice>> {
        self.input_devices
            .iter()
            .chain(self.output_devices.iter())
            .find(|d| d.id == id)
            .map(|d| Box::new(d.clone()) as Box<dyn AudioDevice>)
    }
}

#[derive(Clone)]
struct MockDevice {
    name: String,
    id: String,
    min_sample_rate: u32,
    max_sample_rate: u32,
    channels: u16,
}

impl MockDevice {
    fn new(config: &MockDeviceConfig) -> Self {
        Self {
            name: config.name.clone(),
            id: config.id.clone(),
            min_sample_rate: config.min_sample_rate,
            max_sample_rate: config.max_sample_rate,
            channels: config.channels,
        }
    }
}

impl AudioDevice for MockDevice {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn id(&self) -> Option<String> {
        Some(self.id.clone())
    }

    fn description(&self) -> Result<DeviceDescription, AudioError> {
        Ok(DeviceDescription {
            name: self.name.clone(),
            driver: None,
            direction: DeviceDirection::Duplex,
        })
    }

    fn supported_input_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError> {
        Ok(vec![StreamConfigRange {
            channels: self.channels,
            min_sample_rate: self.min_sample_rate,
            max_sample_rate: self.max_sample_rate,
            sample_format: SampleFormat::F32,
        }])
    }

    fn supported_output_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError> {
        Ok(vec![StreamConfigRange {
            channels: self.channels,
            min_sample_rate: self.min_sample_rate,
            max_sample_rate: self.max_sample_rate,
            sample_format: SampleFormat::F32,
        }])
    }

    fn build_input_stream_f32(
        &self,
        config: &StreamConfig,
        mut data_callback: Box<dyn FnMut(&[f32]) + Send + 'static>,
        _error_callback: Box<dyn FnMut(AudioError) + Send + 'static>,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let frame_size = (config.sample_rate as usize * MOCK_TICK_MS as usize) / 1000;
        let channels = config.channels as usize;
        let buf_len = frame_size * channels;

        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let (play_tx, play_rx) = std_mpsc::channel::<()>();

        thread::Builder::new()
            .name("mock-input-stream".to_string())
            .spawn(move || {
                // Wait for play() to be called
                let _ = play_rx.recv();

                let silence = vec![0.0f32; buf_len];
                loop {
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    data_callback(&silence);
                    thread::sleep(Duration::from_millis(MOCK_TICK_MS));
                }
            })
            .map_err(|e| AudioError::Other(anyhow::anyhow!(e)))?;

        Ok(Box::new(MockStream {
            _stop_tx: stop_tx,
            play_tx: Some(play_tx),
        }))
    }

    fn build_output_stream_f32(
        &self,
        config: &StreamConfig,
        mut data_callback: Box<dyn FnMut(&mut [f32]) + Send + 'static>,
        _error_callback: Box<dyn FnMut(AudioError) + Send + 'static>,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let frame_size = (config.sample_rate as usize * MOCK_TICK_MS as usize) / 1000;
        let channels = config.channels as usize;
        let buf_len = frame_size * channels;

        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let (play_tx, play_rx) = std_mpsc::channel::<()>();

        thread::Builder::new()
            .name("mock-output-stream".to_string())
            .spawn(move || {
                // Wait for play() to be called
                let _ = play_rx.recv();

                let mut buf = vec![0.0f32; buf_len];
                loop {
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    buf.fill(0.0);
                    data_callback(&mut buf);
                    // Output discarded
                    thread::sleep(Duration::from_millis(MOCK_TICK_MS));
                }
            })
            .map_err(|e| AudioError::Other(anyhow::anyhow!(e)))?;

        Ok(Box::new(MockStream {
            _stop_tx: stop_tx,
            play_tx: Some(play_tx),
        }))
    }

    fn clone_boxed(&self) -> Box<dyn AudioDevice> {
        Box::new(self.clone())
    }

    fn identifiers(&self) -> Vec<String> {
        vec![self.name.clone(), self.id.clone()]
    }
}

struct MockStream {
    _stop_tx: std_mpsc::Sender<()>,
    play_tx: Option<std_mpsc::Sender<()>>,
}

impl AudioStream for MockStream {
    fn play(&self) -> Result<(), AudioError> {
        if let Some(tx) = &self.play_tx {
            let _ = tx.send(());
        }
        Ok(())
    }
}

impl Drop for MockStream {
    fn drop(&mut self) {
        // stop_tx is dropped, which causes the thread to exit
        // when it next checks stop_rx.try_recv()
    }
}
