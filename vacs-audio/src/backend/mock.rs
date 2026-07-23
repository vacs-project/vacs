use super::{
    AudioBackend, AudioDevice, AudioHost, AudioStream, DeviceDirection, SampleFormat, StreamConfig,
    StreamConfigRange,
};
use crate::error::AudioError;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::{Duration, Instant};

const MOCK_TICK_MS: u64 = 20;

/// A mock audio backend that provides fake devices producing silence.
/// Useful for running tests without real audio hardware.
#[derive(Default)]
pub struct MockBackend {
    config: MockBackendConfig,
}

/// Configuration for the mock backend.
///
/// Note that the client opens an output stream at startup: a configuration
/// without output devices makes application startup fail with
/// "No default output device".
#[derive(serde::Serialize, serde::Deserialize)]
pub struct MockBackendConfig {
    pub host_name: String,
    pub input_devices: Vec<MockDeviceConfig>,
    pub output_devices: Vec<MockDeviceConfig>,
}

/// Configuration for a single mock device.
#[derive(serde::Serialize, serde::Deserialize)]
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
                .map(|config| MockDevice::new(config, DeviceDirection::Input))
                .collect(),
            output_devices: self
                .config
                .output_devices
                .iter()
                .map(|config| MockDevice::new(config, DeviceDirection::Output))
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

    fn host_names(&self) -> Vec<String> {
        vec![self.config.host_name.clone()]
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
    direction: DeviceDirection,
}

impl MockDevice {
    fn new(config: &MockDeviceConfig, direction: DeviceDirection) -> Self {
        Self {
            name: config.name.clone(),
            id: config.id.clone(),
            min_sample_rate: config.min_sample_rate,
            max_sample_rate: config.max_sample_rate,
            channels: config.channels,
            direction,
        }
    }

    fn config_range(&self) -> StreamConfigRange {
        StreamConfigRange {
            channels: self.channels,
            min_sample_rate: self.min_sample_rate,
            max_sample_rate: self.max_sample_rate,
            sample_format: SampleFormat::F32,
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

    fn supports_direction(&self, direction: DeviceDirection) -> bool {
        self.direction == direction
    }

    fn supported_input_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError> {
        match self.direction {
            DeviceDirection::Input => Ok(vec![self.config_range()]),
            _ => Ok(Vec::new()),
        }
    }

    fn supported_output_configs(&self) -> Result<Vec<StreamConfigRange>, AudioError> {
        match self.direction {
            DeviceDirection::Output => Ok(vec![self.config_range()]),
            _ => Ok(Vec::new()),
        }
    }

    fn build_input_stream_f32(
        &self,
        config: &StreamConfig,
        mut data_callback: Box<dyn FnMut(&[f32]) + Send + 'static>,
        _error_callback: Box<dyn FnMut(AudioError) + Send + 'static>,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        spawn_mock_tick_thread("mock-input-stream", buffer_len(config), move |buf| {
            data_callback(buf);
        })
    }

    fn build_output_stream_f32(
        &self,
        config: &StreamConfig,
        mut data_callback: Box<dyn FnMut(&mut [f32]) + Send + 'static>,
        _error_callback: Box<dyn FnMut(AudioError) + Send + 'static>,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        spawn_mock_tick_thread("mock-output-stream", buffer_len(config), move |buf| {
            data_callback(buf);
            // Output discarded
        })
    }

    fn identifiers(&self) -> Vec<String> {
        vec![self.name.clone(), self.id.clone()]
    }
}

fn buffer_len(config: &StreamConfig) -> usize {
    let frame_size = (config.sample_rate as usize * MOCK_TICK_MS as usize) / 1000;
    frame_size * config.channels as usize
}

/// Spawns the tick thread shared by mock input and output streams.
///
/// The thread waits until [`AudioStream::play`] is called, then invokes
/// `tick` with a zeroed buffer every [`MOCK_TICK_MS`] on a fixed cadence.
/// It exits when the owning [`MockStream`] is dropped: dropping disconnects
/// both channels, which ends the play gate and the stop check alike.
fn spawn_mock_tick_thread(
    thread_name: &str,
    buf_len: usize,
    mut tick: impl FnMut(&mut [f32]) + Send + 'static,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
    let (play_tx, play_rx) = std_mpsc::channel::<()>();

    thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            // Wait for play(); a disconnect means the stream was dropped
            // without ever playing, so the thread must not start ticking.
            if play_rx.recv().is_err() {
                return;
            }

            let mut buf = vec![0.0f32; buf_len];
            let mut next_tick = Instant::now();
            loop {
                // Stop on an explicit signal as well as on disconnect (the
                // stream was dropped); only an empty channel keeps running.
                if !matches!(stop_rx.try_recv(), Err(std_mpsc::TryRecvError::Empty)) {
                    break;
                }
                buf.fill(0.0);
                tick(&mut buf);

                // Fixed cadence: callback duration must not drift the tick.
                next_tick += Duration::from_millis(MOCK_TICK_MS);
                thread::sleep(next_tick.saturating_duration_since(Instant::now()));
            }
        })
        .map_err(|e| AudioError::Other(anyhow::anyhow!(e)))?;

    Ok(Box::new(MockStream {
        _stop_tx: stop_tx,
        play_tx: Some(play_tx),
    }))
}

/// Dropping the stream disconnects both channels, which stops the tick
/// thread: a pending play gate unblocks with an error and a running loop
/// observes the stop channel as disconnected.
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
