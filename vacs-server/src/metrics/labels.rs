use crate::metrics::guards::CallAttemptOutcome;
use crate::release::catalog::BundleType;
use vacs_protocol::http::version::ReleaseChannel;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{DisconnectReason, LoginFailureReason, ServerMessage};
use vacs_protocol::ws::shared::{CallErrorReason, CallSource, CallTarget, ErrorReason};

pub trait AsMetricLabel {
    fn as_metric_label(&self) -> &'static str;
}

impl AsMetricLabel for DisconnectReason {
    fn as_metric_label(&self) -> &'static str {
        match self {
            DisconnectReason::Terminated => "terminated",
            DisconnectReason::NoActiveVatsimConnection => "no_active_vatsim_connection",
            DisconnectReason::AmbiguousVatsimPosition(_) => "ambiguous_vatsim_position",
        }
    }
}

impl AsMetricLabel for Option<DisconnectReason> {
    fn as_metric_label(&self) -> &'static str {
        match self {
            Some(reason) => reason.as_metric_label(),
            None => "graceful",
        }
    }
}

impl AsMetricLabel for LoginFailureReason {
    fn as_metric_label(&self) -> &'static str {
        match self {
            LoginFailureReason::Unauthorized => "unauthorized",
            LoginFailureReason::DuplicateId => "duplicate_id",
            LoginFailureReason::InvalidCredentials => "invalid_credentials",
            LoginFailureReason::NoActiveVatsimConnection => "no_active_vatsim_connection",
            LoginFailureReason::AmbiguousVatsimPosition(_) => "ambiguous_vatsim_position",
            LoginFailureReason::InvalidVatsimPosition => "invalid_vatsim_position",
            LoginFailureReason::Timeout => "timeout",
            LoginFailureReason::IncompatibleProtocolVersion => "incompatible_protocol_version",
        }
    }
}

impl AsMetricLabel for CallAttemptOutcome {
    fn as_metric_label(&self) -> &'static str {
        match self {
            CallAttemptOutcome::Accepted => "accepted",
            CallAttemptOutcome::Rejected => "rejected",
            CallAttemptOutcome::Cancelled => "cancelled",
            CallAttemptOutcome::Aborted => "aborted",
            CallAttemptOutcome::Error(CallErrorReason::AudioFailure) => "error_audio_failure",
            CallAttemptOutcome::Error(CallErrorReason::AutoHangup) => "error_auto_hangup",
            CallAttemptOutcome::Error(CallErrorReason::WebrtcFailure) => "error_webrtc_failure",
            CallAttemptOutcome::Error(CallErrorReason::CallActive) => "error_call_active",
            CallAttemptOutcome::Error(CallErrorReason::CallFailure) => "error_call_failure",
            CallAttemptOutcome::Error(CallErrorReason::SignalingFailure) => {
                "error_signaling_failure"
            }
            CallAttemptOutcome::Error(CallErrorReason::TargetNotFound) => "error_target_not_found",
            CallAttemptOutcome::Error(CallErrorReason::Other) => "error_other",
        }
    }
}

impl AsMetricLabel for Option<CallAttemptOutcome> {
    fn as_metric_label(&self) -> &'static str {
        match self {
            Some(outcome) => outcome.as_metric_label(),
            None => "aborted",
        }
    }
}

impl AsMetricLabel for ReleaseChannel {
    fn as_metric_label(&self) -> &'static str {
        self.as_str()
    }
}

impl AsMetricLabel for BundleType {
    fn as_metric_label(&self) -> &'static str {
        self.as_str()
    }
}

impl AsMetricLabel for ClientMessage {
    fn as_metric_label(&self) -> &'static str {
        match self {
            ClientMessage::Login(_) => "login",
            ClientMessage::Logout => "logout",
            ClientMessage::CallInvite(_) => "call_invite",
            ClientMessage::CallAccept(_) => "call_accept",
            ClientMessage::CallReject(_) => "call_reject",
            ClientMessage::CallEnd(_) => "call_end",
            ClientMessage::CallError(_) => "call_error",
            ClientMessage::WebrtcOffer(_) => "webrtc_offer",
            ClientMessage::WebrtcAnswer(_) => "webrtc_answer",
            ClientMessage::WebrtcIceCandidate(_) => "webrtc_ice_candidate",
            ClientMessage::ListClients => "list_clients",
            ClientMessage::ListStations => "list_stations",
            ClientMessage::Disconnect => "disconnect",
            ClientMessage::Error(_) => "error",
        }
    }
}

impl AsMetricLabel for ServerMessage {
    fn as_metric_label(&self) -> &'static str {
        match self {
            ServerMessage::LoginFailure(_) => "login_failure",
            ServerMessage::CallInvite(_) => "call_invite",
            ServerMessage::CallAccept(_) => "call_accept",
            ServerMessage::CallEnd(_) => "call_end",
            ServerMessage::CallCancelled(_) => "call_cancelled",
            ServerMessage::CallError(_) => "call_error",
            ServerMessage::WebrtcOffer(_) => "webrtc_offer",
            ServerMessage::WebrtcAnswer(_) => "webrtc_answer",
            ServerMessage::WebrtcIceCandidate(_) => "webrtc_ice_candidate",
            ServerMessage::ClientInfo(_) => "client_info",
            ServerMessage::SessionInfo(_) => "session_info",
            ServerMessage::ClientConnected(_) => "client_connected",
            ServerMessage::ClientDisconnected(_) => "client_disconnected",
            ServerMessage::ClientList(_) => "client_list",
            ServerMessage::StationList(_) => "station_list",
            ServerMessage::StationChanges(_) => "station_changes",
            ServerMessage::Disconnected(_) => "disconnected",
            ServerMessage::Error(_) => "error",
        }
    }
}

impl AsMetricLabel for ErrorReason {
    fn as_metric_label(&self) -> &'static str {
        match self {
            ErrorReason::MalformedMessage => "malformed_message",
            ErrorReason::Internal(_) => "internal",
            ErrorReason::PeerConnection => "peer_connection",
            ErrorReason::UnexpectedMessage(_) => "unexpected_message",
            ErrorReason::RateLimited { .. } => "rate_limited",
            ErrorReason::ClientNotFound => "client_not_found",
        }
    }
}

impl AsMetricLabel for CallErrorReason {
    fn as_metric_label(&self) -> &'static str {
        match self {
            CallErrorReason::TargetNotFound => "target_not_found",
            CallErrorReason::CallActive => "call_active",
            CallErrorReason::WebrtcFailure => "webrtc_failure",
            CallErrorReason::AudioFailure => "audio_failure",
            CallErrorReason::CallFailure => "call_failure",
            CallErrorReason::SignalingFailure => "signaling_failure",
            CallErrorReason::AutoHangup => "auto_hangup",
            CallErrorReason::Other => "other",
        }
    }
}

impl AsMetricLabel for CallTarget {
    fn as_metric_label(&self) -> &'static str {
        match self {
            CallTarget::Client(_) => "client",
            CallTarget::Position(_) => "position",
            CallTarget::Station(_) => "station",
        }
    }
}

impl AsMetricLabel for CallSource {
    fn as_metric_label(&self) -> &'static str {
        match (&self.station_id, &self.position_id) {
            (Some(_), _) => "station",
            (None, Some(_)) => "position",
            (None, None) => "client",
        }
    }
}
