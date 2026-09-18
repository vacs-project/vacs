use serde::Serialize;
use vacs_signaling::protocol::{
    profile::ActiveProfile,
    ws::server::{SessionInfo, SessionProfile},
};

use crate::app::ClientConfig;

pub(crate) mod auth;
pub(crate) mod commands;

/// The `signaling:connected` payload: the server's [`SessionInfo`] flattened,
/// plus the client-side state the frontend needs alongside it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSessionInfo {
    #[serde(flatten)]
    pub session_info: SessionInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_profile_width: Option<u16>,
}

impl ClientSessionInfo {
    /// Looks up the persisted split width for the session's active profile, if it
    /// is a specific profile with a stored entry.
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

#[cfg(test)]
mod tests {
    use super::*;
    use vacs_signaling::protocol::{
        profile::{Profile, ProfileId, ProfileType, ProfileView},
        ws::server::ClientInfo,
    };

    fn profile(id: &str) -> Profile {
        Profile {
            id: ProfileId::from(id),
            view: ProfileView::Split,
            profile_type: ProfileType::Tabbed(vec![]),
        }
    }

    fn session_info(profile: SessionProfile) -> SessionInfo {
        SessionInfo {
            client: ClientInfo {
                id: "1000000".into(),
                display_name: "LOVV_CTR".to_string(),
                frequency: "199.998".to_string(),
                position_id: None,
            },
            profile,
            default_call_sources: vec![],
        }
    }

    fn config_with_width(id: &str, width: u16) -> ClientConfig {
        let mut config = ClientConfig::default();
        config
            .split_profile_widths
            .insert(ProfileId::from(id), width);
        config
    }

    #[test]
    fn width_follows_the_specific_active_profile() {
        let config = config_with_width("LOVV", 320);
        let specific = |id| {
            session_info(SessionProfile::Changed(ActiveProfile::Specific(profile(
                id,
            ))))
        };

        let info = ClientSessionInfo::from_session_info_and_config(specific("LOVV"), &config);
        assert_eq!(info.split_profile_width, Some(320));

        for session_info in [
            specific("EDGG"),
            session_info(SessionProfile::Changed(ActiveProfile::Custom)),
            session_info(SessionProfile::Changed(ActiveProfile::None)),
            session_info(SessionProfile::Unchanged),
        ] {
            let info = ClientSessionInfo::from_session_info_and_config(session_info, &config);
            assert_eq!(info.split_profile_width, None);
        }
    }

    #[test]
    fn serializes_flat_with_the_width_only_when_present() {
        let config = config_with_width("LOVV", 320);
        let with_width = ClientSessionInfo::from_session_info_and_config(
            session_info(SessionProfile::Changed(ActiveProfile::Specific(profile(
                "LOVV",
            )))),
            &config,
        );
        let value = serde_json::to_value(&with_width).unwrap();
        assert_eq!(value["client"]["displayName"], "LOVV_CTR");
        assert_eq!(value["profile"]["type"], "changed");
        assert_eq!(value["defaultCallSources"], serde_json::json!([]));
        assert_eq!(value["splitProfileWidth"], 320);
        assert!(value.get("sessionInfo").is_none());

        let without_width = ClientSessionInfo::from_session_info_and_config(
            session_info(SessionProfile::Unchanged),
            &config,
        );
        let value = serde_json::to_value(&without_width).unwrap();
        assert!(value.get("splitProfileWidth").is_none());
    }
}
