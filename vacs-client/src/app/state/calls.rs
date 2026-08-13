use crate::app::state::webrtc::WebrtcCall;
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;
use vacs_signaling::protocol::vatsim::ClientId;
use vacs_signaling::protocol::ws::server::CallInvitation;
use vacs_signaling::protocol::ws::shared::{CallId, CallParticipants, CallTarget};

pub struct Call {
    call_id: CallId,
    webrtc: WebrtcCall,
    invited_targets: HashSet<CallTarget>,
    joined_participants: CallParticipants,
}

impl Call {
    pub fn new(
        call_id: CallId,
        invited_targets: HashSet<CallTarget>,
        shutdown_token: &CancellationToken,
    ) -> Self {
        Call {
            call_id,
            webrtc: WebrtcCall::new(call_id, shutdown_token),
            invited_targets,
            joined_participants: CallParticipants::default(),
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

    pub fn joined_participants(&self) -> &CallParticipants {
        &self.joined_participants
    }

    pub fn is_active(&self, own_client_id: &ClientId) -> bool {
        self.joined_participants.contains_key(own_client_id)
    }

    pub fn update(
        &mut self,
        own_client_id: &ClientId,
        invited_targets: HashSet<CallTarget>,
        joined_participants: CallParticipants,
    ) -> (CallParticipants, HashSet<ClientId>) {
        self.invited_targets = invited_targets;

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
