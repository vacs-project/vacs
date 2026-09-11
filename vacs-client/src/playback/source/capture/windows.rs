use crate::playback::source::capture::{CaptureSource, LoopbackCapture, LoopbackEvent};
use crate::playback::{PlaybackError, TapId};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fmt::Display;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use tokio::sync::mpsc;
use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};

/// Channel capacity for the capture-thread → async forwarder mpsc.
const CHANNEL_CAPACITY: usize = 1024;
const CHUNKSIZE: usize = 1024;

const SAMPLE_RATE: usize = 48000;
const CHANNELS: usize = 2;

pub struct WindowsApplicationCapture {
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WindowsApplicationCapture {
    fn start_inner(
        source: CaptureSource,
    ) -> Result<(Self, mpsc::Receiver<LoopbackEvent>), PlaybackError> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (shutdown, thread) = spawn_wasapi_thread(source, tx)?;
        Ok((
            Self {
                shutdown,
                thread: Some(thread),
            },
            rx,
        ))
    }

    fn stop_inner(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && let Err(err) = thread.join()
        {
            log::warn!("WASAPI loopback audio capture thread panicked: {err:?}");
        }
    }
}

impl LoopbackCapture for WindowsApplicationCapture {
    fn start(
        source: CaptureSource,
    ) -> Result<(Self, mpsc::Receiver<LoopbackEvent>), PlaybackError> {
        Self::start_inner(source)
    }

    fn stop(&mut self) {
        self.stop_inner();
    }
}

impl Drop for WindowsApplicationCapture {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

fn spawn_wasapi_thread(
    source: CaptureSource,
    tx: mpsc::Sender<LoopbackEvent>,
) -> Result<(Arc<AtomicBool>, JoinHandle<()>), PlaybackError> {
    let refreshes = RefreshKind::nothing().with_processes(ProcessRefreshKind::everything());
    let system = System::new_with_specifics(refreshes);
    let source_app_name = source.to_string();
    let process_ids = system.processes_by_name(OsStr::new(source_app_name.as_str()));
    let mut process_id = 0;
    for process in process_ids {
        // Note: When capturing audio windows allows you to capture an app's entire process tree,
        // however you must ensure you use the parent as the target process ID
        process_id = process.parent().unwrap_or(process.pid()).as_u32();
    }

    if process_id == 0 {
        return Err(PlaybackError::Source(format!(
            "Can not find {source_app_name} process"
        )));
    }

    let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<Arc<AtomicBool>, String>>(1);

    let thread = std::thread::Builder::new()
        .name("vacs-playback-wasapi".to_owned())
        .spawn(move || run_main_loop(tx, process_id, init_tx))
        .map_err(PlaybackError::Io)?;

    let stop_requested = match init_rx.recv() {
        Ok(Ok(stop_requested)) => stop_requested,
        Ok(Err(err)) => return Err(PlaybackError::Source(err)),
        Err(_) => {
            return Err(PlaybackError::Source(
                "WASAPI loopback audio capture thread exited before init".to_owned(),
            ));
        }
    };

    Ok((stop_requested, thread))
}

fn run_main_loop(
    tx: mpsc::Sender<LoopbackEvent>,
    process_id: u32,
    init_tx: std::sync::mpsc::SyncSender<Result<Arc<AtomicBool>, String>>,
) {
    let wave_format = WaveFormat::new(32, 32, &SampleType::Float, SAMPLE_RATE, CHANNELS, None);
    let blockalign = wave_format.get_blockalign();

    let mut audio_client = match AudioClient::new_application_loopback_client(process_id, true) {
        Ok(a) => a,
        Err(err) => {
            let _ = init_tx.send(Err(format!(
                "WASAPI AudioClient::new_application_loopback_client failed: {err}"
            )));
            return;
        }
    };

    if let Err(err) = audio_client.initialize_client(
        &wave_format,
        &Direction::Capture,
        &StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 0,
        },
    ) {
        let _ = init_tx.send(Err(format!(
            "WASAPI audio_client.initialize_client failed: {err}"
        )));
        return;
    }

    let h_event = match audio_client.set_get_eventhandle() {
        Ok(h) => h,
        Err(err) => {
            let _ = init_tx.send(Err(format!(
                "WASAPI audio_client.set_get_eventhandle failed: {err}"
            )));
            return;
        }
    };

    let capture_client = match audio_client.get_audiocaptureclient() {
        Ok(h) => h,
        Err(err) => {
            let _ = init_tx.send(Err(format!(
                "WASAPI audio_client.get_audiocaptureclient failed: {err}"
            )));
            return;
        }
    };

    let mut sample_queue: VecDeque<u8> = VecDeque::new();

    if let Err(err) = tx.try_send(LoopbackEvent::Opened {
        tap: TapId::Merged,
        sample_rate: SAMPLE_RATE as u32,
        channels: CHANNELS as u16,
    }) {
        log::warn!("failed to send opened event: {err}");
    }

    if let Err(err) = audio_client.start_stream() {
        let _ = init_tx.send(Err(format!(
            "WASAPI audio_client.start_stream failed: {err}"
        )));
        return;
    }

    let stop_requested = Arc::new(AtomicBool::new(false));

    if init_tx.send(Ok(Arc::clone(&stop_requested))).is_err() {
        log::warn!("caller disappeared before WASAPI init completed");
        return;
    }

    while !stop_requested.load(Ordering::Relaxed) {
        while sample_queue.len() > (blockalign as usize * CHUNKSIZE) {
            if stop_requested.load(Ordering::Relaxed) {
                break;
            }
            let mut chunk = vec![0u8; blockalign as usize * CHUNKSIZE];
            for element in chunk.iter_mut() {
                *element = sample_queue.pop_front().unwrap_or_default();
            }
            if let Err(err) = tx.try_send(LoopbackEvent::Frame {
                tap: TapId::Merged,
                samples: bytes_to_f32(&chunk).into(),
                captured_at: std::time::Instant::now(),
            }) {
                log::warn!("failed to send frame: {err}");
            };
        }

        let new_frames = match capture_client.get_next_packet_size() {
            Ok(n) => n.unwrap_or(0),
            Err(err) => {
                log::warn!("WASAPI capture_client.get_next_packet_size failed: {err}");
                continue;
            }
        };
        let additional = (new_frames as usize * blockalign as usize)
            .saturating_sub(sample_queue.capacity() - sample_queue.len());
        sample_queue.reserve(additional);
        if new_frames > 0
            && let Err(err) = capture_client.read_from_device_to_deque(&mut sample_queue)
        {
            log::warn!("WASAPI capture_client.read_from_device_to_deque failed: {err}");
            continue;
        }

        if h_event.wait_for_event(3000).is_err() {
            break;
        }
    }

    if let Err(err) = tx.try_send(LoopbackEvent::Closed { tap: TapId::Merged }) {
        log::warn!("failed to send closed event: {err}");
    }

    if let Err(err) = audio_client.stop_stream() {
        log::warn!("WASAPI audio_client.stop_stream failed: {err}");
    }

    log::info!("Windows loopback audio capture thread exiting");
}

fn bytes_to_f32(samples: &[u8]) -> Vec<f32> {
    samples
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

impl Display for CaptureSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureSource::TrackAudio => f.write_str("trackaudio.exe"),
            CaptureSource::AudioForVatsim => f.write_str("AudioForVATSIM.exe"),
        }
    }
}
