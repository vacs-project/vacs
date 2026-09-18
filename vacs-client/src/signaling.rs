use serde::Serialize;
use vacs_signaling::protocol::{
    profile::ActiveProfile,
    ws::server::{SessionInfo, SessionProfile},
};

use crate::app::ClientConfig;

pub(crate) mod auth;
pub(crate) mod commands;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSessionInfo {
    #[serde(flatten)]
    pub session_info: SessionInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_profile_width: Option<u16>,
}

impl ClientSessionInfo {
    pub fn from_session_info_and_config(
        session_info: SessionInfo,
        client_config: &ClientConfig,
    ) -> Self {
        let split_profile_width = match &session_info.profile {
            SessionProfile::Changed(ActiveProfile::Specific(profile)) => {
                client_config.split_profile_widths.get(&profile.id).copied()
            }
            _ => None,
        };

        Self {
            session_info,
            split_profile_width,
        }
    }
}
