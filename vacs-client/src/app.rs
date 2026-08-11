use crate::app::state::AppState;
use crate::app::window::WindowProvider;
use crate::config::AppConfig;
use crate::config::BackendEndpoint;
use crate::error::Error;
use crate::keybinds::JoystickDevice;
use crate::keybinds::{KeybindsConfig, TransmitConfig};
use crate::playback::PlaybackConfig;
use crate::radio::RadioConfig;
use crate::remote::RemoteConfig;
use anyhow::Context;
use rfd::{MessageButtons, MessageDialogResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;
use vacs_macros::Frontend;
use vacs_signaling::protocol::http::version::ReleaseChannel;
use vacs_signaling::protocol::profile::client_page::{
    ClientGroupMode, ClientPageConfig, FrequencyDisplayMode,
};
use vacs_signaling::protocol::vatsim::ClientId;

pub(crate) mod commands;
pub(crate) mod state;
pub(crate) mod window;

#[cfg(not(feature = "e2e"))]
pub fn handle_deep_link(app: AppHandle, url: String) {
    use tauri::Emitter;

    let url = url.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = crate::auth::handle_auth_callback(&app, &url).await {
            app.emit("auth:error", serde_json::Value::Null).ok();
            app.emit::<crate::error::FrontendError>("error", err.into())
                .ok();
        }
    });
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_version: Option<String>,
    required: bool,
}

pub async fn get_update(app: &AppHandle) -> Result<Option<Update>, Error> {
    let state = app.state::<AppState>();
    let state = state.lock().await;
    let channel = &state.config.client.release_channel;
    let updater_url = state
        .config
        .backend
        .endpoint_url(&BackendEndpoint::VersionUpdateCheck)
        .replace("{{channel}}", channel.as_str());

    log::info!("Checking for update at {updater_url}...");

    Ok(app
        .updater_builder()
        .endpoints(vec![
            Url::parse(&updater_url).context("Failed to parse update url")?,
        ])
        .context("Failed to set update url")?
        .build()
        .context("Failed to build updater")?
        .check()
        .await
        .context("Failed to check for updates")?)
}

pub fn open_fatal_error_dialog(app: &AppHandle, msg: &str) {
    let open_logs = "Open logs folder";
    let result = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("vacs - Fatal error")
        .set_description(msg)
        .set_buttons(MessageButtons::OkCancelCustom(
            open_logs.to_string(),
            "Close".to_string(),
        ))
        .show_blocking();

    match result {
        MessageDialogResult::Custom(text) if text == open_logs => {
            if let Err(err) = open_app_folder(app, AppFolder::Logs) {
                log::error!("Failed to open logs folder: {err}");

                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("vacs - Fatal error")
                    .set_description("Failed to open logs folder.")
                    .show_blocking();
            }
        }
        _ => {}
    };
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum AppFolder {
    Config,
    Logs,
}

pub fn open_app_folder(app: &AppHandle, folder: AppFolder) -> Result<(), Error> {
    let folder_path = match folder {
        AppFolder::Config => app
            .path()
            .app_config_dir()
            .context("Failed to get config folder")?,
        AppFolder::Logs => app
            .path()
            .app_log_dir()
            .context("Failed to get logs folder")?,
    };
    let folder_path = folder_path.to_str().context("Folder path is empty")?;

    app.opener()
        .open_path(folder_path, None::<&str>)
        .context("Failed to open folder")?;

    Ok(())
}

trait BlockingMessageDialog {
    fn show_blocking(self) -> MessageDialogResult;
}

impl BlockingMessageDialog for rfd::MessageDialog {
    #[cfg(not(target_os = "macos"))]
    fn show_blocking(self) -> MessageDialogResult {
        use std::sync::mpsc::sync_channel;

        let (tx, rx) = sync_channel(0);

        std::thread::spawn(move || {
            let result = self.show();
            tx.send(result).unwrap();
        });

        rx.recv().unwrap()
    }

    #[cfg(target_os = "macos")]
    fn show_blocking(self) -> MessageDialogResult {
        self.show()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub always_on_top: bool,
    pub fullscreen: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<PhysicalPosition<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<PhysicalSize<u32>>,
    pub release_channel: ReleaseChannel,
    pub signaling_auto_reconnect: bool,
    pub transmit_config: TransmitConfig,
    pub radio: RadioConfig,
    pub auto_hangup_seconds: u64,
    /// List of peer IDs (CIDs) that should be ignored by the client.
    ///
    /// Any incoming calls initiated by a CID in this list will be silently ignored
    /// by the client. This does **not** completely block communications with ignored
    /// parties as the (local) user can still actively initiate calls to them.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub ignored: HashSet<ClientId>,
    #[serde(default)]
    pub keybinds: KeybindsConfig,
    #[serde(default)]
    pub call: CallConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_client_page_config: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_client_page_config: Option<String>,
    pub test_profile_watcher_delay_ms: u64,
    #[serde(default)]
    pub remote: RemoteConfig,
    #[serde(default)]
    pub playback: PlaybackConfig,
    #[serde(default = "default_zoom_level")]
    pub zoom_level: f64,
    #[serde(default)]
    pub clock_mode: ClockMode,
    #[serde(default)]
    pub cpl_mode: CplMode,
    /// Joystick devices excluded from binding capture. Existing bindings on
    /// these devices keep working; they just can no longer win a capture.
    #[serde(default)]
    pub ignored_joysticks: HashSet<JoystickDevice>,
}

fn default_zoom_level() -> f64 {
    1.0f64
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            always_on_top: false,
            fullscreen: false,
            position: None,
            size: None,
            release_channel: ReleaseChannel::default(),
            signaling_auto_reconnect: true,
            transmit_config: TransmitConfig::default(),
            radio: RadioConfig::default(),
            auto_hangup_seconds: 60,
            ignored: HashSet::new(),
            keybinds: KeybindsConfig::default(),
            call: CallConfig::default(),
            selected_client_page_config: None,
            extra_client_page_config: None,
            test_profile_watcher_delay_ms: 500,
            remote: RemoteConfig::default(),
            playback: PlaybackConfig::default(),
            zoom_level: 1.0f64,
            clock_mode: ClockMode::default(),
            cpl_mode: CplMode::default(),
            ignored_joysticks: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ClockMode {
    #[default]
    Realtime,
    Relaxed,
    Day,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CplMode {
    #[default]
    Original,
    Fast,
}

impl ClientConfig {
    pub fn max_signaling_reconnect_attempts(&self) -> u8 {
        if self.signaling_auto_reconnect { 8 } else { 0 }
    }

    pub fn default_window_size<P>(provider: &P) -> Result<PhysicalSize<u32>, Error>
    where
        P: WindowProvider + ?Sized,
    {
        Ok(LogicalSize::new(
            1000.0f64,
            if cfg!(target_os = "macos") {
                781.0f64
            } else {
                753.0f64
            },
        )
        .to_physical(provider.scale_factor()?))
    }

    pub fn update_window_state<P>(&mut self, provider: &P) -> Result<(), Error>
    where
        P: WindowProvider + ?Sized,
    {
        let window = provider.window()?;
        if window.is_minimized().unwrap_or(false) || window.is_maximized().unwrap_or(false) {
            log::debug!("Window is minimized or maximized, skipping window state update");
            return Ok(());
        }

        let size = window.size()?;
        if size.width == 0 || size.height == 0 {
            log::debug!("Window size is 0, skipping window state update");
            return Ok(());
        }

        let position = window.position()?;

        self.position = Some(position);
        self.size = Some(size);

        log::debug!(
            "Updating window position to {:?} and size to {:?}",
            self.position.unwrap(),
            self.size.unwrap()
        );
        Ok(())
    }

    pub fn restore_window_state<P>(&self, provider: &P) -> Result<(), Error>
    where
        P: WindowProvider + ?Sized,
    {
        let window = provider.window()?;

        log::debug!(
            "Restoring window position to {:?} and size to {:?}",
            self.position,
            self.size
        );

        if let Some(position) = self.position {
            for m in window
                .available_monitors()
                .context("Failed to get available monitors")?
            {
                let PhysicalPosition { x, y } = *m.position();
                let PhysicalSize { width, height } = *m.size();

                let left = x;
                let right = x + width as i32;
                let top = y;
                let bottom = y + height as i32;

                let size = self.size.unwrap_or(Self::default_window_size(&window)?);

                let intersects = [
                    (position.x, position.y),
                    (position.x + size.width as i32, position.y),
                    (position.x, position.y + size.height as i32),
                    (
                        position.x + size.width as i32,
                        position.y + size.height as i32,
                    ),
                ]
                .into_iter()
                .any(|(x, y)| x >= left && x < right && y >= top && y < bottom);

                if intersects {
                    window
                        .set_position(position)
                        .context("Failed to set main window position")?;
                    break;
                }
            }
        }

        if let Some(mut size) = self.size {
            if size.width == 0 || size.height == 0 {
                log::warn!("Window size {size:?} is 0, restoring default size");
                size = Self::default_window_size(&window)?;
            }

            window
                .set_size(size)
                .context("Failed to set main window size")?;

            #[cfg(target_os = "linux")]
            {
                log::debug!("Verifying correct window size after decorations apply");

                // This timeout is **absolutely crucial** as the window manager does not update the
                // window size immediately after a resize has been requested, but only after a short
                // delay. If we were to compare the window size immediately after resizing, we would
                // always receive the expected values, however, the window manager would still apply
                // decorations later, changing the actual size, which is then incorrectly persisted.
                // This will result in a short "flicker" of the window size, which we would optimally
                // hide by simply not showing the window until we're sure its size is correct. However,
                // since there's another bug that prevents the menu bar from being interactable if the
                // window is initialized hidden, which is even less desirable, we'll have to live with
                // the flicker for now.
                // Upstream tauri/tao issues related to this:
                // - https://github.com/tauri-apps/tao/issues/929
                // - https://github.com/tauri-apps/tao/pull/1055
                std::thread::sleep(std::time::Duration::from_millis(50));
                let actual_size = window.inner_size().context("Failed to get window size")?;

                let width_diff = actual_size.width.saturating_sub(size.width);
                let height_diff = actual_size.height.saturating_sub(size.height);

                if width_diff > 0 || height_diff > 0 {
                    log::warn!(
                        "Window size changed after decorations apply, expected: {size:?}, got: {actual_size:?}. Resizing again"
                    );
                    window
                        .set_size(PhysicalSize::new(
                            size.width.saturating_sub(width_diff),
                            size.height.saturating_sub(height_diff),
                        ))
                        .context("Failed to fix main window size")?;
                }
            }
        }

        Ok(())
    }
}

/// Various settings regarding calls.
#[derive(Debug, Clone, Serialize, Deserialize, Frontend)]
pub struct CallConfig {
    /// Toggles highlighting of incoming call target DA keys.
    #[serde(default = "default_true")]
    pub highlight_incoming_call_target: bool,
    /// Enables the priority call ringtone and visual highlighting. If disabled, Priority calls will still be received, but not handled differently.
    #[serde(default = "default_true")]
    pub enable_priority_calls: bool,
    /// Enables sound effect when a call is established
    #[serde(default = "default_true")]
    pub enable_call_start_sound: bool,
    /// Enables sound effect when the call is ended
    #[serde(default = "default_true")]
    pub enable_call_end_sound: bool,
    /// Enables default call source selection based on the dataset position
    #[serde(default = "default_true")]
    pub use_default_call_sources: bool,
    /// Forces call audio to always be relayed via a TURN server instead of attempting a direct
    /// peer-to-peer connection. Helps with one-way audio issues caused by VPNs (e.g. Cloudflare
    /// WARP) at the cost of slightly higher latency.
    #[serde(default)]
    pub force_relay: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CallConfig {
    fn default() -> Self {
        Self {
            highlight_incoming_call_target: true,
            enable_priority_calls: true,
            enable_call_start_sound: true,
            enable_call_end_sound: true,
            use_default_call_sources: true,
            force_relay: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientPageSettings {
    /// Named configs for different client page configurations.
    /// Users can switch between configs in the UI.
    #[serde(default)]
    pub configs: HashMap<String, ClientPageConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendClientPageSettings {
    selected: Option<String>,
    configs: HashMap<String, FrontendClientPageConfig>,
}

impl From<&AppConfig> for FrontendClientPageSettings {
    fn from(config: &AppConfig) -> Self {
        FrontendClientPageSettings {
            selected: config.client.selected_client_page_config.clone(),
            configs: config
                .client_page
                .configs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendClientPageConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub priority: Vec<String>,
    pub frequencies: FrequencyDisplayMode,
    pub grouping: ClientGroupMode,
}

impl Default for FrontendClientPageConfig {
    fn default() -> Self {
        Self::from(ClientPageConfig::default())
    }
}

impl From<ClientPageConfig> for FrontendClientPageConfig {
    fn from(client_page_config: ClientPageConfig) -> Self {
        Self {
            include: client_page_config.include,
            exclude: client_page_config.exclude,
            priority: client_page_config.priority,
            frequencies: client_page_config.frequencies,
            grouping: client_page_config.grouping,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PersistedClientConfig {
    pub client: ClientConfig,
}

impl From<ClientConfig> for PersistedClientConfig {
    fn from(client: ClientConfig) -> Self {
        Self { client }
    }
}
