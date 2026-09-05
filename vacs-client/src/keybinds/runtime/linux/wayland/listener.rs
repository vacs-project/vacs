//! Wayland keybind listener implementation using XDG Global Shortcuts portal.
//!
//! # Architecture
//!
//! This listener connects to the XDG Desktop Portal's Global Shortcuts API to receive
//! global keyboard events on Wayland. The implementation is split into several helper
//! functions to keep the code maintainable:
//!
//! - `initialize_portal()`: Registers the app id, creates the D-Bus proxy and session
//! - `check_existing_shortcuts()`: Checks if shortcuts are already configured
//! - `bind_shortcuts()`: Registers new shortcuts with the portal
//! - `ensure_configuration()`: Shows the configuration UI if needed
//! - `run_shortcuts_listener()`: Main event loop listening for portal signals
//!
//! ## Startup Synchronization
//!
//! The listener uses a oneshot channel to signal when initialization is complete. This
//! ensures the `KeybindEngine` doesn't proceed until the portal connection is established
//! and shortcuts are registered. A 10-second timeout prevents hanging if the portal is
//! unavailable.
//!
//! ## Cleanup Strategy
//!
//! The listener uses two cancellation tokens:
//! - `cancellation_token`: Signals the background task to stop
//! - `cleanup_token`: Signals when cleanup (closing the portal session) is complete
//!
//! The `Drop` implementation cancels the task and waits up to 2 seconds for graceful
//! cleanup before aborting the task.
//!
//! ## Thread Safety & Locking
//!
//! The `shortcuts` map uses `parking_lot::RwLock` instead of `tokio::sync::RwLock` because:
//! - Accesses are very short-lived (just reading/writing a HashMap)
//! - No async operations are performed while holding the lock
//! - `parking_lot::RwLock` is more efficient for this use case (no async overhead)
//!
//! The map is shared between the main struct and the background task to allow querying
//! the current bindings via `get_external_binding()`.

use crate::keybinds::runtime::linux::wayland::{PortalShortcutId, registry};
use crate::keybinds::runtime::{self, KeybindListener};
use crate::keybinds::{KeyEvent, Keybind, KeybindsError};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut};
use ashpd::zbus::export::futures_core::Stream;
use futures_util::StreamExt;
use keyboard_types::KeyState;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct WaylandKeybindListener {
    cancellation_token: CancellationToken,
    cleanup_token: CancellationToken,
    task_handle: Option<JoinHandle<()>>,
    /// Map of portal shortcut IDs to their current key bindings (e.g., "Ctrl+Alt+P").
    /// Shared with the background task to allow querying current bindings.
    shortcuts: ShortcutMap,
}

impl KeybindListener for WaylandKeybindListener {
    async fn start(key_event_tx: UnboundedSender<KeyEvent>) -> Result<Self, KeybindsError>
    where
        Self: Sized,
    {
        log::debug!("Starting Wayland keybind listener");

        let (startup_tx, startup_rx) = oneshot::channel::<Result<(), KeybindsError>>();

        let cancellation_token = CancellationToken::new();
        let cleanup_token = CancellationToken::new();
        let shortcuts = Arc::new(RwLock::new(HashMap::new()));

        let task_handle = {
            let cancellation_token = cancellation_token.clone();
            let cleanup_token = cleanup_token.clone();
            let shortcuts = shortcuts.clone();

            tauri::async_runtime::spawn(async move {
                match setup_shortcuts_listener(
                    key_event_tx,
                    startup_tx,
                    cancellation_token,
                    cleanup_token,
                    shortcuts,
                )
                .await
                {
                    Ok(()) => log::trace!("Wayland keybind listener task finished"),
                    Err(err) => log::error!("Wayland keybind listener failed: {err}"),
                };
            })
        };

        match runtime::await_startup(
            startup_rx,
            Duration::from_secs(10),
            "Wayland keybind listener",
        )
        .await
        {
            Ok(()) => Ok(Self {
                cancellation_token,
                cleanup_token,
                task_handle: Some(task_handle),
                shortcuts,
            }),
            Err(err) => {
                cancellation_token.cancel();
                task_handle.abort();
                Err(err)
            }
        }
    }

    fn get_external_binding(&self, keybind: Keybind) -> Option<String> {
        self.get_shortcut_binding(PortalShortcutId::from(keybind))
    }

    fn has_external_binding(&self, keybind: Keybind) -> bool {
        self.shortcuts
            .read()
            .contains_key(&PortalShortcutId::from(keybind))
    }
}

impl Drop for WaylandKeybindListener {
    fn drop(&mut self) {
        log::debug!("Stopping Wayland keybind listener");

        self.cancellation_token.cancel();

        if let Some(handle) = self.task_handle.take() {
            let cleanup_token = self.cleanup_token.clone();

            tauri::async_runtime::spawn(async move {
                tokio::select! {
                    _ = cleanup_token.cancelled() => {
                        log::debug!("Wayland keybind listener cleanup completed");
                    }
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {
                        log::warn!("Wayland keybind listener cleanup timed out, aborting");
                        handle.abort();
                    }
                }
            });
        }
    }
}

impl WaylandKeybindListener {
    pub fn get_shortcut_binding(&self, shortcut_id: PortalShortcutId) -> Option<String> {
        self.shortcuts.read().get(&shortcut_id).cloned()
    }
}

async fn setup_shortcuts_listener(
    key_event_tx: UnboundedSender<KeyEvent>,
    startup_tx: oneshot::Sender<Result<(), KeybindsError>>,
    cancellation_token: CancellationToken,
    cleanup_token: CancellationToken,
    shortcuts_map: ShortcutMap,
) -> ashpd::Result<()> {
    log::debug!("Initializing Wayland global shortcuts");

    let mut startup_tx = Some(startup_tx);

    let (proxy, session) = match initialize_portal(&mut startup_tx).await {
        Ok(res) => res,
        Err(err) => return Err(err),
    };

    // Every error return past session creation funnels through the cleanup below.
    // Bailing out early would leak the session for the life of the process, and the
    // portal re-emits every shortcut signal once per live session.
    //
    // Aborting this task still leaks it: `ashpd::desktop::Session` has no `Drop`,
    // only an async `close()`. Both abort paths (the `await_startup` timeout and
    // the `Drop` cleanup timeout) are slow-portal cases, so the window is narrow,
    // but closing on abort would need the session behind a guard that spawns the
    // close when the future is dropped.
    let res = async {
        // Seed the map so `get_external_binding` has data while the bind request is
        // still in flight (it can block on a user-facing dialog on first run).
        let all_known =
            check_existing_shortcuts(&proxy, &session, &mut startup_tx, &shortcuts_map).await?;

        // BindShortcuts must run on every session, even when the portal already
        // reports all shortcuts as configured. It is the only call that registers
        // the shortcuts with the compositor's shortcut daemon: xdg-desktop-portal-kde
        // used to register them on session creation too, but stopped doing so in
        // 6.7.4 (commit 0d743a71), and xdg-desktop-portal-gnome never did - its
        // ListShortcuts returns nothing until the session is bound.
        bind_shortcuts(&proxy, &session, &mut startup_tx, &shortcuts_map, all_known).await?;

        let activated = proxy.receive_activated().await?;
        let deactivated = proxy.receive_deactivated().await?;
        let shortcuts_changed = proxy.receive_shortcuts_changed().await?;

        run_shortcuts_listener(
            key_event_tx,
            cancellation_token,
            &shortcuts_map,
            activated,
            deactivated,
            shortcuts_changed,
        )
        .await
    }
    .await;

    log::trace!("Cleaning up Wayland global shortcuts session");
    if let Err(err) = tokio::time::timeout(Duration::from_secs(2), session.close()).await {
        log::warn!("Failed to close global shortcuts session: {err}");
    }

    cleanup_token.cancel();

    res
}

async fn initialize_portal(
    startup_tx: &mut Option<oneshot::Sender<Result<(), KeybindsError>>>,
) -> ashpd::Result<(GlobalShortcuts, ashpd::desktop::Session<GlobalShortcuts>)> {
    let connect = async {
        let connection = registry::connect().await?;
        GlobalShortcuts::with_connection(connection).await
    };
    let proxy = match tokio::time::timeout(Duration::from_secs(5), connect).await {
        Ok(Ok(proxy)) => proxy,
        Ok(Err(err)) => {
            log::error!("Failed to create GlobalShortcuts proxy: {err}");
            let _ = startup_tx.take().map(|tx| {
                tx.send(Err(KeybindsError::Listener(
                    "Portal unavailable".to_string(),
                )))
            });
            return Err(err);
        }
        Err(_) => {
            log::error!("Timed out creating GlobalShortcuts proxy");
            let _ = startup_tx.take().map(|tx| {
                tx.send(Err(KeybindsError::Listener(
                    "Portal unavailable".to_string(),
                )))
            });
            return Err(ashpd::Error::NoResponse);
        }
    };

    let session = match tokio::time::timeout(
        Duration::from_secs(5),
        proxy.create_session(Default::default()),
    )
    .await
    {
        Ok(Ok(session)) => session,
        Ok(Err(err)) => {
            log::error!("Failed to create shortcuts session: {err}");
            let _ = startup_tx.take().map(|tx| {
                tx.send(Err(KeybindsError::Listener(
                    "Portal session failed".to_string(),
                )))
            });
            return Err(err);
        }
        Err(_) => {
            log::error!("Timed out creating shortcuts session");
            let _ = startup_tx.take().map(|tx| {
                tx.send(Err(KeybindsError::Listener(
                    "Portal session failed".to_string(),
                )))
            });
            return Err(ashpd::Error::NoResponse);
        }
    };

    Ok((proxy, session))
}

/// Populates `shortcuts_map` from the portal's current view of the session.
///
/// Portal backends differ in what this returns before [`bind_shortcuts`] has run
/// on the session: xdg-desktop-portal-kde reports the persisted shortcuts,
/// xdg-desktop-portal-gnome reports nothing. Treat an empty result as "unknown",
/// not as "unbound".
///
/// Returns whether every required shortcut was already reported, meaning the
/// upcoming [`bind_shortcuts`] has nothing new to ask the user about.
async fn check_existing_shortcuts(
    proxy: &GlobalShortcuts,
    session: &ashpd::desktop::Session<GlobalShortcuts>,
    startup_tx: &mut Option<oneshot::Sender<Result<(), KeybindsError>>>,
    shortcuts_map: &ShortcutMap,
) -> ashpd::Result<bool> {
    log::trace!("Checking for existing shortcuts");
    let request = proxy
        .list_shortcuts(session, Default::default())
        .await
        .map_err(|err| {
            log::error!("Failed to list shortcuts: {err}");
            let _ = startup_tx.take().map(|tx| {
                tx.send(Err(KeybindsError::Listener(
                    "Failed to list shortcuts".to_string(),
                )))
            });
            err
        })?;

    match request.response() {
        Ok(response) => {
            let shortcuts = response.shortcuts();
            log::trace!("Portal reports {} existing shortcuts", shortcuts.len());
            // An empty result means "this backend does not report before binding",
            // not "nothing is bound", so leave the map for `bind_shortcuts` to fill.
            if !shortcuts.is_empty() {
                update_shortcuts_map(shortcuts_map, shortcuts);
            }

            let reported_ids = shortcuts.iter().map(|s| s.id()).collect::<Vec<_>>();
            Ok(PortalShortcutId::all()
                .iter()
                .all(|id| reported_ids.contains(&id.as_str())))
        }
        Err(err) => {
            log::warn!("Failed to get list shortcuts response: {err}");
            Ok(false)
        }
    }
}

async fn bind_shortcuts(
    proxy: &GlobalShortcuts,
    session: &ashpd::desktop::Session<GlobalShortcuts>,
    startup_tx: &mut Option<oneshot::Sender<Result<(), KeybindsError>>>,
    shortcuts_map: &ShortcutMap,
    all_known: bool,
) -> ashpd::Result<()> {
    let shortcuts = PortalShortcutId::all()
        .iter()
        .map(NewShortcut::from)
        .collect::<Vec<_>>();

    log::trace!("Binding {} shortcuts", shortcuts.len());

    // When the portal may show a configuration dialog (some shortcuts are new to
    // it), the request blocks until the user closes it, so startup must be
    // signaled now or the engine's startup timeout fires mid-dialog. The cost is
    // that a failure after this point can only be logged. When every shortcut is
    // already known no dialog is possible and the request completes immediately,
    // so startup waits for the response and a bind failure fails `start()`
    // instead of leaving a listener that looks alive but delivers nothing. That
    // wait is bounded: keybind startup failure is fatal to app setup, so a
    // merely slow portal must degrade to the early signal, not block the launch.
    let request = if all_known {
        let bind = proxy.bind_shortcuts(session, &shortcuts, None, Default::default());
        tokio::pin!(bind);
        match tokio::time::timeout(Duration::from_secs(3), &mut bind).await {
            Ok(res) => res,
            Err(_) => {
                log::warn!(
                    "BindShortcuts still pending after 3s, signaling startup completion and continuing to wait"
                );
                let _ = startup_tx.take().map(|tx| tx.send(Ok(())));
                bind.await
            }
        }
    } else {
        log::trace!("Signaling startup completion before binding, a dialog may block the request");
        let _ = startup_tx.take().map(|tx| tx.send(Ok(())));
        proxy
            .bind_shortcuts(session, &shortcuts, None, Default::default())
            .await
    }
    .map_err(|err| {
        log::error!("Failed to bind shortcuts: {err}");
        let _ = startup_tx.take().map(|tx| {
            tx.send(Err(KeybindsError::Listener(
                "Failed to bind shortcuts".to_string(),
            )))
        });
        err
    })?;

    let response = request.response().map_err(|err| {
        log::error!("Failed to get bind shortcuts response: {err}");
        let _ = startup_tx.take().map(|tx| {
            tx.send(Err(KeybindsError::Listener(
                "Failed to bind shortcuts".to_string(),
            )))
        });
        err
    })?;

    let bound_shortcuts = response.shortcuts();
    log::trace!("Received {} bound shortcuts", bound_shortcuts.len());

    update_shortcuts_map(shortcuts_map, bound_shortcuts);

    let configured_shortcuts = bound_shortcuts
        .iter()
        .filter(|s| !s.trigger_description().is_empty())
        .collect::<Vec<_>>();
    if configured_shortcuts.is_empty() {
        // We still want to start the listener even if no shortcuts are configured
        // so that the user can configure them later without restarting the app
        log::warn!("No shortcuts configured, make sure to bind at least one before use");
    } else {
        log::trace!("Shortcuts configured: {:?}", configured_shortcuts);
    }

    let _ = startup_tx.take().map(|tx| tx.send(Ok(())));

    Ok(())
}

async fn run_shortcuts_listener(
    key_event_tx: UnboundedSender<KeyEvent>,
    cancellation_token: CancellationToken,
    shortcuts_map: &ShortcutMap,
    mut activated: impl Stream<Item = ashpd::desktop::global_shortcuts::Activated> + Unpin,
    mut deactivated: impl Stream<Item = ashpd::desktop::global_shortcuts::Deactivated> + Unpin,
    mut shortcuts_changed: impl Stream<Item = ashpd::desktop::global_shortcuts::ShortcutsChanged>
    + Unpin,
) -> ashpd::Result<()> {
    log::trace!("Starting Wayland shortcuts listener");
    loop {
        tokio::select! {
            biased;

            _ = cancellation_token.cancelled() => {
                log::debug!("Wayland shortcuts listener cancelled");
                break;
            }

            Some(signal) = activated.next() => {
                let shortcut_id = signal.shortcut_id();
                if let Ok(shortcut_id) = PortalShortcutId::try_from(shortcut_id) {
                    log::trace!("Shortcut activated: {shortcut_id:?}");

                    let _ = key_event_tx.send(KeyEvent::portal(
                        shortcut_id.into(),
                        shortcut_id.as_str().to_string(),
                        KeyState::Down,
                    ));
                } else {
                    log::warn!("Unknown shortcut activated: {shortcut_id}");
                }
            }

            Some(signal) = deactivated.next() => {
                let shortcut_id = signal.shortcut_id();
                if let Ok(shortcut_id) = PortalShortcutId::try_from(shortcut_id) {
                    log::trace!("Shortcut deactivated: {shortcut_id:?}");

                    let _ = key_event_tx.send(KeyEvent::portal(
                        shortcut_id.into(),
                        shortcut_id.as_str().to_string(),
                        KeyState::Up,
                    ));
                } else {
                    log::warn!("Unknown shortcut deactivated: {shortcut_id}");
                }
            }

            Some(signal) = shortcuts_changed.next() => {
                let updated_shortcuts = signal.shortcuts();
                log::debug!("Shortcuts changed, updating {} entries", updated_shortcuts.len());

                {
                    let mut map = shortcuts_map.write();
                    for shortcut in updated_shortcuts {
                        if let Ok(id) = PortalShortcutId::try_from(shortcut.id()) {
                            let trigger = shortcut.trigger_description();

                            if trigger.is_empty() {
                                if map.remove(&id).is_some() {
                                    log::trace!("Removed shortcut binding: {id:?}");
                                }
                            } else {
                                let previous = map.insert(id, trigger.to_string());
                                if let Some(previous) = previous {
                                    if previous != trigger {
                                        log::trace!("Updated shortcut binding {}: {} -> {trigger}", shortcut.id(), previous);
                                    }
                                } else {
                                    log::trace!("Shortcut configured: {} -> {trigger}", shortcut.id());
                                }
                            }
                        }
                    }

                    log::debug!("Updated shortcuts map with {} entries", map.len());
                }
            }

            else => {
                log::warn!("Signal streams ended unexpectedly");
                break;
            }
        }
    }

    log::trace!("Wayland shortcuts listener finished");
    Ok(())
}

fn update_shortcuts_map(shortcut_map: &ShortcutMap, bound_shortcuts: &[Shortcut]) {
    let mut map = shortcut_map.write();

    // Merge, do not replace: this runs once with the ListShortcuts seed and once
    // with the BindShortcuts response, and backends disagree on which of the two
    // carries the trigger descriptions. Whichever call reported real data wins;
    // an empty trigger means "this backend does not report here", not "unbound"
    // (an unbound shortcut is simply never inserted). User-driven removals
    // arrive through the ShortcutsChanged handler, which tracks them per entry.
    for shortcut in bound_shortcuts {
        if let Ok(id) = PortalShortcutId::try_from(shortcut.id()) {
            let trigger = shortcut.trigger_description();
            if !trigger.is_empty() {
                map.insert(id, trigger.to_string());
            }
        }
    }

    // Not a warning: this runs once per ListShortcuts and once per BindShortcuts,
    // and an empty map is the normal state before the user has assigned any key.
    // `bind_shortcuts` raises the single actionable warning after binding.
    log::debug!("Updated shortcuts map with {} entries", map.len());
}

type ShortcutMap = Arc<RwLock<HashMap<PortalShortcutId, String>>>;
