//! Wayland keybind listener implementation using XDG Global Shortcuts portal.
//!
//! # Overview
//!
//! This module implements global keybind listening for Wayland compositors using the
//! [XDG Desktop Portal](https://flatpak.github.io/xdg-desktop-portal/) Global Shortcuts API.
//!
//! ## Why XDG Portal?
//!
//! Unlike X11 where applications can directly listen to global keyboard events, Wayland's
//! security model requires applications to request permission from the compositor. The
//! XDG Desktop Portal provides a standardized D-Bus API for this purpose.
//!
//! ## Compositor Support
//!
//! This implementation works on compositors that support the Global Shortcuts portal:
//! - KDE Plasma (via `xdg-desktop-portal-kde`)
//! - GNOME (via `xdg-desktop-portal-gnome`)
//! - Hyprland (via `xdg-desktop-portal-hyprland`)
//!
//! ## Code Mapping Strategy
//!
//! The portal allows complex key combinations (e.g., `Ctrl+Alt+Shift+P`) that cannot be
//! represented as a single `keyboard_types::Code`. To work around this, we map each
//! transmit mode to a unique function key:
//!
//! - `ToggleRadioPrio` → `Code::F31`
//! - `CallControl` → `Code::F32`
//! - `PushToTalk` → `Code::F33`
//! - `PushToMute` → `Code::F34`
//! - `RadioIntegration` → `Code::F35`
//!
//! These keys don't exist on most keyboards, avoiding conflicts with user input. When the
//! portal activates a shortcut, we emit the corresponding F-key code, and the rest of the
//! keybind engine works unchanged.
//!
//! ## User Experience
//!
//! 1. On first launch, the compositor shows a configuration dialog
//! 2. User configures their preferred key combinations
//! 3. Shortcuts are stored by the compositor and persist across app restarts
//! 4. User can reconfigure shortcuts in their desktop environment settings

mod listener;
pub use listener::*;

use crate::keybinds::Keybind;
use ashpd::desktop::global_shortcuts::NewShortcut;
use keyboard_types::Code;
use std::str::FromStr;

/// Identifiers for shortcuts registered with the XDG Global Shortcuts portal.
///
/// Each variant corresponds to a transmit mode in vacs. These IDs are used to:
/// - Register shortcuts with the portal
/// - Identify which shortcut was activated in portal signals
/// - Query the current key binding from the portal

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortalShortcutId {
    PushToTalk,
    PushToMute,
    RadioIntegration,
    CallControl,
    ToggleRadioPrio,
}

impl PortalShortcutId {
    pub const fn as_str(&self) -> &'static str {
        match self {
            PortalShortcutId::PushToTalk => "push_to_talk",
            PortalShortcutId::PushToMute => "push_to_mute",
            PortalShortcutId::RadioIntegration => "radio_integration",
            PortalShortcutId::CallControl => "call_control",
            PortalShortcutId::ToggleRadioPrio => "toggle_radio_prio",
        }
    }

    pub const fn description(&self) -> &'static str {
        match self {
            PortalShortcutId::PushToTalk => "Push-to-talk (activate voice transmission while held)",
            PortalShortcutId::PushToMute => "Push-to-mute (mute microphone while held)",
            PortalShortcutId::RadioIntegration => "Radio Integration",
            PortalShortcutId::CallControl => "Call Control (end active/accept next)",
            PortalShortcutId::ToggleRadioPrio => "Toggle Radio Priority (during active call)",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            PortalShortcutId::PushToTalk,
            PortalShortcutId::PushToMute,
            PortalShortcutId::RadioIntegration,
            PortalShortcutId::CallControl,
            PortalShortcutId::ToggleRadioPrio,
        ]
    }

    pub const fn from_transmit_mode(mode: crate::config::TransmitMode) -> Option<Self> {
        match mode {
            crate::config::TransmitMode::PushToTalk => Some(PortalShortcutId::PushToTalk),
            crate::config::TransmitMode::PushToMute => Some(PortalShortcutId::PushToMute),
            crate::config::TransmitMode::RadioIntegration => {
                Some(PortalShortcutId::RadioIntegration)
            }
            _ => None,
        }
    }
}

impl FromStr for PortalShortcutId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "push_to_talk" => Ok(PortalShortcutId::PushToTalk),
            "push_to_mute" => Ok(PortalShortcutId::PushToMute),
            "radio_integration" => Ok(PortalShortcutId::RadioIntegration),
            "call_control" => Ok(PortalShortcutId::CallControl),
            "toggle_radio_prio" => Ok(PortalShortcutId::ToggleRadioPrio),
            _ => Err(format!("unknown portal shortcut id {s}")),
        }
    }
}

impl TryFrom<&str> for PortalShortcutId {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for PortalShortcutId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.as_str().parse()
    }
}

impl AsRef<str> for PortalShortcutId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&PortalShortcutId> for NewShortcut {
    fn from(value: &PortalShortcutId) -> Self {
        NewShortcut::new(value.as_str(), value.description())
    }
}

impl From<PortalShortcutId> for NewShortcut {
    fn from(value: PortalShortcutId) -> Self {
        NewShortcut::new(value.as_str(), value.description())
    }
}

impl From<PortalShortcutId> for Code {
    fn from(value: PortalShortcutId) -> Self {
        match value {
            PortalShortcutId::ToggleRadioPrio => Code::F31,
            PortalShortcutId::CallControl => Code::F32,
            PortalShortcutId::PushToTalk => Code::F33,
            PortalShortcutId::PushToMute => Code::F34,
            PortalShortcutId::RadioIntegration => Code::F35,
        }
    }
}

impl TryFrom<Code> for PortalShortcutId {
    type Error = String;
    fn try_from(value: Code) -> Result<Self, Self::Error> {
        match value {
            Code::F31 => Ok(PortalShortcutId::ToggleRadioPrio),
            Code::F32 => Ok(PortalShortcutId::CallControl),
            Code::F33 => Ok(PortalShortcutId::PushToTalk),
            Code::F34 => Ok(PortalShortcutId::PushToMute),
            Code::F35 => Ok(PortalShortcutId::RadioIntegration),
            _ => Err(format!("unknown portal shortcut code {value}")),
        }
    }
}

impl From<Keybind> for PortalShortcutId {
    fn from(value: Keybind) -> Self {
        match value {
            Keybind::PushToTalk => PortalShortcutId::PushToTalk,
            Keybind::PushToMute => PortalShortcutId::PushToMute,
            Keybind::RadioIntegration => PortalShortcutId::RadioIntegration,
            Keybind::AcceptCall => PortalShortcutId::CallControl,
            Keybind::EndCall => PortalShortcutId::CallControl,
            Keybind::ToggleRadioPrio => PortalShortcutId::ToggleRadioPrio,
        }
    }
}
