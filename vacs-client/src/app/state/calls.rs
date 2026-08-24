use crate::app::state::webrtc::WebrtcCall;
use std::collections::{HashMap, HashSet};
use tokio_util::sync::CancellationToken;
use vacs_signaling::protocol::vatsim::ClientId;
use vacs_signaling::protocol::ws::client::CallInvite;
use vacs_signaling::protocol::ws::server::{CallInvitation, CallUpdate};
use vacs_signaling::protocol::ws::shared::{CallId, CallParticipants, CallTarget};

pub struct Call {
    call_id: CallId,
    webrtc: WebrtcCall,
    invited_targets: HashSet<CallTarget>,
    joined_participants: CallParticipants,
    conference_leader: Option<ClientId>,
}

impl Call {
    pub fn from_invite(invite: &CallInvite, shutdown_token: &CancellationToken) -> Self {
        Self {
            call_id: invite.call_id,
            webrtc: WebrtcCall::new(invite.call_id, shutdown_token),
            invited_targets: invite.targets.clone(),
            joined_participants: HashMap::new(),
            conference_leader: None,
        }
    }

    pub fn from_invitation(
        invitation: &CallInvitation,
        shutdown_token: &CancellationToken,
    ) -> Self {
        Self {
            call_id: invitation.call_id,
            webrtc: WebrtcCall::new(invitation.call_id, shutdown_token),
            invited_targets: invitation.invited_targets.clone(),
            joined_participants: invitation.joined_participants.clone(),
            conference_leader: invitation.conference_leader.clone(),
        }
    }

    pub fn call_id(&self) -> CallId {
        self.call_id
    }

    pub fn webrtc(&self) -> &WebrtcCall {
        &self.webrtc
    }
    pub fn webrtc_mut(&mut self) -> &mut WebrtcCall {
        &mut self.webrtc
    }

    pub fn invited_targets(&self) -> &HashSet<CallTarget> {
        &self.invited_targets
    }

    pub fn add_invited_targets(&mut self, targets: HashSet<CallTarget>) {
        self.invited_targets.extend(targets);
    }

    pub fn remove_invited_targets(&mut self, targets: &HashSet<CallTarget>) {
        self.invited_targets
            .retain(|target| !targets.contains(target));
    }

    pub fn joined_participants(&self) -> &CallParticipants {
        &self.joined_participants
    }

    pub fn is_active(&self, own_client_id: &ClientId) -> bool {
        self.joined_participants.contains_key(own_client_id)
    }

    pub fn is_empty(&self) -> bool {
        self.joined_participants.is_empty() && self.invited_targets.is_empty()
    }

    pub fn update(
        &mut self,
        own_client_id: &ClientId,
        invited_targets: HashSet<CallTarget>,
        joined_participants: CallParticipants,
        conference_leader: Option<ClientId>,
    ) -> (CallParticipants, HashSet<ClientId>) {
        self.invited_targets = invited_targets;
        self.conference_leader = conference_leader;

        let (added, removed) = if joined_participants.contains_key(own_client_id) {
            if self.is_active(own_client_id) {
                (
                    joined_participants
                        .iter()
                        .filter(|(id, _)| !self.joined_participants.contains_key(*id))
                        .map(|(id, target)| (id.clone(), target.clone()))
                        .collect(),
                    self.joined_participants
                        .keys()
                        .filter(|id| !joined_participants.contains_key(*id))
                        .cloned()
                        .collect(),
                )
            } else {
                (
                    joined_participants
                        .iter()
                        .filter(|(id, _)| *id != own_client_id)
                        .map(|(id, target)| (id.clone(), target.clone()))
                        .collect(),
                    HashSet::new(),
                )
            }
        } else {
            (CallParticipants::new(), HashSet::new())
        };

        self.joined_participants = joined_participants;

        (added, removed)
    }

    pub fn into_webrtc(self) -> WebrtcCall {
        self.webrtc
    }
}

impl From<&Call> for CallUpdate {
    fn from(call: &Call) -> Self {
        CallUpdate {
            call_id: call.call_id,
            invited_targets: call.invited_targets.clone(),
            joined_participants: call.joined_participants.clone(),
            conference_leader: call.conference_leader.clone(),
        }
    }
}

impl From<&mut Call> for CallUpdate {
    fn from(call: &mut Call) -> Self {
        CallUpdate {
            call_id: call.call_id,
            invited_targets: call.invited_targets.clone(),
            joined_participants: call.joined_participants.clone(),
            conference_leader: call.conference_leader.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vacs_signaling::protocol::ws::shared::CallSource;

    fn client(id: &str) -> ClientId {
        ClientId::from(id)
    }

    fn target(id: &str) -> CallTarget {
        CallTarget::Client(client(id))
    }

    fn participants(ids: &[&str]) -> CallParticipants {
        ids.iter().map(|id| (client(id), target(id))).collect()
    }

    fn call(caller: &str, targets: &[&str]) -> Call {
        let invite = CallInvite {
            call_id: CallId::new(),
            source: CallSource {
                client_id: client(caller),
                position_id: None,
                station_id: None,
            },
            targets: targets.iter().map(|id| target(id)).collect(),
            prio: false,
        };
        Call::from_invite(&invite, &CancellationToken::new())
    }

    #[test]
    fn update_when_joining_reports_all_other_participants() {
        let mut call = call("a", &["b", "c"]);
        assert!(!call.is_active(&client("a")));

        let (added, removed) = call.update(
            &client("a"),
            HashSet::from([target("c")]),
            participants(&["a", "b"]),
            None,
        );

        assert_eq!(added, participants(&["b"]));
        assert!(removed.is_empty());
        assert!(call.is_active(&client("a")));
    }

    #[test]
    fn update_while_active_reports_joined_and_left_participants() {
        let mut call = call("a", &["b", "c"]);
        call.update(
            &client("a"),
            HashSet::from([target("c")]),
            participants(&["a", "b"]),
            None,
        );

        let (added, removed) = call.update(
            &client("a"),
            HashSet::new(),
            participants(&["a", "c"]),
            None,
        );

        assert_eq!(added, participants(&["c"]));
        assert_eq!(removed, HashSet::from([client("b")]));
        assert_eq!(call.joined_participants(), &participants(&["a", "c"]));
    }

    #[test]
    fn update_without_self_reports_no_deltas() {
        let mut call = call("a", &["b", "c"]);

        // Not yet joined
        let (added, removed) = call.update(
            &client("a"),
            HashSet::from([target("c")]),
            participants(&["b"]),
            None,
        );
        assert!(added.is_empty());
        assert!(removed.is_empty());
        assert!(!call.is_active(&client("a")));

        // Joined, then dropped from the call
        call.update(
            &client("a"),
            HashSet::new(),
            participants(&["a", "b"]),
            None,
        );
        let (added, removed) =
            call.update(&client("a"), HashSet::new(), participants(&["b"]), None);
        assert!(added.is_empty());
        assert!(removed.is_empty());
        assert!(!call.is_active(&client("a")));
    }

    #[test]
    fn update_replaces_invited_targets_and_participants() {
        let mut call = call("a", &["b", "c"]);
        assert_eq!(
            call.invited_targets(),
            &HashSet::from([target("b"), target("c")])
        );

        call.update(
            &client("a"),
            HashSet::from([target("c")]),
            participants(&["b"]),
            None,
        );

        assert_eq!(call.invited_targets(), &HashSet::from([target("c")]));
        assert_eq!(call.joined_participants(), &participants(&["b"]));
        assert!(!call.is_empty());

        call.update(&client("a"), HashSet::new(), CallParticipants::new(), None);
        assert!(call.is_empty());
    }
}
