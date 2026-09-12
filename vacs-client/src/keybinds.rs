use crate::error::Error as VacsError;
use keyboard_types::{Code, KeyState};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::hash::Hash;
use thiserror::Error;
use vacs_macros::Frontend;

#[cfg(target_os = "linux")]
use crate::platform::Platform;

pub mod commands;
pub mod engine;
pub mod joystick;
pub mod runtime;

#[derive(Debug, Clone, Error)]
pub enum KeybindsError {
    #[error("Keybinds listener error: {0}")]
    Listener(String),
    #[error("Keybinds emitter error: {0}")]
    Emitter(String),
    #[error("Unrecognized keybinds code: {0}")]
    UnrecognizedCode(String),
    // Windows keybind engine is the only one emitting FakeMarker for composite key presses (AltGr)
    #[error("Fake marker")]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    FakeMarker,
    #[error("{0}")]
    Other(String),
}

/// A physical input that can be bound to a keybind action.
///
/// This is the *persisted* identity of a binding: keyboard keys serialize as their
/// bare [`Code`] string (e.g. `"KeyA"`), byte-identical to the pre-gamepad on-disk
/// format, while joystick buttons serialize as a `{ device, button, name }` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputCode {
    Key(Code),
    Button(JoystickButton),
}

impl fmt::Display for InputCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputCode::Key(code) => write!(f, "{code}"),
            InputCode::Button(button) => write!(f, "{button}"),
        }
    }
}

impl Serialize for InputCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            InputCode::Key(code) => serializer.serialize_str(&code.to_string()),
            InputCode::Button(button) => button.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for InputCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InputCodeVisitor;

        impl<'de> Visitor<'de> for InputCodeVisitor {
            type Value = InputCode;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a key code string or a joystick button table")
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<InputCode, E> {
                s.parse::<Code>()
                    .map(InputCode::Key)
                    .map_err(|_| E::custom(format!("Unrecognized key code: {s}")))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<InputCode, A::Error> {
                JoystickButton::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(InputCode::Button)
            }
        }

        // A manual visitor instead of #[serde(untagged)]: the `config` crate
        // re-deserializes through `config::Value`, where untagged buffering is
        // fragile, and untagged error messages are useless to users.
        deserializer.deserialize_any(InputCodeVisitor)
    }
}

/// A joystick/gamepad button, addressed by stable device identity.
///
/// Devices are identified by their SDL joystick GUID (derived from bus/VID/PID),
/// which is stable across reconnects and USB ports - unlike SDL instance ids,
/// which change on every reconnect and thus cannot be persisted. Two physically
/// identical devices share a GUID, so a binding matches either of them; distinct
/// products (yoke, throttle, pedals) always have distinct GUIDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoystickButton {
    /// SDL joystick GUID as a hex string.
    pub device: String,
    /// Raw button index on the device (SDL joystick button numbering).
    pub button: u32,
    /// Last-seen human-readable device name; display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl PartialEq for JoystickButton {
    fn eq(&self, other: &Self) -> bool {
        // `name` is display metadata and must not affect binding identity
        self.device == other.device && self.button == other.button
    }
}

impl Eq for JoystickButton {}

impl fmt::Display for JoystickButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => write!(f, "Button {} ({name})", self.button),
            None => write!(f, "Button {} (Joystick)", self.button),
        }
    }
}

/// A joystick/gamepad device, addressed by its stable SDL GUID.
///
/// Used for the capture ignore list: devices with latched switches or
/// position-simulated buttons (common on flight sim throttles) would instantly
/// "capture" during binding, so users can exclude them. The GUID keeps the
/// entry valid across unplugs; `name` is retained for display while the device
/// is disconnected. Physically identical devices share a GUID and are ignored
/// together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoystickDevice {
    /// SDL joystick GUID as a hex string.
    pub device: String,
    /// Last-seen human-readable device name; display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl PartialEq for JoystickDevice {
    fn eq(&self, other: &Self) -> bool {
        // `name` is display metadata and must not affect device identity
        self.device == other.device
    }
}

impl Eq for JoystickDevice {}

impl Hash for JoystickDevice {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.device.hash(state);
    }
}

/// Runtime identity of a fired keybind source.
///
/// Never serialized - configs only ever store [`InputCode`]. The `Portal` variant
/// carries semantic XDG portal shortcut activations on Wayland, where keyboard
/// capture happens at the OS level and no physical key identity exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Input(InputCode),
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Portal(PortalAction),
}

/// Platform-neutral identifier of the Wayland portal shortcuts.
///
/// Mirrors the Linux-only `PortalShortcutId` so the keybind engine can compare
/// triggers without pulling `cfg(target_os = "linux")` types into its core logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum PortalAction {
    PushToTalk,
    PushToMute,
    RadioPushToTalk,
    CallControl,
    ToggleRadioPrio,
}

#[derive(Debug, Clone)]
pub struct KeyEvent {
    trigger: Trigger,
    #[allow(dead_code)]
    label: String,
    state: KeyState,
}

impl KeyEvent {
    /// Keyboard key event.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub(crate) fn key(code: Code, label: String, state: KeyState) -> Self {
        Self {
            trigger: Trigger::Input(InputCode::Key(code)),
            label,
            state,
        }
    }

    /// Joystick button event.
    pub(crate) fn button(button: JoystickButton, label: String, state: KeyState) -> Self {
        Self {
            trigger: Trigger::Input(InputCode::Button(button)),
            label,
            state,
        }
    }

    /// Wayland portal shortcut activation.
    #[cfg(target_os = "linux")]
    pub(crate) fn portal(action: PortalAction, label: String, state: KeyState) -> Self {
        Self {
            trigger: Trigger::Portal(action),
            label,
            state,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Keybind {
    PushToTalk,
    PushToMute,
    RadioPushToTalk,
    AcceptCall,
    EndCall,
    ToggleRadioPrio,
}

/// Parse an optional frontend key-code string (e.g. `"KeyA"`) into a [`Code`].
///
/// Returns a user-facing error if the string is not a recognized key code.
pub(crate) fn parse_key_code(code: Option<String>) -> Result<Option<Code>, VacsError> {
    code.map(|s| {
        s.parse::<Code>().map_err(|_| {
            VacsError::Other(Box::new(anyhow::anyhow!(
                "Unrecognized key code: {s}. Please report this error in our GitHub repository's issue tracker."
            )))
        })
    })
    .transpose()
}

/// Compose the active trigger for a keybind action on Wayland.
///
/// A configured joystick button takes precedence and replaces the OS-level
/// portal shortcut for the action; without one, the portal activation (if any)
/// drives the action. Configured keyboard codes are ignored - keyboard capture
/// is handled entirely by the portal.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn compose_wayland_trigger(
    portal: Option<PortalAction>,
    configured: &Option<InputCode>,
) -> Option<Trigger> {
    if let Some(button @ InputCode::Button(_)) = configured {
        return Some(Trigger::Input(button.clone()));
    }
    portal.map(Trigger::Portal)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
pub enum CallMicMode {
    #[default]
    VoiceActivation,
    PushToTalk,
    PushToMute,
}

/// Configuration for the transmission mode and associated keybinds.
#[derive(Debug, Clone, Serialize, Default, Frontend)]
pub struct TransmitConfig {
    /// The transmit mode to use.
    pub call_mic_mode: CallMicMode,
    /// Input binding for Push-to-Talk mode.
    /// Required if mode is `PushToTalk`.
    pub push_to_talk: Option<InputCode>,
    /// Input binding for Push-to-Mute mode.
    /// Required if mode is `PushToMute`.
    pub push_to_mute: Option<InputCode>,
    /// Input binding for Radio PTT.
    pub radio_push_to_talk: Option<InputCode>,
    #[serde(skip)]
    #[frontend(skip)]
    pub was_radio_integration: Option<bool>,
}

impl TransmitConfig {
    /// The configured input that should drive the call MIC action, disregarding
    /// the Wayland portal (which replaces keyboard bindings at the OS level).
    fn configured_call_input(&self) -> &Option<InputCode> {
        match self.call_mic_mode {
            CallMicMode::VoiceActivation => &None,
            CallMicMode::PushToTalk => &self.push_to_talk,
            CallMicMode::PushToMute => &self.push_to_mute,
        }
    }

    /// The trigger that drives the call MIC action, if any.
    pub fn active_call_trigger(&self) -> Option<Trigger> {
        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // On Wayland, keyboard shortcuts are configured at the OS level via the
            // XDG Global Shortcuts portal and arrive as semantic portal activations,
            // so configured keyboard codes are ignored here.
            let portal = match self.call_mic_mode {
                CallMicMode::VoiceActivation => None,
                CallMicMode::PushToTalk => Some(PortalAction::PushToTalk),
                CallMicMode::PushToMute => Some(PortalAction::PushToMute),
            };
            let trigger = compose_wayland_trigger(portal, self.configured_call_input());
            log::trace!(
                "Using trigger {trigger:?} for call mic mode {:?}",
                self.call_mic_mode
            );
            return trigger;
        }

        self.configured_call_input().clone().map(Trigger::Input)
    }

    /// The configured input that should drive radio TX, disregarding the Wayland
    /// portal. In `PushToTalk` mode without a dedicated radio binding, radio TX
    /// follows the call PTT binding; in `PushToMute` mode it follows the PTM
    /// binding.
    fn configured_radio_input(&self) -> Option<InputCode> {
        match self.call_mic_mode {
            CallMicMode::VoiceActivation => self.radio_push_to_talk.clone(),
            CallMicMode::PushToTalk => self
                .radio_push_to_talk
                .clone()
                .or_else(|| self.push_to_talk.clone()),
            CallMicMode::PushToMute => self.push_to_mute.clone(),
        }
    }

    /// The dedicated trigger for radio TX, if any.
    ///
    /// Off Wayland this is also the effective trigger: `configured_radio_input`
    /// already folds in the "no dedicated binding, follow the call PTT binding"
    /// case, so the result equals `active_call_trigger` when that applies.
    ///
    /// On Wayland with a keyboard binding it is only the dedicated portal
    /// action. Whether radio TX is following the call trigger instead is a
    /// runtime question, because the portal registers the radio shortcut
    /// whether or not a key is assigned to it. `radio_falls_back_to_call`
    /// reports whether the configuration allows that, and the engine resolves
    /// the live answer. Do not read this as "the trigger that keys the radio
    /// right now".
    pub fn active_radio_trigger(&self, enabled: bool) -> Option<Trigger> {
        if !enabled {
            return None;
        }

        #[cfg(target_os = "linux")]
        if matches!(Platform::get(), Platform::LinuxWayland) {
            // See active_call_trigger: portal activations replace keyboard codes.
            let portal = match self.call_mic_mode {
                CallMicMode::VoiceActivation | CallMicMode::PushToTalk => {
                    Some(PortalAction::RadioPushToTalk)
                }
                CallMicMode::PushToMute => Some(PortalAction::PushToMute),
            };
            let trigger = compose_wayland_trigger(portal, &self.configured_radio_input());
            log::trace!(
                "Using trigger {trigger:?} for radio in call mic mode {:?}",
                self.call_mic_mode
            );
            return trigger;
        }

        self.configured_radio_input().map(Trigger::Input)
    }

    /// Whether radio TX falls back to the call trigger while no dedicated radio
    /// shortcut is bound at the OS level.
    ///
    /// Only the Wayland portal has this ambiguity: the radio shortcut is
    /// registered whether or not the user assigned a key to it, so "is there a
    /// dedicated radio key" cannot be answered from configuration alone. The
    /// answer also changes while the app runs, whenever the user edits the
    /// binding in their desktop environment, so the engine resolves it from the
    /// listener's live view rather than caching it here.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub fn radio_falls_back_to_call(&self, enabled: bool) -> bool {
        #[cfg(target_os = "linux")]
        if enabled
            && matches!(Platform::get(), Platform::LinuxWayland)
            && matches!(self.call_mic_mode, CallMicMode::PushToTalk)
        {
            // A configured joystick button replaces the portal shortcut, so
            // there is nothing to fall back from.
            return !matches!(self.configured_radio_input(), Some(InputCode::Button(_)));
        }

        false
    }
}

impl<'de> Deserialize<'de> for TransmitConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize, Default)]
        enum TransmitMode {
            #[default]
            VoiceActivation,
            PushToTalk,
            PushToMute,
            RadioIntegration,
        }

        #[derive(Deserialize, Default)]
        struct TransmitConfigRaw {
            call_mic_mode: Option<CallMicMode>,
            mode: Option<TransmitMode>,
            push_to_talk: Option<InputCode>,
            push_to_mute: Option<InputCode>,
            radio_push_to_talk: Option<InputCode>,
        }

        let raw = TransmitConfigRaw::deserialize(deserializer)?;

        // Migrate old TransmitMode
        if let Some(mode) = raw.mode {
            let call_mic_mode = match mode {
                TransmitMode::VoiceActivation => CallMicMode::VoiceActivation,
                TransmitMode::PushToTalk | TransmitMode::RadioIntegration => {
                    CallMicMode::PushToTalk
                }
                TransmitMode::PushToMute => CallMicMode::PushToMute,
            };

            let is_radio_integration = matches!(mode, TransmitMode::RadioIntegration);

            let push_to_talk = if is_radio_integration {
                raw.radio_push_to_talk.clone()
            } else {
                raw.push_to_talk
            };

            return Ok(TransmitConfig {
                call_mic_mode,
                push_to_talk,
                push_to_mute: raw.push_to_mute,
                radio_push_to_talk: raw.radio_push_to_talk,
                was_radio_integration: Some(is_radio_integration),
            });
        }

        if let Some(call_mic_mode) = raw.call_mic_mode {
            return Ok(TransmitConfig {
                call_mic_mode,
                push_to_talk: raw.push_to_talk,
                push_to_mute: raw.push_to_mute,
                radio_push_to_talk: raw.radio_push_to_talk,
                was_radio_integration: None,
            });
        }

        Ok(TransmitConfig::default())
    }
}

/// Configuration for generic call control keybinds.
///
/// These keybinds allow accepting and ending calls as well as toggling radio prio without needing
/// to use the UI and can be used independently of the transmit mode.
#[derive(Debug, Clone, Serialize, Deserialize, Default, Frontend)]
pub struct KeybindsConfig {
    /// Input binding to accept an incoming call.
    pub accept_call: Option<InputCode>,
    /// Input binding to end an active call.
    pub end_call: Option<InputCode>,
    /// Input binding to toggle radio prio during an active call.
    pub toggle_radio_prio: Option<InputCode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(device: &str, button: u32, name: Option<&str>) -> InputCode {
        InputCode::Button(JoystickButton {
            device: device.to_string(),
            button,
            name: name.map(str::to_string),
        })
    }

    fn running_on_wayland() -> bool {
        #[cfg(target_os = "linux")]
        {
            matches!(
                crate::platform::Platform::get(),
                crate::platform::Platform::LinuxWayland
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    #[test]
    fn key_binding_serializes_as_bare_string() {
        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            push_to_talk: Some(InputCode::Key(Code::F13)),
            ..Default::default()
        };

        let toml = toml::to_string(&config).unwrap();
        assert!(
            toml.contains(r#"push_to_talk = "F13""#),
            "expected bare string serialization, got:\n{toml}"
        );
    }

    #[test]
    fn pre_gamepad_config_deserializes_unchanged() {
        let toml = r#"
            call_mic_mode = "PushToTalk"
            push_to_talk = "KeyA"
            radio_push_to_talk = "F24"
        "#;

        let config: TransmitConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.push_to_talk, Some(InputCode::Key(Code::KeyA)));
        assert_eq!(config.radio_push_to_talk, Some(InputCode::Key(Code::F24)));
        assert_eq!(config.push_to_mute, None);
    }

    #[test]
    fn button_binding_roundtrips_through_toml() {
        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            push_to_talk: Some(button(
                "030003f05e0400008e02000010010000",
                2,
                Some("VPC Throttle"),
            )),
            ..Default::default()
        };

        let toml = toml::to_string(&config).unwrap();
        let parsed: TransmitConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.push_to_talk, config.push_to_talk);
    }

    #[test]
    fn mixed_bindings_roundtrip_through_config_crate() {
        // The runtime config is loaded through the `config` crate (re-deserialized
        // via config::Value), which is why InputCode uses a manual visitor instead
        // of #[serde(untagged)]. Guard that path explicitly.
        let toml = r#"
            call_mic_mode = "PushToTalk"
            push_to_mute = "F22"

            [push_to_talk]
            device = "030003f05e0400008e02000010010000"
            button = 5
            name = "Peiker Handset"
        "#;

        let config: TransmitConfig = config::Config::builder()
            .add_source(config::File::from_str(toml, config::FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();

        assert_eq!(
            config.push_to_talk,
            Some(button(
                "030003f05e0400008e02000010010000",
                5,
                Some("Peiker Handset")
            ))
        );
        assert_eq!(config.push_to_mute, Some(InputCode::Key(Code::F22)));
    }

    #[test]
    fn unrecognized_key_code_errors() {
        let toml = r#"push_to_talk = "NotAKey""#;
        let err = toml::from_str::<TransmitConfig>(toml).unwrap_err();
        assert!(err.to_string().contains("Unrecognized key code"));
    }

    #[test]
    fn joystick_device_equality_ignores_name() {
        let device = |guid: &str, name: Option<&str>| JoystickDevice {
            device: guid.to_string(),
            name: name.map(str::to_string),
        };
        assert_eq!(device("guid", Some("Name A")), device("guid", None));
        assert_ne!(device("guid-a", None), device("guid-b", None));
    }

    #[test]
    fn joystick_button_equality_ignores_name() {
        assert_eq!(button("guid", 1, Some("Name A")), button("guid", 1, None));
        assert_ne!(button("guid", 1, None), button("guid", 2, None));
        assert_ne!(button("guid-a", 1, None), button("guid-b", 1, None));
    }

    #[test]
    fn old_transmit_mode_migration_still_works() {
        let toml = r#"
            mode = "RadioIntegration"
            radio_push_to_talk = "F24"
        "#;

        let config: TransmitConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.call_mic_mode, CallMicMode::PushToTalk);
        assert_eq!(config.push_to_talk, Some(InputCode::Key(Code::F24)));
        assert_eq!(config.was_radio_integration, Some(true));
    }

    #[test]
    fn compose_wayland_trigger_button_replaces_portal() {
        let configured = Some(button("guid", 3, None));
        let trigger = compose_wayland_trigger(Some(PortalAction::PushToTalk), &configured);
        assert_eq!(trigger, Some(Trigger::Input(button("guid", 3, None))));

        assert_eq!(
            compose_wayland_trigger(Some(PortalAction::PushToTalk), &None),
            Some(Trigger::Portal(PortalAction::PushToTalk))
        );
    }

    #[test]
    fn compose_wayland_trigger_ignores_keyboard_binding() {
        let configured = Some(InputCode::Key(Code::KeyA));
        let trigger = compose_wayland_trigger(Some(PortalAction::PushToTalk), &configured);
        assert_eq!(trigger, Some(Trigger::Portal(PortalAction::PushToTalk)));

        assert_eq!(
            compose_wayland_trigger(None, &Some(InputCode::Key(Code::KeyA))),
            None
        );
    }

    #[test]
    fn radio_never_falls_back_to_call_without_radio_integration() {
        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            ..Default::default()
        };

        assert!(!config.radio_falls_back_to_call(false));
    }

    #[test]
    fn radio_only_falls_back_to_call_for_wayland_push_to_talk() {
        let ptt = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            ..Default::default()
        };

        // Off Wayland the radio binding is known from configuration alone, so
        // the fallback is folded into active_radio_trigger instead.
        assert_eq!(ptt.radio_falls_back_to_call(true), running_on_wayland());

        for mode in [CallMicMode::PushToMute, CallMicMode::VoiceActivation] {
            let config = TransmitConfig {
                call_mic_mode: mode,
                ..Default::default()
            };
            assert!(
                !config.radio_falls_back_to_call(true),
                "{mode:?} has an unambiguous radio trigger"
            );
        }
    }

    #[test]
    fn configured_joystick_button_suppresses_the_radio_fallback() {
        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            radio_push_to_talk: Some(button("guid", 4, None)),
            ..Default::default()
        };

        // The button replaces the portal shortcut outright, so there is no
        // unbound portal shortcut left to fall back from.
        assert!(!config.radio_falls_back_to_call(true));
        assert_eq!(
            config.active_radio_trigger(true),
            Some(Trigger::Input(button("guid", 4, None)))
        );
    }

    #[test]
    fn wayland_radio_trigger_is_always_the_dedicated_portal_action() {
        if !running_on_wayland() {
            return;
        }

        // Unlike the keyboard bindings, which the portal owns, the trigger no
        // longer depends on whether a key is currently assigned to it: the
        // engine resolves the fallback from the listener at runtime.
        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            ..Default::default()
        };

        assert_eq!(
            config.active_radio_trigger(true),
            Some(Trigger::Portal(PortalAction::RadioPushToTalk))
        );
        assert!(config.radio_falls_back_to_call(true));
    }

    #[test]
    fn active_triggers_in_push_to_mute_mode_follow_ptm_binding() {
        // Only run the non-Wayland branch deterministically; the Wayland
        // composition itself is covered by the compose_wayland_trigger tests.
        if running_on_wayland() {
            return;
        }

        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToMute,
            push_to_mute: Some(button("guid", 1, None)),
            // Deliberately set: a dedicated radio binding is not supported in
            // PTM mode, so radio TX must follow the PTM binding instead.
            radio_push_to_talk: Some(button("guid", 2, None)),
            ..Default::default()
        };

        let expected = Some(Trigger::Input(button("guid", 1, None)));
        assert_eq!(config.active_call_trigger(), expected);
        assert_eq!(config.active_radio_trigger(true), expected);
    }

    #[test]
    fn active_triggers_in_voice_activation_mode() {
        // Only run the non-Wayland branch deterministically; the Wayland
        // composition itself is covered by the compose_wayland_trigger tests.
        if running_on_wayland() {
            return;
        }

        let config = TransmitConfig {
            call_mic_mode: CallMicMode::VoiceActivation,
            // Ignored: no call key exists in voice activation mode
            push_to_talk: Some(button("guid", 1, None)),
            radio_push_to_talk: Some(button("guid", 2, None)),
            ..Default::default()
        };

        assert_eq!(config.active_call_trigger(), None);
        assert_eq!(
            config.active_radio_trigger(true),
            Some(Trigger::Input(button("guid", 2, None)))
        );
    }

    #[test]
    fn active_triggers_use_configured_bindings() {
        // Only run the non-Wayland branch deterministically; the Wayland
        // composition itself is covered by the compose_wayland_trigger tests.
        if running_on_wayland() {
            return;
        }

        let config = TransmitConfig {
            call_mic_mode: CallMicMode::PushToTalk,
            push_to_talk: Some(button("guid", 0, None)),
            ..Default::default()
        };

        assert_eq!(
            config.active_call_trigger(),
            Some(Trigger::Input(button("guid", 0, None)))
        );

        // No dedicated radio binding: radio TX follows the call PTT binding
        let radio = config.active_radio_trigger(true);
        assert_eq!(radio, Some(Trigger::Input(button("guid", 0, None))));

        assert_eq!(config.active_radio_trigger(false), None);
    }
}
