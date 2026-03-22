use axum::extract::ws::{self, Utf8Bytes};
use serde::{Deserialize, Serialize};
use std::fmt;

/// [RFC 7807](https://datatracker.ietf.org/doc/html/rfc7807)-compatible problem details for error responses.
///
/// Standard fields: `type`, `title`, `detail`.
/// Extension fields: `is_non_critical`, `timeout_ms` (for frontend overlay behaviour).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetails {
    /// URI reference identifying the problem type.
    #[serde(rename = "type")]
    pub problem_type: ProblemType,
    /// Short human-readable summary.
    pub title: String,
    /// Longer human-readable explanation.
    pub detail: String,
    /// `true` for expected/recoverable errors; `false` for unexpected failures.
    pub is_non_critical: bool,
    /// Optional auto-dismiss timeout in milliseconds for the frontend overlay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u16>,
}

/// Well-known problem type URIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemType {
    /// The command is only available on the desktop application.
    DesktopOnly,
    /// One or more command arguments were invalid or missing.
    InvalidArgument,
    /// The command did not complete within the time limit.
    Timeout,
    /// An application-level error originating from the backend.
    ApplicationError,
}

impl ProblemType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DesktopOnly => "urn:vacs:error:remote:desktop-only",
            Self::InvalidArgument => "urn:vacs:error:remote:invalid-argument",
            Self::Timeout => "urn:vacs:error:remote:timeout",
            Self::ApplicationError => "urn:vacs:error:remote:application",
        }
    }
}

impl fmt::Display for ProblemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ProblemType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl ProblemDetails {
    pub fn new(
        problem_type: ProblemType,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            problem_type,
            title: title.into(),
            detail: detail.into(),
            is_non_critical: false,
            timeout_ms: None,
        }
    }

    pub fn non_critical(mut self) -> Self {
        self.is_non_critical = true;
        self
    }

    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout_ms: u16) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn desktop_only() -> Self {
        Self::new(
            ProblemType::DesktopOnly,
            "Desktop only",
            "This operation is only available on the desktop application",
        )
        .non_critical()
    }

    pub fn invalid_argument(key: &str, reason: impl fmt::Display) -> Self {
        Self::new(
            ProblemType::InvalidArgument,
            "Invalid argument",
            format!("Failed to parse argument '{key}': {reason}"),
        )
        .non_critical()
    }

    pub fn timeout() -> Self {
        Self::new(
            ProblemType::Timeout,
            "Timeout",
            "The command did not complete within the time limit",
        )
        .non_critical()
    }
}

impl From<&crate::error::Error> for ProblemDetails {
    fn from(err: &crate::error::Error) -> Self {
        let fe = crate::error::FrontendError::from(err);
        Self {
            problem_type: ProblemType::ApplicationError,
            title: fe.title,
            detail: fe.detail,
            is_non_critical: fe.is_non_critical,
            timeout_ms: fe.timeout_ms,
        }
    }
}

/// Messages sent from the remote (browser) client to the desktop server.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Invoke a Tauri command.
    Invoke {
        /// Opaque ID to correlate the response with.
        id: String,
        /// The command to invoke.
        cmd: RemoteCommand,
        /// Arguments for the command.
        args: serde_json::Value,
    },
    /// Subscribe to a Tauri event by name.
    Subscribe { event: RemoteEvent },
    /// Unsubscribe from a previously subscribed event.
    Unsubscribe { event: RemoteEvent },
    /// Keepalive ping. The server replies with a `Pong`.
    Ping,
}

/// All commands that can be invoked over the remote WebSocket protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCommand {
    AppFrontendReady,
    AppOpenFolder,
    AppCheckForUpdate,
    AppQuit,
    AppUpdate,
    AppPlatformCapabilities,
    AppSetAlwaysOnTop,
    AppSetFullscreen,
    AppResetWindowSize,
    AppGetCallConfig,
    AppSetCallConfig,
    AppLoadTestProfile,
    AppUnloadTestProfile,
    AppGetClientPageSettings,
    AppSetSelectedClientPageConfig,
    AppLoadExtraClientPageConfig,
    AppGetVersion,

    AudioGetHosts,
    AudioSetHost,
    AudioGetDevices,
    AudioSetDevice,
    AudioGetVolumes,
    AudioSetVolume,
    AudioPlayUiClick,
    AudioStartInputLevelMeter,
    AudioStopInputLevelMeter,
    AudioSetRadioPrio,

    AuthOpenOauthUrl,
    AuthCheckSession,
    AuthLogout,

    KeybindsGetTransmitConfig,
    KeybindsSetTransmitConfig,
    KeybindsGetKeybindsConfig,
    KeybindsSetBinding,
    KeybindsGetRadioConfig,
    KeybindsSetRadioConfig,
    KeybindsGetRadioState,
    KeybindsGetExternalBinding,
    KeybindsOpenSystemShortcutsSettings,
    KeybindsReconnectRadio,

    RemoteBroadcastStoreSync,
    RemoteGetConfig,
    RemoteGetSessionState,
    RemoteRequestStoreSync,

    SignalingConnect,
    SignalingDisconnect,
    SignalingTerminate,
    SignalingStartCall,
    SignalingAcceptCall,
    SignalingEndCall,
    SignalingGetIgnoredClients,
    SignalingAddIgnoredClient,
    SignalingRemoveIgnoredClient,
}

impl RemoteCommand {
    pub const fn is_desktop_only(self) -> bool {
        matches!(
            self,
            Self::AppOpenFolder
                | Self::AppQuit
                | Self::AppUpdate
                | Self::AppSetAlwaysOnTop
                | Self::AppSetFullscreen
                | Self::AppResetWindowSize
                | Self::AppLoadExtraClientPageConfig
                | Self::AuthOpenOauthUrl
                | Self::KeybindsOpenSystemShortcutsSettings
        )
    }
}

/// All Tauri events that can be subscribed to over the remote WebSocket protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemoteEvent {
    AudioImplicitRadioPrio,
    AudioInputLevel,
    AudioRadioPrio,
    AudioStopInputLevelMeter,
    AuthAuthenticated,
    AuthError,
    AuthUnauthenticated,
    Error,
    RadioState,
    SignalingAcceptIncomingCall,
    SignalingAddIncomingToCallList,
    SignalingAmbiguousPosition,
    SignalingCallEnd,
    SignalingCallInvite,
    SignalingCallReject,
    SignalingClientConnected,
    SignalingClientDisconnected,
    SignalingClientList,
    SignalingClientNotFound,
    SignalingClientPageConfig,
    SignalingConnected,
    SignalingDisconnected,
    SignalingForceCallEnd,
    SignalingOutgoingCallAccepted,
    SignalingReconnecting,
    SignalingStationChanges,
    SignalingStationList,
    SignalingTestProfile,
    SignalingUpdateCallList,
    StoreSyncRequest,
    UpdateProgress,
    WebrtcCallConnected,
    WebrtcCallDisconnected,
    StoreSync,
    WebrtcCallError,
}

impl RemoteEvent {
    pub const ALL: &[RemoteEvent] = &[
        Self::AudioImplicitRadioPrio,
        Self::AudioInputLevel,
        Self::AudioRadioPrio,
        Self::AudioStopInputLevelMeter,
        Self::AuthAuthenticated,
        Self::AuthError,
        Self::AuthUnauthenticated,
        Self::Error,
        Self::RadioState,
        Self::SignalingAcceptIncomingCall,
        Self::SignalingAddIncomingToCallList,
        Self::SignalingAmbiguousPosition,
        Self::SignalingCallEnd,
        Self::SignalingCallInvite,
        Self::SignalingCallReject,
        Self::SignalingClientConnected,
        Self::SignalingClientDisconnected,
        Self::SignalingClientList,
        Self::SignalingClientNotFound,
        Self::SignalingClientPageConfig,
        Self::SignalingConnected,
        Self::SignalingDisconnected,
        Self::SignalingForceCallEnd,
        Self::SignalingOutgoingCallAccepted,
        Self::SignalingReconnecting,
        Self::SignalingStationChanges,
        Self::SignalingStationList,
        Self::SignalingTestProfile,
        Self::SignalingUpdateCallList,
        Self::StoreSync,
        Self::StoreSyncRequest,
        Self::UpdateProgress,
        Self::WebrtcCallConnected,
        Self::WebrtcCallDisconnected,
        Self::WebrtcCallError,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AudioImplicitRadioPrio => "audio:implicit-radio-prio",
            Self::AudioInputLevel => "audio:input-level",
            Self::AudioRadioPrio => "audio:radio-prio",
            Self::AudioStopInputLevelMeter => "audio:stop-input-level-meter",
            Self::AuthAuthenticated => "auth:authenticated",
            Self::AuthError => "auth:error",
            Self::AuthUnauthenticated => "auth:unauthenticated",
            Self::Error => "error",
            Self::RadioState => "radio:state",
            Self::SignalingAcceptIncomingCall => "signaling:accept-incoming-call",
            Self::SignalingAddIncomingToCallList => "signaling:add-incoming-to-call-list",
            Self::SignalingAmbiguousPosition => "signaling:ambiguous-position",
            Self::SignalingCallEnd => "signaling:call-end",
            Self::SignalingCallInvite => "signaling:call-invite",
            Self::SignalingCallReject => "signaling:call-reject",
            Self::SignalingClientConnected => "signaling:client-connected",
            Self::SignalingClientDisconnected => "signaling:client-disconnected",
            Self::SignalingClientList => "signaling:client-list",
            Self::SignalingClientNotFound => "signaling:client-not-found",
            Self::SignalingClientPageConfig => "signaling:client-page-config",
            Self::SignalingConnected => "signaling:connected",
            Self::SignalingDisconnected => "signaling:disconnected",
            Self::SignalingForceCallEnd => "signaling:force-call-end",
            Self::SignalingOutgoingCallAccepted => "signaling:outgoing-call-accepted",
            Self::SignalingReconnecting => "signaling:reconnecting",
            Self::SignalingStationChanges => "signaling:station-changes",
            Self::SignalingStationList => "signaling:station-list",
            Self::SignalingTestProfile => "signaling:test-profile",
            Self::SignalingUpdateCallList => "signaling:update-call-list",
            Self::StoreSync => "store:sync",
            Self::StoreSyncRequest => "store:sync:request",
            Self::UpdateProgress => "update:progress",
            Self::WebrtcCallConnected => "webrtc:call-connected",
            Self::WebrtcCallDisconnected => "webrtc:call-disconnected",
            Self::WebrtcCallError => "webrtc:call-error",
        }
    }
}

impl std::str::FromStr for RemoteEvent {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .find(|e| e.as_str() == s)
            .copied()
            .ok_or(())
    }
}

impl std::fmt::Display for RemoteEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for RemoteEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RemoteEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s: &str = Deserialize::deserialize(deserializer)?;
        s.parse()
            .map_err(|_| serde::de::Error::unknown_variant(s, &[]))
    }
}

/// Messages sent from the desktop server to the remote (browser) client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Response to an `Invoke` message.
    Response {
        /// Opaque ID to correlate the request with.
        id: String,
        /// Whether the command succeeded or failed.
        ok: bool,
        /// Optional data returned by the command (if `ok` is `true`).
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        /// Optional error information returned by the command (if `ok` is `false`).
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ProblemDetails>,
    },
    /// A Tauri event forwarded to the remote client.
    Event {
        /// The name of the event.
        name: RemoteEvent,
        /// The event payload.
        payload: serde_json::Value,
    },
    /// Keepalive pong in response to a `Ping`.
    Pong,
    /// WebSocket protocol-level pong (not serialized as JSON).
    #[serde(skip)]
    WsPong(Vec<u8>),
}

impl ServerMessage {
    pub fn ok(id: String, data: serde_json::Value) -> Self {
        Self::Response {
            id,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(id: String, error: ProblemDetails) -> Self {
        Self::Response {
            id,
            ok: false,
            data: None,
            error: Some(error),
        }
    }

    pub fn serialize(self) -> Result<ws::Message, serde_json::Error> {
        match self {
            Self::WsPong(data) => Ok(ws::Message::Pong(data.into())),
            other => serde_json::to_string(&other)
                .map(Utf8Bytes::from)
                .map(ws::Message::Text),
        }
    }
}
