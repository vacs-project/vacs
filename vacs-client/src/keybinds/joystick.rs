//! Shared SDL3 joystick button source.
//!
//! # Architecture
//!
//! Joystick support is deliberately *not* a [`KeybindListener`](super::runtime::KeybindListener):
//! the platform listeners are owned by the [`KeybindEngine`](super::engine::KeybindEngine) and
//! only exist while at least one binding is configured, but the settings UI must be able to
//! capture "the next pressed button" before any binding exists. This service is therefore owned
//! by Tauri app state and shared by both consumers:
//!
//! - the engine subscribes when a joystick binding is configured
//! - the binding-capture IPC command subscribes on demand
//!
//! # SDL usage
//!
//! SDL is initialized lazily on the first [`subscribe`](JoystickService::subscribe) call, on a
//! dedicated thread that owns the (joystick+events only, no video/audio) SDL context and pumps
//! events. The thread then stays alive until [`shutdown`](JoystickService::shutdown): repeated
//! SDL init/quit cycles are a known source of platform bugs, and a thread parked in
//! `wait_event_timeout` is effectively free. Users who never touch joystick features never
//! initialize SDL at all.
//!
//! `SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS` is set before init so button events keep arriving
//! while another application (e.g. a radar client) has focus - a hard requirement for PTT.
//!
//! # Permissions (Linux)
//!
//! No root, udev rules, or group membership are required: udev tags joystick-capable devices
//! with `uaccess`, so logind grants the active seat user an ACL on the evdev nodes. This works
//! identically under Wayland and X11.

use crate::keybinds::runtime;
use crate::keybinds::{JoystickButton, JoystickDevice, KeyEvent, KeybindsError};
use keyboard_types::KeyState;
use sdl3::event::Event;
use sdl3::joystick::JoystickId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_WAIT_TIMEOUT: Duration = Duration::from_millis(250);

pub type JoystickServiceHandle = Arc<JoystickService>;

/// Senders the poller fans button events out to; closed senders are pruned on
/// the next event.
///
/// A list is needed here (unlike the keyboard listeners, which write into a
/// plain mpsc sender because their single consumer - the engine - also owns
/// their lifetime): this long-lived service serves several consumers with
/// independent lifetimes at once. The engine's channel gets a new identity on
/// every settings change, and binding captures need their own ephemeral
/// channels even while the engine is stopped. A `tokio::broadcast` bus would
/// remove the list but has bounded buffers that drop events for lagging
/// receivers - and a dropped PTT release means a stuck transmission. Unbounded
/// senders make this a lossless broadcast instead.
type SenderRegistry = Arc<parking_lot::Mutex<Vec<UnboundedSender<KeyEvent>>>>;

/// Currently connected devices, keyed by SDL instance id and maintained by the
/// poller thread. Shared so the device list can be queried for the settings UI.
type DeviceRegistry = Arc<parking_lot::Mutex<HashMap<JoystickId, JoystickDevice>>>;

#[derive(Debug, Default)]
pub struct JoystickService {
    poller: tokio::sync::Mutex<Option<RunningPoller>>,
}

#[derive(Debug)]
struct RunningPoller {
    senders: SenderRegistry,
    devices: DeviceRegistry,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl JoystickService {
    pub fn new() -> JoystickServiceHandle {
        Arc::new(Self::default())
    }

    /// Register a channel to receive joystick button events, lazily starting
    /// the SDL poller thread on first use.
    ///
    /// The sender stays registered until its receiver is dropped, after which
    /// it is pruned on the next joystick event.
    pub async fn register(&self, tx: UnboundedSender<KeyEvent>) -> Result<(), KeybindsError> {
        let mut poller = self.poller.lock().await;
        let running = Self::ensure_started(&mut poller).await?;

        let mut senders = running.senders.lock();
        // Eagerly drop senders whose receivers are gone (e.g. from a
        // stopped engine) instead of waiting for the next event.
        senders.retain(|sender| !sender.is_closed());
        senders.push(tx);
        Ok(())
    }

    /// Register a dedicated channel and return its receiver, for consumers
    /// without an existing channel (e.g. the binding-capture command).
    pub async fn subscribe(&self) -> Result<UnboundedReceiver<KeyEvent>, KeybindsError> {
        let (tx, rx) = unbounded_channel();
        self.register(tx).await?;
        Ok(rx)
    }

    /// The joystick devices currently connected. Lazily starts the SDL poller
    /// thread on first use.
    ///
    /// Devices hash by GUID, so physically identical devices collapse into a
    /// single entry.
    pub async fn connected_devices(&self) -> Result<HashSet<JoystickDevice>, KeybindsError> {
        let mut poller = self.poller.lock().await;
        let running = Self::ensure_started(&mut poller).await?;

        let devices = running.devices.lock();
        Ok(devices.values().cloned().collect())
    }

    /// Start the SDL poller thread if it is not already running.
    async fn ensure_started(
        poller: &mut Option<RunningPoller>,
    ) -> Result<&RunningPoller, KeybindsError> {
        if poller.is_none() {
            log::debug!("Starting SDL joystick poller");

            let senders: SenderRegistry = Arc::default();
            let devices: DeviceRegistry = Arc::default();
            let (startup_tx, startup_rx) = oneshot::channel();
            let stop = Arc::new(AtomicBool::new(false));

            let thread = {
                let senders = senders.clone();
                let devices = devices.clone();
                let stop = stop.clone();
                std::thread::Builder::new()
                    .name("VACS_SDL_Joystick".to_string())
                    .spawn(move || run_poller(senders, devices, stop, startup_tx))
                    .map_err(|err| {
                        KeybindsError::Listener(format!("Failed to spawn joystick thread: {err}"))
                    })?
            };

            match runtime::await_startup(startup_rx, STARTUP_TIMEOUT, "SDL joystick poller").await {
                Ok(()) => {
                    *poller = Some(RunningPoller {
                        senders,
                        devices,
                        stop,
                        thread: Some(thread),
                    });
                }
                Err(err) => {
                    // Harmless if the thread already exited on a startup failure
                    stop.store(true, Ordering::Relaxed);
                    return Err(err);
                }
            }
        }

        Ok(poller.as_ref().expect("poller started above"))
    }

    /// Stop the poller thread (if running) and wait for it to exit.
    pub async fn shutdown(&self) {
        let running = self.poller.lock().await.take();
        if let Some(mut running) = running {
            log::debug!("Stopping SDL joystick poller");
            running.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = running.thread.take() {
                // The poller notices the stop flag within its event-wait timeout
                // (up to 250ms); join off the async runtime to avoid stalling a
                // worker thread for that long.
                match tauri::async_runtime::spawn_blocking(move || thread.join()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => log::warn!("SDL joystick poller thread panicked"),
                    Err(err) => log::warn!("Failed to join SDL joystick poller thread: {err}"),
                }
            }
        }
    }
}

struct OpenDevice {
    // Keeps the SDL device handle alive; button events stop without it.
    _joystick: sdl3::joystick::Joystick,
    guid: String,
    name: String,
    held: HashSet<u32>,
}

fn run_poller(
    senders: SenderRegistry,
    devices: DeviceRegistry,
    stop: Arc<AtomicBool>,
    startup_tx: oneshot::Sender<Result<(), KeybindsError>>,
) {
    let send = |event: KeyEvent| {
        senders
            .lock()
            .retain(|sender| sender.send(event.clone()).is_ok());
    };

    // Must be set before SDL_Init for the Windows backends to deliver events
    // while another window (e.g. a radar client) has focus.
    sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
    // SDL's events subsystem catches SIGINT/SIGTERM by default and converts
    // them into an SDL_EVENT_QUIT (which this poller ignores), which would
    // prevent the whole application from terminating on Ctrl+C. Tauri owns the
    // process lifecycle, so keep SDL away from signal handling.
    sdl3::hint::set("SDL_NO_SIGNAL_HANDLERS", "1");

    let init = || -> Result<(sdl3::JoystickSubsystem, sdl3::EventPump), String> {
        let sdl = sdl3::init().map_err(|err| format!("SDL init failed: {err}"))?;
        let joystick = sdl
            .joystick()
            .map_err(|err| format!("SDL joystick subsystem init failed: {err}"))?;
        let pump = sdl
            .event_pump()
            .map_err(|err| format!("SDL event pump init failed: {err}"))?;
        Ok((joystick, pump))
    };

    let (joystick, mut pump) = match init() {
        Ok(res) => res,
        Err(err) => {
            let _ = startup_tx.send(Err(KeybindsError::Listener(err)));
            return;
        }
    };

    // Keyed by SDL joystick instance id. Devices already connected produce
    // synthetic JoyDeviceAdded events during SDL init; drain them before
    // signaling readiness so an immediate device-list query sees them.
    let mut open: HashMap<JoystickId, OpenDevice> = HashMap::new();
    while let Some(event) = pump.poll_event() {
        handle_event(&joystick, &devices, &mut open, &send, event);
    }

    let _ = startup_tx.send(Ok(()));

    while !stop.load(Ordering::Relaxed) {
        let Some(event) = pump.wait_event_timeout(EVENT_WAIT_TIMEOUT) else {
            continue;
        };

        handle_event(&joystick, &devices, &mut open, &send, event);
    }

    log::trace!("SDL joystick poller finished");
}

fn handle_event(
    joystick: &sdl3::JoystickSubsystem,
    devices: &DeviceRegistry,
    open: &mut HashMap<JoystickId, OpenDevice>,
    send: &impl Fn(KeyEvent),
    event: Event,
) {
    match event {
        Event::JoyDeviceAdded { which, .. } => match joystick.open(which) {
            Ok(device) => {
                let guid = device.guid().to_string();
                let name = device.name();
                log::info!("Joystick connected: {name} (guid {guid}, instance {which})");
                devices.lock().insert(
                    which,
                    JoystickDevice {
                        device: guid.clone(),
                        name: Some(name.clone()),
                    },
                );
                open.insert(
                    which,
                    OpenDevice {
                        _joystick: device,
                        guid,
                        name,
                        held: HashSet::new(),
                    },
                );
            }
            Err(err) => log::warn!("Failed to open joystick (instance {which}): {err}"),
        },
        Event::JoyDeviceRemoved { which, .. } => {
            devices.lock().remove(&which);
            if let Some(device) = open.remove(&which) {
                log::info!(
                    "Joystick disconnected: {} (guid {})",
                    device.name,
                    device.guid
                );
                // Synthesize releases for held buttons so a PTT bound to this
                // device cannot get stuck transmitting after an unplug.
                for button in device.held {
                    send(button_event(
                        &device.guid,
                        &device.name,
                        button,
                        KeyState::Up,
                    ));
                }
            }
        }
        Event::JoyButtonDown {
            which, button_idx, ..
        } => {
            if let Some(device) = open.get_mut(&which) {
                let button = u32::from(button_idx);
                device.held.insert(button);
                send(button_event(
                    &device.guid,
                    &device.name,
                    button,
                    KeyState::Down,
                ));
            }
        }
        Event::JoyButtonUp {
            which, button_idx, ..
        } => {
            if let Some(device) = open.get_mut(&which) {
                let button = u32::from(button_idx);
                device.held.remove(&button);
                send(button_event(
                    &device.guid,
                    &device.name,
                    button,
                    KeyState::Up,
                ));
            }
        }
        _ => {}
    }
}

fn button_event(guid: &str, name: &str, button: u32, state: KeyState) -> KeyEvent {
    KeyEvent::button(
        JoystickButton {
            device: guid.to_string(),
            button,
            name: Some(name.to_string()),
        },
        format!("{name} B{button}"),
        state,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinds::{InputCode, Trigger};
    use sdl3::sys::joystick::{
        SDL_AttachVirtualJoystick, SDL_CloseJoystick, SDL_DetachVirtualJoystick, SDL_Joystick,
        SDL_OpenJoystick, SDL_SetJoystickVirtualButton, SDL_VirtualJoystickDesc,
    };
    use std::time::Duration;

    const VIRTUAL_NAME: &str = "VACS Virtual Test Stick";

    /// A process-local virtual joystick, driven through the raw SDL FFI.
    ///
    /// The safe sdl3 wrapper cannot be used here: it binds `Sdl` contexts to the
    /// first-initializing thread, which is the service's poller thread. The C
    /// joystick API itself is internally synchronized and safe to call from the
    /// test thread while the poller pumps events.
    struct VirtualJoystick {
        id: sdl3::sys::joystick::SDL_JoystickID,
        handle: *mut SDL_Joystick,
    }

    impl VirtualJoystick {
        fn attach(name: &std::ffi::CStr, buttons: u16) -> Self {
            let mut desc = SDL_VirtualJoystickDesc::new();
            desc.name = name.as_ptr();
            desc.nbuttons = buttons;

            let id = unsafe { SDL_AttachVirtualJoystick(&desc) };
            assert_ne!(id.0, 0, "failed to attach virtual joystick");

            // Events are only generated for opened joysticks; the poller opens
            // it too once it sees the added event.
            let handle = unsafe { SDL_OpenJoystick(id) };
            assert!(!handle.is_null(), "failed to open virtual joystick");

            Self { id, handle }
        }

        fn set_button(&self, button: i32, down: bool) {
            assert!(unsafe { SDL_SetJoystickVirtualButton(self.handle, button, down) });
        }

        /// Detach the device, simulating an unplug.
        fn detach(self) {
            unsafe {
                SDL_CloseJoystick(self.handle);
                SDL_DetachVirtualJoystick(self.id);
            }
        }
    }

    /// Receive the next event originating from the virtual test device,
    /// skipping chatter from real joysticks attached to the machine running
    /// the tests.
    async fn next_virtual_event(
        rx: &mut UnboundedReceiver<KeyEvent>,
    ) -> (JoystickButton, KeyState) {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out waiting for joystick event")
                .expect("joystick event stream closed");
            if let Trigger::Input(InputCode::Button(button)) = event.trigger
                && button.name.as_deref() == Some(VIRTUAL_NAME)
            {
                return (button, event.state);
            }
        }
    }

    /// End-to-end roundtrip through the SDL poller using a process-local
    /// virtual joystick (no hardware or display required): button transitions
    /// arrive on all registered channels, and detaching the device while a
    /// button is held synthesizes its release (stuck-PTT guard).
    #[test]
    #[ignore = "initializes SDL and drives its event loop"]
    fn virtual_joystick_roundtrip() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let service = JoystickService::new();
            let mut rx = service
                .subscribe()
                .await
                .expect("joystick service failed to start");
            let mut rx_second = service
                .subscribe()
                .await
                .expect("second subscription failed");

            let name = std::ffi::CString::new(VIRTUAL_NAME).unwrap();
            let device = VirtualJoystick::attach(&name, 2);

            // press/release reaches subscribers with the raw button index
            device.set_button(0, true);
            let (button, state) = next_virtual_event(&mut rx).await;
            assert_eq!(state, KeyState::Down);
            assert_eq!(button.button, 0);
            let guid = button.device.clone();

            // the attached device shows up in the connected-device list
            // (used by the capture ignore-list settings UI)
            let connected = service.connected_devices().await.unwrap();
            let listed = connected
                .iter()
                .find(|listed| listed.device == guid)
                .expect("virtual device missing from connected devices");
            assert_eq!(listed.name.as_deref(), Some(VIRTUAL_NAME));

            // both subscriptions observe the same event (engine and binding
            // capture consume the broadcast concurrently in production)
            let (second_button, second_state) = next_virtual_event(&mut rx_second).await;
            assert_eq!((second_button, second_state), (button, KeyState::Down));

            device.set_button(0, false);
            let (button, state) = next_virtual_event(&mut rx).await;
            assert_eq!((button.button, state), (0, KeyState::Up));

            // hold a button and "unplug" the device: a release must arrive so
            // a PTT bound to it cannot get stuck transmitting
            device.set_button(1, true);
            let (button, state) = next_virtual_event(&mut rx).await;
            assert_eq!((button.button, state), (1, KeyState::Down));

            device.detach();

            let (button, state) = next_virtual_event(&mut rx).await;
            assert_eq!(state, KeyState::Up);
            assert_eq!(button.button, 1);
            assert_eq!(button.device, guid);

            // and it disappears from the connected-device list again
            let connected = service.connected_devices().await.unwrap();
            assert!(connected.iter().all(|listed| listed.device != guid));

            service.shutdown().await;
        });
    }
}
