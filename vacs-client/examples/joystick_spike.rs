//! Phase-0 spike for SDL3 joystick keybind support.
//!
//! Verifies, using the exact patterns the production `JoystickService` will use:
//! 1. joystick+events-only SDL init on a dedicated (non-main) thread
//! 2. `SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS` set before init (background PTT)
//! 3. devices present at init produce synthetic `JoyDeviceAdded` events
//! 4. GUID stability across replug (replug a device while running and compare)
//! 5. runs as a plain user on Linux (no root, no udev rules)
//!
//! Run with: `cargo run -p vacs-client --example joystick_spike`
//! Press joystick buttons / replug devices; Ctrl+C to exit.

use sdl3::event::Event;
use sdl3::joystick::JoystickId;
use std::collections::HashMap;

fn main() {
    let handle = std::thread::Builder::new()
        .name("VACS_SDL_Joystick".into())
        .spawn(run_poller)
        .expect("failed to spawn poller thread");

    handle.join().expect("poller thread panicked");
}

fn run_poller() {
    // Must be set before SDL_Init for the Windows DirectInput/RawInput backends to
    // deliver events while another window (e.g. a flight sim) has focus.
    sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");

    let sdl = sdl3::init().expect("SDL_Init failed");
    let joystick = sdl.joystick().expect("joystick subsystem init failed");
    let mut pump = sdl.event_pump().expect("event pump init failed");

    println!(
        "SDL initialized on thread {:?} (version {})",
        std::thread::current().name(),
        sdl3::version::version()
    );
    println!("Waiting for joystick events (buttons, hotplug)...");

    let mut open: HashMap<JoystickId, sdl3::joystick::Joystick> = HashMap::new();

    loop {
        let Some(event) = pump.wait_event_timeout(std::time::Duration::from_millis(250)) else {
            continue;
        };

        match event {
            Event::JoyDeviceAdded { which, .. } => match joystick.open(which) {
                Ok(dev) => {
                    println!(
                        "ADDED   id={which} guid={} name={:?} buttons={}",
                        dev.guid(),
                        dev.name(),
                        dev.num_buttons()
                    );
                    open.insert(which, dev);
                }
                Err(err) => eprintln!("ADDED   id={which} but open failed: {err}"),
            },
            Event::JoyDeviceRemoved { which, .. } => {
                let guid = open
                    .remove(&which)
                    .map(|d| d.guid().to_string())
                    .unwrap_or_else(|| "<unknown>".into());
                println!("REMOVED id={which} guid={guid}");
            }
            Event::JoyButtonDown {
                which, button_idx, ..
            } => {
                let name = open.get(&which).map(|d| d.name()).unwrap_or_default();
                println!("DOWN    id={which} button={button_idx} ({name} B{button_idx})");
            }
            Event::JoyButtonUp {
                which, button_idx, ..
            } => {
                println!("UP      id={which} button={button_idx}");
            }
            Event::Quit { .. } => break,
            _ => {}
        }
    }
}
