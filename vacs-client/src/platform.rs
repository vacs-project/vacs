use serde::Serialize;
use std::fmt::Display;
use std::sync::OnceLock;

/// Platform capabilities that determine which features are available.
///
/// Different platforms have different capabilities due to OS-level restrictions:
/// - **Windows/macOS**: Full keybind listener and emitter support
/// - **Linux Wayland**: Listener support via XDG portal, but no emitter (security model)
/// - **Linux X11**: Listener via XInput2 raw events
/// - **Linux Unknown**: No display server detected, stub implementations
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Capabilities {
    pub always_on_top: bool,
    pub keybind_listener: bool,
    pub keybind_emitter: bool,
    pub joystick: bool,
    pub playback: bool,

    pub platform: Platform,
}

static CAPABILITIES_CACHE: OnceLock<Capabilities> = OnceLock::new();

#[allow(dead_code)]
impl Capabilities {
    pub fn get() -> &'static Self {
        CAPABILITIES_CACHE.get_or_init(Self::detect)
    }

    fn detect() -> Self {
        let platform = *Platform::get();

        #[cfg(target_os = "linux")]
        let keybind_listener = match platform {
            Platform::LinuxWayland => check_wayland_global_shortcuts_portal(),
            // XInput2 raw events need no permission or portal
            Platform::LinuxX11 => true,
            _ => false,
        };

        #[cfg(not(target_os = "linux"))]
        let keybind_listener = matches!(platform, Platform::Windows | Platform::MacOs);

        let playback = cfg_select! {
            target_os = "linux" => {
                check_pipewire()
            }
            target_os = "windows" => {
                true
            }
            target_os = "macos" => {
                false
            }
        };

        Self {
            always_on_top: !matches!(platform, Platform::LinuxWayland),
            keybind_listener,
            // X11 emits via the XTest extension; Wayland has no injection API
            keybind_emitter: matches!(
                platform,
                Platform::Windows | Platform::MacOs | Platform::LinuxX11
            ),
            // SDL's joystick subsystem needs no display server or special
            // permissions on any supported platform; runtime failures surface
            // (and are logged) when the joystick service is first used.
            joystick: !matches!(platform, Platform::Unknown),
            playback,
            platform,
        }
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        *Self::get()
    }
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn check_wayland_global_shortcuts_portal() -> bool {
    log::debug!("Checking availability of Wayland Global Shortcuts portal");

    let probe_portal = async || -> bool {
        use ashpd::desktop::global_shortcuts::GlobalShortcuts;

        match tokio::time::timeout(std::time::Duration::from_secs(1), GlobalShortcuts::new()).await
        {
            Ok(Ok(_)) => {
                log::debug!("Wayland Global Shortcuts portal is available");
                true
            }
            Ok(Err(err)) => {
                log::warn!("Wayland Global Shortcuts portal check failed: {err}");
                false
            }
            Err(_) => {
                log::warn!("Wayland Global Shortcuts portal check timed out");
                false
            }
        }
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(probe_portal()))
    } else {
        tauri::async_runtime::block_on(probe_portal())
    }
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn check_pipewire() -> bool {
    let socket_path = std::env::var("PIPEWIRE_RUNTIME_DIR")
        .ok()
        .or_else(|| std::env::var("XDG_RUNTIME_DIR").ok())
        .map(|dir| {
            let name = std::env::var("PIPEWIRE_REMOTE").unwrap_or("pipewire-0".to_string());
            std::path::PathBuf::from(dir).join(name)
        });

    let Some(socket_path) = socket_path else {
        log::warn!("PipeWire socket path could not be determined");
        return false;
    };

    if !socket_path.exists() {
        log::debug!("PipeWire socket not found at {}", socket_path.display());
        return false;
    }

    log::debug!(
        "PipeWire socket found at {}, probing daemon",
        socket_path.display()
    );

    let probe_daemon = async || -> bool {
        match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            tokio::task::spawn_blocking(|| {
                pipewire::init();
                let Ok(mainloop) = pipewire::main_loop::MainLoopBox::new(None) else {
                    return false;
                };
                let Ok(context) = pipewire::context::ContextBox::new(mainloop.loop_(), None) else {
                    return false;
                };
                context.connect(None).is_ok()
            }),
        )
        .await
        {
            Ok(Ok(connected)) => {
                if connected {
                    log::debug!("PipeWire is available");
                } else {
                    log::warn!("PipeWire daemon probe failed");
                }
                connected
            }
            Ok(Err(err)) => {
                log::warn!("PipeWire check failed: {err}");
                false
            }
            Err(_) => {
                log::warn!("PipeWire check timed out");
                false
            }
        }
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(probe_daemon()))
    } else {
        tauri::async_runtime::block_on(probe_daemon())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub enum Platform {
    Unknown,
    Windows,
    MacOs,
    LinuxX11,
    LinuxWayland,
    LinuxUnknown,
}

/// Cached platform detection result.
///
/// Platform detection reads environment variables, which is relatively expensive.
/// Since the platform cannot change during application runtime (you can't switch
/// from X11 to Wayland without restarting the app), we cache the result using
/// `OnceLock` for thread-safe lazy initialization.
static PLATFORM_CACHE: OnceLock<Platform> = OnceLock::new();

impl Platform {
    /// Get the current platform, using a cached value if available.
    ///
    /// This is the preferred method for platform detection as it avoids repeated
    /// environment variable lookups. The first call will perform detection and
    /// cache the result; subsequent calls return the cached value.
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and can be called from multiple threads
    /// simultaneously. Only one thread will perform the actual detection.
    pub fn get() -> &'static Self {
        PLATFORM_CACHE.get_or_init(Self::detect)
    }

    /// Detect the current platform by examining environment variables.
    ///
    /// This method performs the actual platform detection logic. It should not
    /// be called directly in most cases; use `Platform::get()` instead to benefit
    /// from caching.
    ///
    /// # Detection Strategy
    ///
    /// On Linux, we check in order:
    /// 1. `XDG_SESSION_TYPE` environment variable (most reliable)
    /// 2. `WAYLAND_DISPLAY` environment variable (fallback for Wayland)
    /// 3. `DISPLAY` environment variable (fallback for X11)
    /// 4. If none are set, return `LinuxUnknown`
    fn detect() -> Self {
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }

        #[cfg(target_os = "macos")]
        {
            Platform::MacOs
        }

        #[cfg(target_os = "linux")]
        {
            use std::env;
            if let Ok(xdg_session_type) = env::var("XDG_SESSION_TYPE") {
                match xdg_session_type.to_lowercase().as_str() {
                    "wayland" => return Platform::LinuxWayland,
                    "x11" => return Platform::LinuxX11,
                    _ => {}
                }
            }

            if env::var("WAYLAND_DISPLAY").is_ok() {
                Platform::LinuxWayland
            } else if env::var("DISPLAY").is_ok() {
                Platform::LinuxX11
            } else {
                Platform::LinuxUnknown
            }
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            Platform::Unknown
        }
    }

    #[allow(dead_code)]
    pub fn is_linux(&self) -> bool {
        matches!(
            self,
            Platform::LinuxX11 | Platform::LinuxWayland | Platform::LinuxUnknown
        )
    }

    #[allow(dead_code)]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::MacOs => "MacOs",
            Platform::LinuxX11 => "LinuxX11",
            Platform::LinuxWayland => "LinuxWayland",
            Platform::LinuxUnknown => "LinuxUnknown",
            Platform::Unknown => "Unknown",
        }
    }
}

impl Default for Platform {
    fn default() -> Self {
        *Self::get()
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Desktop environment detected on Linux.
///
/// This enum represents the various desktop environments that can be detected
/// on Linux systems. Detection is based on the `XDG_CURRENT_DESKTOP` environment
/// variable.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub enum DesktopEnvironment {
    /// KDE Plasma desktop environment
    Kde,
    /// GNOME desktop environment
    Gnome,
    /// XFCE desktop environment
    Xfce,
    /// Hyprland Wayland compositor
    Hyprland,
    /// Unknown or unsupported desktop environment
    Unknown,
}

/// Cached desktop environment detection result.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
static DESKTOP_ENV_CACHE: OnceLock<DesktopEnvironment> = OnceLock::new();

#[cfg(target_os = "linux")]
impl DesktopEnvironment {
    /// Get the current desktop environment, using a cached value if available.
    ///
    /// This method detects the desktop environment by examining the `XDG_CURRENT_DESKTOP`
    /// environment variable. The result is cached for subsequent calls.
    ///
    /// # Platform Support
    ///
    /// This method only performs detection on Linux. On other platforms, it returns
    /// `DesktopEnvironment::Unknown`.
    #[allow(dead_code)]
    pub fn get() -> DesktopEnvironment {
        *DESKTOP_ENV_CACHE.get_or_init(Self::detect)
    }

    /// Detect the current desktop environment.
    ///
    /// This method performs the actual detection logic by examining the
    /// `XDG_CURRENT_DESKTOP` environment variable.
    #[allow(dead_code)]
    fn detect() -> DesktopEnvironment {
        #[cfg(target_os = "linux")]
        {
            use std::env;
            let desktop = env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .to_lowercase();

            if desktop.contains("kde") || desktop.contains("plasma") {
                DesktopEnvironment::Kde
            } else if desktop.contains("gnome") {
                DesktopEnvironment::Gnome
            } else if desktop.contains("xfce") {
                DesktopEnvironment::Xfce
            } else if desktop.contains("hyprland") {
                DesktopEnvironment::Hyprland
            } else {
                DesktopEnvironment::Unknown
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            DesktopEnvironment::Unknown
        }
    }

    /// Open the keyboard shortcuts settings for this desktop environment.
    ///
    /// This method attempts to open the appropriate system settings page for
    /// configuring keyboard shortcuts. The exact behavior depends on the desktop
    /// environment:
    ///
    /// - **KDE**: Opens System Settings → Shortcuts (`systemsettings5 kcm_keys`)
    /// - **GNOME**: Opens Settings → Keyboard (`gnome-control-center keyboard`)
    /// - **XFCE**: Opens Keyboard Settings (`xfce4-keyboard-settings`)
    /// - **Hyprland**: Opens config file in default editor (`xdg-open ~/.config/hypr/hyprland.conf`)
    /// - **Unknown**: Tries generic fallbacks (`xdg-open settings://keyboard`)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The platform is not Linux
    /// - The desktop environment's settings application cannot be launched
    #[allow(dead_code)]
    pub fn open_keyboard_shortcuts_settings(&self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            // These all live on the host, so they must not inherit our bundle's environment.
            use crate::external::host_command as command;

            log::debug!("Opening keyboard shortcuts settings for {:?}", self);

            let result = match self {
                DesktopEnvironment::Kde => {
                    // KDE Plasma: Open System Settings to Shortcuts page
                    log::debug!("Opening KDE System Settings shortcuts page");
                    command("systemsettings5")
                        .arg("kcm_keys")
                        .spawn()
                        .or_else(|_| {
                            // Fallback for KDE 6
                            command("systemsettings").arg("kcm_keys").spawn()
                        })
                }
                DesktopEnvironment::Gnome => {
                    // GNOME: Open Settings to Keyboard Shortcuts
                    log::debug!("Opening GNOME Settings keyboard shortcuts");
                    command("gnome-control-center").arg("keyboard").spawn()
                }
                DesktopEnvironment::Xfce => {
                    // XFCE: Open Keyboard Settings
                    log::debug!("Opening XFCE keyboard settings");
                    command("xfce4-keyboard-settings").spawn()
                }
                DesktopEnvironment::Hyprland => {
                    // Hyprland: Open config file in default editor
                    // Hyprland doesn't have a traditional settings GUI, it's configured via text files
                    log::debug!("Opening Hyprland config file");

                    // Follow XDG Base Directory specification
                    let config_path = if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
                        format!("{}/hypr/hyprland.conf", xdg_config)
                    } else if let Ok(home) = std::env::var("HOME") {
                        format!("{}/.config/hypr/hyprland.conf", home)
                    } else {
                        "~/.config/hypr/hyprland.conf".to_string()
                    };

                    command("xdg-open").arg(&config_path).spawn()
                }
                DesktopEnvironment::Unknown => {
                    // Unknown DE: Try generic approaches
                    log::debug!("Unknown DE, trying generic keyboard settings");

                    // Try xdg-open with settings:// URI (some DEs support this)
                    command("xdg-open")
                        .arg("settings://keyboard")
                        .spawn()
                        .or_else(|_| {
                            // Fallback: just open system settings
                            command("xdg-settings")
                                .arg("get")
                                .arg("default-url-scheme-handler")
                                .arg("settings")
                                .spawn()
                        })
                }
            };

            match result {
                Ok(child) => {
                    crate::external::reap_detached(child);
                    log::info!("Successfully opened keyboard shortcuts settings");
                    Ok(())
                }
                Err(err) => {
                    log::warn!("Failed to open keyboard shortcuts settings: {}", err);
                    Err(format!(
                        "Failed to open system settings. Please open your desktop environment's keyboard shortcuts settings manually. Error: {}",
                        err
                    ))
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err("Opening keyboard shortcuts settings is only supported on Linux".to_string())
        }
    }

    #[allow(dead_code)]
    pub const fn as_str(&self) -> &'static str {
        match self {
            DesktopEnvironment::Kde => "KDE",
            DesktopEnvironment::Gnome => "GNOME",
            DesktopEnvironment::Xfce => "XFCE",
            DesktopEnvironment::Hyprland => "Hyprland",
            DesktopEnvironment::Unknown => "Unknown",
        }
    }
}
