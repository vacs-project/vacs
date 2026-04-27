//! Linux PipeWire loopback capture for the replay recorder.
//!
//! Connects to the user PipeWire socket, walks the registry for nodes matching
//! `application.name == "afv::speaker" | "afv::headset"` AND
//! `media.class == "Stream/Output/Audio"`, and creates one passive capture stream
//! per match. Each stream produces interleaved 32-bit float audio that is forwarded
//! through a [`tokio::sync::mpsc`] channel as [`LoopbackEvent`] values.
//!
//! The PipeWire main loop runs on a dedicated OS thread. All event delivery to async
//! consumers is non-blocking from the PipeWire side.

use super::{LoopbackCapture, LoopbackEvent};
use crate::replay::{ReplayError, TapId};
use pipewire::context::Context;
use pipewire::keys;
use pipewire::link::Link;
use pipewire::main_loop::MainLoop;
use pipewire::properties::properties;
use pipewire::spa::param::ParamType;
use pipewire::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pipewire::spa::pod::Pod;
use pipewire::spa::pod::serialize::PodSerializer;
use pipewire::spa::pod::{Object, Value};
use pipewire::spa::utils::{Direction, SpaTypes};
use pipewire::stream::{Stream, StreamFlags, StreamListener, StreamState as PwStreamState};
use pipewire::types::ObjectType;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;
use tokio::sync::mpsc;

/// Channel capacity for the capture-thread → async forwarder mpsc.
const CHANNEL_CAPACITY: usize = 1024;

const AFV_APP_HEADSET: &str = "afv::headset";
const AFV_APP_SPEAKER: &str = "afv::speaker";
const STREAM_OUTPUT_CLASS: &str = "Stream/Output/Audio";
/// Per-stream NODE_NAME prefix; the suffix is the afv target node id so we can
/// match our stream's own node global back to the corresponding [`Capture`].
const NODE_NAME_PREFIX: &str = "vacs-replay-tap-";

/// Boxed FnOnce used to ask the PipeWire main loop to quit.
type ShutdownFn = Box<dyn FnOnce() + Send>;

/// Handle to the running PipeWire capture thread that taps afv-native's output streams.
pub struct AfvNativePipewireCapture {
    shutdown: Option<ShutdownFn>,
    thread: Option<JoinHandle<()>>,
}

impl AfvNativePipewireCapture {
    /// Spawn the PipeWire main loop thread and start scanning for afv output streams.
    /// Returns the capture handle and the receiver for [`LoopbackEvent`]s.
    fn start_inner() -> Result<(Self, mpsc::Receiver<LoopbackEvent>), ReplayError> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (shutdown, thread) = spawn_pipewire_thread(tx)?;
        Ok((
            Self {
                shutdown: Some(shutdown),
                thread: Some(thread),
            },
            rx,
        ))
    }

    fn stop_inner(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown();
        }
        if let Some(thread) = self.thread.take()
            && let Err(err) = thread.join()
        {
            log::warn!("PipeWire capture thread panicked: {err:?}");
        }
    }
}

impl LoopbackCapture for AfvNativePipewireCapture {
    fn start() -> Result<(Self, mpsc::Receiver<LoopbackEvent>), ReplayError> {
        Self::start_inner()
    }

    fn stop(&mut self) {
        self.stop_inner();
    }
}

impl Drop for AfvNativePipewireCapture {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

/// Per-stream state stashed in `add_local_listener_with_user_data`.
struct CaptureUserData {
    tap: TapId,
    opened: bool,
    sample_rate: u32,
    channels: u16,
    tx: mpsc::Sender<LoopbackEvent>,
    node_id: u32,
}

struct Capture {
    target_node_id: u32,
    tap: TapId,
    // Owning the listeners keeps the callbacks alive.
    _stream: Stream,
    _listener: StreamListener<CaptureUserData>,
    /// Registry id of our own capture stream's node, populated when its node global
    /// arrives.
    own_node_id: Option<u32>,
    /// Output port global ids belonging to the afv target node.
    target_outputs: Vec<u32>,
    /// Input port global ids belonging to our capture stream's node.
    own_inputs: Vec<u32>,
    /// Active links from afv outputs to our inputs. Dropping these destroys the link.
    links: Vec<Link>,
}

fn spawn_pipewire_thread(
    tx: mpsc::Sender<LoopbackEvent>,
) -> Result<(ShutdownFn, JoinHandle<()>), ReplayError> {
    // We use a oneshot to surface init failures from the thread back to the caller
    // synchronously, so `AfvNativePipewireCapture::start` returns a meaningful error.
    let (init_tx, init_rx) = std::sync::mpsc::sync_channel::<Result<MainLoopHandle, String>>(1);
    let tx_thread = tx.clone();

    let thread = std::thread::Builder::new()
        .name("vacs-replay-pipewire".to_owned())
        .spawn(move || run_main_loop(tx_thread, init_tx))
        .map_err(ReplayError::Io)?;

    let handle = match init_rx.recv() {
        Ok(Ok(h)) => h,
        Ok(Err(err)) => return Err(ReplayError::Source(err)),
        Err(_) => {
            return Err(ReplayError::Source(
                "PipeWire capture thread exited before init".to_owned(),
            ));
        }
    };

    let weak = handle;
    let shutdown = Box::new(move || weak.quit());

    Ok((shutdown, thread))
}

struct MainLoopHandle {
    weak: pipewire::main_loop::WeakMainLoop,
}
// SAFETY: `WeakMainLoop` wraps a `pw_main_loop *` whose `quit()` is documented as
// thread-safe (it just calls `pw_loop_invoke`/`pw_loop_signal_event` internally).
unsafe impl Send for MainLoopHandle {}

impl MainLoopHandle {
    fn quit(self) {
        if let Some(strong) = self.weak.upgrade() {
            strong.quit();
        }
    }
}

fn run_main_loop(
    tx: mpsc::Sender<LoopbackEvent>,
    init_tx: std::sync::mpsc::SyncSender<Result<MainLoopHandle, String>>,
) {
    pipewire::init();

    let mainloop = match MainLoop::new(None) {
        Ok(m) => m,
        Err(err) => {
            let _ = init_tx.send(Err(format!("PipeWire MainLoop::new failed: {err}")));
            return;
        }
    };

    let context = match Context::new(&mainloop) {
        Ok(c) => c,
        Err(err) => {
            let _ = init_tx.send(Err(format!("PipeWire Context::new failed: {err}")));
            return;
        }
    };

    let core = match context.connect(None) {
        Ok(c) => c,
        Err(err) => {
            let _ = init_tx.send(Err(format!("PipeWire Context::connect failed: {err}")));
            return;
        }
    };

    let registry = match core.get_registry() {
        Ok(r) => r,
        Err(err) => {
            let _ = init_tx.send(Err(format!("PipeWire core.get_registry failed: {err}")));
            return;
        }
    };

    // All callbacks run on this thread, so Rc<RefCell<...>> is safe.
    // The map is keyed by the afv target node id.
    let captures: Rc<RefCell<HashMap<u32, Capture>>> = Rc::new(RefCell::new(HashMap::new()));

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let captures = captures.clone();
            let core = core.clone();
            let tx = tx.clone();
            move |obj| {
                let Some(props) = obj.props else { return };
                match obj.type_ {
                    ObjectType::Node => {
                        let app_name = props.get("application.name").unwrap_or("");
                        let media_class = props.get("media.class").unwrap_or("");
                        // Case 1: an afv stream output node we want to capture.
                        if media_class == STREAM_OUTPUT_CLASS
                            && (app_name == AFV_APP_SPEAKER || app_name == AFV_APP_HEADSET)
                        {
                            let tap = if app_name == AFV_APP_SPEAKER {
                                TapId::Speaker
                            } else {
                                TapId::Headset
                            };
                            log::info!(
                                "matched PipeWire node id={} app.name={app_name} for tap {tap:?}",
                                obj.id,
                            );
                            match build_capture(&core, obj.id, tap, tx.clone()) {
                                Ok(cap) => {
                                    captures.borrow_mut().insert(obj.id, cap);
                                }
                                Err(err) => {
                                    log::error!(
                                        "failed to attach capture to node {}: {err}",
                                        obj.id
                                    );
                                }
                            }
                            return;
                        }
                        // Case 2: our own capture stream's node global. The suffix
                        // of NODE_NAME tells us which target it belongs to.
                        let node_name = props.get("node.name").unwrap_or("");
                        if let Some(rest) = node_name.strip_prefix(NODE_NAME_PREFIX)
                            && let Ok(target) = rest.parse::<u32>()
                        {
                            let mut map = captures.borrow_mut();
                            if let Some(cap) = map.get_mut(&target) {
                                cap.own_node_id = Some(obj.id);
                                log::debug!(
                                    "capture stream for target {target} has node id {}",
                                    obj.id
                                );
                                try_link(&core, cap);
                            }
                        }
                    }
                    ObjectType::Port => {
                        let Some(node_id) =
                            props.get("node.id").and_then(|s| s.parse::<u32>().ok())
                        else {
                            return;
                        };
                        let direction = props.get("port.direction").unwrap_or("");
                        let mut map = captures.borrow_mut();
                        for cap in map.values_mut() {
                            if node_id == cap.target_node_id && direction == "out" {
                                if !cap.target_outputs.contains(&obj.id) {
                                    cap.target_outputs.push(obj.id);
                                    log::trace!(
                                        "target node {} got output port {} (total {})",
                                        cap.target_node_id,
                                        obj.id,
                                        cap.target_outputs.len()
                                    );
                                }
                                try_link(&core, cap);
                            } else if Some(node_id) == cap.own_node_id && direction == "in" {
                                if !cap.own_inputs.contains(&obj.id) {
                                    cap.own_inputs.push(obj.id);
                                    log::trace!(
                                        "capture {:?} got input port {} (total {})",
                                        cap.tap,
                                        obj.id,
                                        cap.own_inputs.len()
                                    );
                                }
                                try_link(&core, cap);
                            }
                        }
                    }
                    _ => {}
                }
            }
        })
        .global_remove({
            let captures = captures.clone();
            let tx = tx.clone();
            move |id| {
                if let Some(cap) = captures.borrow_mut().remove(&id) {
                    log::info!("PipeWire node {id} ({:?}) removed", cap.tap);
                    let _ = tx.try_send(LoopbackEvent::Closed { tap: cap.tap });
                }
            }
        })
        .register();

    let handle = MainLoopHandle {
        weak: mainloop.downgrade(),
    };
    if init_tx.send(Ok(handle)).is_err() {
        log::warn!("caller disappeared before PipeWire init completed");
        return;
    }

    log::debug!("PipeWire main loop running");
    mainloop.run();
    log::debug!("PipeWire main loop exited");

    // Stream/listener drops (in `captures`) tear down before unsafe bindings outlive
    // anything; explicit drop here makes that ordering obvious.
    drop(captures);
    drop(registry);
    drop(core);
    drop(context);
    drop(mainloop);
}

fn build_capture(
    core: &pipewire::core::Core,
    node_id: u32,
    tap: TapId,
    tx: mpsc::Sender<LoopbackEvent>,
) -> Result<Capture, String> {
    // Note: no `target.object` and no AUTOCONNECT. Wireplumber's session policy
    // would otherwise hijack `MEDIA_CATEGORY=Capture` and link us to the default
    // source (mic). We create the link manually via the link factory once we have
    // both the target's output ports and our own input ports from the registry.
    let stream_node_name = format!("{NODE_NAME_PREFIX}{node_id}");
    let props = properties! {
        *keys::MEDIA_TYPE => "Audio",
        *keys::MEDIA_CATEGORY => "Capture",
        *keys::MEDIA_ROLE => "Music",
        *keys::APP_NAME => "vacs-replay",
        *keys::NODE_NAME => stream_node_name.as_str(),
        "node.autoconnect" => "false",
    };

    let stream = Stream::new(core, "vacs-replay-tap", props)
        .map_err(|e| format!("Stream::new failed: {e}"))?;

    let user_data = CaptureUserData {
        tap,
        opened: false,
        sample_rate: 0,
        channels: 0,
        tx,
        node_id,
    };

    let listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(|_stream, state, old, new| {
            log::trace!("stream node={} state {old:?} -> {new:?}", state.node_id,);
            if let PwStreamState::Error(err) = &new {
                log::warn!("stream node={} error: {err}", state.node_id);
            }
        })
        .param_changed(|_stream, state, id, param| {
            if id != ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else { return };
            let mut info = AudioInfoRaw::new();
            if let Err(err) = info.parse(param) {
                log::warn!("failed to parse audio format: {err}");
                return;
            }
            let sample_rate = info.rate();
            let channels = info.channels();
            log::info!(
                "stream node={} ({:?}) format negotiated rate={sample_rate} channels={channels}",
                state.node_id,
                state.tap,
            );
            state.sample_rate = sample_rate;
            state.channels = channels as u16;
            if !state.opened && sample_rate > 0 && channels > 0 {
                state.opened = true;
                let _ = state.tx.try_send(LoopbackEvent::Opened {
                    tap: state.tap,
                    sample_rate,
                    channels: channels as u16,
                });
            }
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            // We requested interleaved F32LE, so samples for all channels live in datas[0].
            let data = &mut datas[0];
            let size = data.chunk().size() as usize;
            if size == 0 {
                return;
            }

            let Some(bytes) = data.data() else { return };
            let stride = std::mem::size_of::<f32>();
            if bytes.len() < size {
                log::warn!("short PipeWire buffer ({} < {size})", bytes.len());
                return;
            }

            let n_samples = size / stride;
            let mut samples = Vec::with_capacity(n_samples);
            for chunk_bytes in bytes[..size].chunks_exact(stride) {
                samples.push(f32::from_le_bytes([
                    chunk_bytes[0],
                    chunk_bytes[1],
                    chunk_bytes[2],
                    chunk_bytes[3],
                ]));
            }

            let evt = LoopbackEvent::Frame {
                tap: state.tap,
                samples: Arc::from(samples.into_boxed_slice()),
                captured_at: Instant::now(),
            };
            if let Err(err) = state.tx.try_send(evt) {
                log::trace!("dropped frame: {err}");
            }
        })
        .register()
        .map_err(|e| format!("stream.register failed: {e}"))?;

    // Build format negotiation pod: F32LE interleaved, peer-defined channels & rate.
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    let obj = Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };

    let bytes = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(obj))
        .map_err(|e| format!("pod serialize failed: {e}"))?
        .0
        .into_inner();
    let pod = Pod::from_bytes(&bytes).ok_or_else(|| "invalid pod bytes".to_owned())?;
    let mut params = [pod];

    stream
        .connect(
            Direction::Input,
            None,
            StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| format!("stream.connect failed: {e}"))?;

    Ok(Capture {
        target_node_id: node_id,
        tap,
        _stream: stream,
        _listener: listener,
        own_node_id: None,
        target_outputs: Vec::new(),
        own_inputs: Vec::new(),
        links: Vec::new(),
    })
}

/// Pair up known target outputs with our stream's inputs and create any missing
/// links via the `link-factory`. Idempotent: only links beyond `capture.links.len()`
/// are created.
fn try_link(core: &pipewire::core::Core, capture: &mut Capture) {
    let Some(my_node_id) = capture.own_node_id else {
        return;
    };
    let n_pairs = capture.target_outputs.len().min(capture.own_inputs.len());
    while capture.links.len() < n_pairs {
        let i = capture.links.len();
        let out_port = capture.target_outputs[i];
        let in_port = capture.own_inputs[i];
        let props = properties! {
            "link.output.node" => capture.target_node_id.to_string(),
            "link.output.port" => out_port.to_string(),
            "link.input.node" => my_node_id.to_string(),
            "link.input.port" => in_port.to_string(),
            "object.linger" => "false",
        };

        match core.create_object::<Link>("link-factory", &props) {
            Ok(link) => {
                log::info!(
                    "linked {}:{out_port} -> {my_node_id}:{in_port} for {:?}",
                    capture.target_node_id,
                    capture.tap,
                );
                capture.links.push(link);
            }
            Err(err) => {
                log::warn!(
                    "link.create failed for {:?} ({}:{out_port} -> {my_node_id}:{in_port}): {err}",
                    capture.tap,
                    capture.target_node_id,
                );
                break;
            }
        }
    }
}
