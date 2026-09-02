use std::collections::HashSet;
use std::time::Duration;
use test_log::test;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::ServerMessage;
use vacs_protocol::ws::shared::{CallId, CallTarget};
use vacs_server::test_utils::{TestApp, TestClient, setup_n_test_clients};

#[test(tokio::test)]
async fn call_offer() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 5).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;

    let invite_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client2 should receive CallInvite"
    );

    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;

    let accept_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && update.joined_participants.contains_key(client2.id()))
        })
        .await;
    assert_eq!(
        accept_messages.len(),
        1,
        "client1 should receive a call update with client2 joined"
    );

    client1
        .send(ClientMessage::WebrtcOffer(
            vacs_protocol::ws::shared::WebrtcOffer {
                call_id,
                from_client_id: client1.id().clone(),
                to_client_id: client2.id().clone(),
                sdp: "sdp1".to_string(),
            },
        ))
        .await?;

    let call_offer_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::WebrtcOffer(_))
        })
        .await;

    assert_eq!(
        call_offer_messages.len(),
        1,
        "client2 should have received exactly one WebrtcOffer message"
    );

    match &call_offer_messages[0] {
        ServerMessage::WebrtcOffer(offer) => {
            assert_eq!(
                &offer.from_client_id,
                client1.id(),
                "WebrtcOffer targeted the wrong client"
            );
            assert_eq!(offer.sdp, "sdp1", "WebrtcOffer contains the wrong SDP");
        }
        message => panic!(
            "Unexpected message: {:?}, expected WebrtcOffer from client1",
            message
        ),
    };

    for (i, client) in clients.iter_mut().enumerate() {
        let call_offer_messages = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::WebrtcOffer(_))
            })
            .await;

        assert!(
            call_offer_messages.is_empty(),
            "client{} should have received no messages, but received: {:?}",
            i + 3,
            call_offer_messages
        );
    }

    let call_offer_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::WebrtcOffer(_))
        })
        .await;
    assert!(
        call_offer_messages.is_empty(),
        "client1 should have received no messages, but received: {:?}",
        call_offer_messages
    );

    Ok(())
}

#[test(tokio::test)]
async fn call_offer_answer() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 5).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    let call_id = CallId::new();
    // Setup call first
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let _ = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && update.joined_participants.contains_key(client2.id()))
        })
        .await;

    client1
        .send(ClientMessage::WebrtcOffer(
            vacs_protocol::ws::shared::WebrtcOffer {
                call_id,
                from_client_id: client1.id().clone(),
                to_client_id: client2.id().clone(),
                sdp: "sdp1".to_string(),
            },
        ))
        .await?;

    // Consume offer on client2
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::WebrtcOffer(_))
        })
        .await;

    client2
        .send(ClientMessage::WebrtcAnswer(
            vacs_protocol::ws::shared::WebrtcAnswer {
                call_id,
                from_client_id: client2.id().clone(),
                to_client_id: client1.id().clone(),
                sdp: "sdp2".to_string(),
            },
        ))
        .await?;

    let call_answer_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::WebrtcAnswer(_))
        })
        .await;

    assert_eq!(
        call_answer_messages.len(),
        1,
        "client1 should have received exactly one CallAnswer message"
    );

    match &call_answer_messages[0] {
        ServerMessage::WebrtcAnswer(answer) => {
            assert_eq!(
                &answer.from_client_id,
                client2.id(),
                "CallAnswer targeted the wrong client"
            );
            assert_eq!(answer.sdp, "sdp2", "CallAnswer contains the wrong SDP");
        }
        message => panic!(
            "Unexpected message: {:?}, expected CallAnswer from client2",
            message
        ),
    };

    for (i, client) in clients.iter_mut().enumerate() {
        let messages = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(
                    m,
                    ServerMessage::WebrtcOffer(_) | ServerMessage::WebrtcAnswer(_)
                )
            })
            .await;

        assert!(
            messages.is_empty(),
            "client{} should have received no messages, but received: {:?}",
            i + 3,
            messages
        );
    }

    let call_offer_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::WebrtcOffer(_))
        })
        .await;
    assert!(
        call_offer_messages.is_empty(),
        "client1 should have received no messages, but received: {:?}",
        call_offer_messages
    );

    let call_answer_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::WebrtcAnswer(_))
        })
        .await;
    assert!(
        call_answer_messages.is_empty(),
        "client2 should have received no messages, but received: {:?}",
        call_answer_messages
    );

    Ok(())
}

#[test(tokio::test)]
async fn invite_after_call_end() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let _ = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && update.joined_participants.contains_key(client2.id()))
        })
        .await;

    // client1 ends the active call; client2 must be notified
    client1
        .send(ClientMessage::CallEnd(vacs_protocol::ws::shared::CallEnd {
            call_id,
            ending_client_id: client1.id().clone(),
        }))
        .await?;
    let call_end_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallEnd(_))
        })
        .await;
    assert_eq!(
        call_end_messages.len(),
        1,
        "client2 should receive CallEnd after client1 ended the call"
    );

    // The ending client must be able to place a new call afterwards
    let new_call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: new_call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client3.id().clone())]),
                prio: false,
            },
        ))
        .await?;

    let error_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(_))
        })
        .await;
    assert!(
        error_messages.is_empty(),
        "client1 should not be considered busy after ending its call, but received: {:?}",
        error_messages
    );

    let invite_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client3 should receive CallInvite for the new call"
    );

    Ok(())
}

#[test(tokio::test)]
async fn call_end_from_non_participant() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    // Both participants receive the update of the acceptance, including client2 itself
    let client2_id = client2.id().clone();
    for client in [&mut client1, &mut client2] {
        let _ = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && update.joined_participants.contains_key(&client2_id))
            })
            .await;
    }

    // client3 is not a participant and must not be able to affect the call
    client3
        .send(ClientMessage::CallEnd(vacs_protocol::ws::shared::CallEnd {
            call_id,
            ending_client_id: client3.id().clone(),
        }))
        .await?;

    let error_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(_))
        })
        .await;
    assert_eq!(
        error_messages.len(),
        1,
        "client3 should receive CallError for ending a call it does not participate in"
    );
    match &error_messages[0] {
        ServerMessage::CallError(error) => {
            assert_eq!(
                error.reason,
                vacs_protocol::ws::shared::CallErrorReason::CallNotFound,
                "CallErrorReason mismatch"
            );
        }
        message => panic!("Unexpected message: {:?}, expected CallError", message),
    };

    // The active call must be untouched: no end or update leaks to the participants
    for (name, client) in [("client1", &mut client1), ("client2", &mut client2)] {
        let messages = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(
                    m,
                    ServerMessage::CallEnd(_)
                        | ServerMessage::CallUpdate(_)
                        | ServerMessage::CallError(_)
                )
            })
            .await;
        assert!(
            messages.is_empty(),
            "{} should have received no messages, but received: {:?}",
            name,
            messages
        );
    }

    Ok(())
}

#[test(tokio::test)]
async fn call_end_by_callee_cancels_pending_invitations() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    // client1 invites client2 and client3
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([
                    CallTarget::Client(client2.id().clone()),
                    CallTarget::Client(client3.id().clone()),
                ]),
                prio: false,
            },
        ))
        .await?;
    for client in [&mut client2, &mut client3] {
        let invitations = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallInvitation(_))
            })
            .await;
        assert_eq!(invitations.len(), 1, "callee should receive CallInvitation");
    }

    // client2 accepts, client3 keeps ringing
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let client2_id = client2.id().clone();
    for client in [&mut client1, &mut client2] {
        let _ = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && update.joined_participants.contains_key(&client2_id))
            })
            .await;
    }

    // client2 (not the caller) ends the whole call
    client2
        .send(ClientMessage::CallEnd(vacs_protocol::ws::shared::CallEnd {
            call_id,
            ending_client_id: client2.id().clone(),
        }))
        .await?;

    let end_messages = client1
        .recv_until_timeout_with_filter(
            Duration::from_millis(100),
            |m| matches!(m, ServerMessage::CallEnd(end) if end.call_id == call_id),
        )
        .await;
    assert_eq!(end_messages.len(), 1, "client1 should receive CallEnd");

    // The still ringing invitation must be cancelled with the call
    let cancelled_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled) if cancelled.call_id == call_id)
        })
        .await;
    assert_eq!(
        cancelled_messages.len(),
        1,
        "client3 should receive CallCancelled for its pending invitation"
    );

    // Accepting the ended call must fail instead of resurrecting it
    client3
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client3.id().clone(),
            },
        ))
        .await?;
    let error_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(error)
                if error.call_id == call_id
                    && error.reason == vacs_protocol::ws::shared::CallErrorReason::CallFailure)
        })
        .await;
    assert_eq!(
        error_messages.len(),
        1,
        "client3 should receive CallError when accepting the ended call"
    );

    Ok(())
}

#[test(tokio::test)]
async fn callee_disconnect_cancels_pending_invitations() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    // client1 invites client2 and client3
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([
                    CallTarget::Client(client2.id().clone()),
                    CallTarget::Client(client3.id().clone()),
                ]),
                prio: false,
            },
        ))
        .await?;
    for client in [&mut client2, &mut client3] {
        let invitations = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallInvitation(_))
            })
            .await;
        assert_eq!(invitations.len(), 1, "callee should receive CallInvitation");
    }

    // client2 accepts, client3 keeps ringing
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let client2_id = client2.id().clone();
    for client in [&mut client1, &mut client2] {
        let _ = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && update.joined_participants.contains_key(&client2_id))
            })
            .await;
    }

    // client2 (not the caller) disconnects, fully ending the 1:1 call
    client2.close().await;

    let end_messages = client1
        .recv_until_timeout_with_filter(
            Duration::from_millis(500),
            |m| matches!(m, ServerMessage::CallEnd(end) if end.call_id == call_id && end.ending_client_id == client2_id),
        )
        .await;
    assert_eq!(
        end_messages.len(),
        1,
        "client1 should receive CallEnd attributed to the disconnecting client"
    );

    // The still ringing invitation must be cancelled with the call
    let cancelled_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(500), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled) if cancelled.call_id == call_id)
        })
        .await;
    assert_eq!(
        cancelled_messages.len(),
        1,
        "client3 should receive CallCancelled for its pending invitation"
    );

    Ok(())
}

#[test(tokio::test)]
async fn ringing_conference_target_disconnect_notifies_the_inviter() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    // client1 invites client2 and client3
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([
                    CallTarget::Client(client2.id().clone()),
                    CallTarget::Client(client3.id().clone()),
                ]),
                prio: false,
            },
        ))
        .await?;
    for client in [&mut client2, &mut client3] {
        let invitations = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallInvitation(_))
            })
            .await;
        assert_eq!(invitations.len(), 1, "callee should receive CallInvitation");
    }

    // client2 accepts, making the call active while client3 keeps ringing
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let client2_id = client2.id().clone();
    for client in [&mut client1, &mut client2] {
        let _ = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && update.joined_participants.contains_key(&client2_id))
            })
            .await;
    }

    // client3 disconnects while still ringing
    let client3_target = CallTarget::Client(client3.id().clone());
    client3.close().await;

    // The inviter gets explicit feedback about the lost target plus the shrunk roster
    let messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(500), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled) if cancelled.call_id == call_id)
                || matches!(m, ServerMessage::CallUpdate(update) if update.call_id == call_id)
        })
        .await;
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, ServerMessage::CallCancelled(cancelled)
            if cancelled.reason == vacs_protocol::ws::server::CallCancelReason::Disconnected
                && cancelled.targets.contains(&client3_target))),
        "client1 should receive CallCancelled for the disconnected ringing target"
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, ServerMessage::CallUpdate(update)
            if update.invited_targets.is_empty())),
        "client1 should receive a CallUpdate without the lost target"
    );

    // The joined callee sees the shrunk roster
    let updates = client2
        .recv_until_timeout_with_filter(Duration::from_millis(500), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id && update.invited_targets.is_empty())
        })
        .await;
    assert_eq!(
        updates.len(),
        1,
        "the joined callee should receive a CallUpdate without the lost target"
    );

    Ok(())
}

#[test(tokio::test)]
async fn unanswered_target_disconnect_updates_remaining_ringing_targets() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    // client1 invites client2 and client3, nobody accepts
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([
                    CallTarget::Client(client2.id().clone()),
                    CallTarget::Client(client3.id().clone()),
                ]),
                prio: false,
            },
        ))
        .await?;
    for client in [&mut client2, &mut client3] {
        let invitations = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallInvitation(_))
            })
            .await;
        assert_eq!(invitations.len(), 1, "callee should receive CallInvitation");
    }

    // client3 disconnects while everything still rings
    let client3_target = CallTarget::Client(client3.id().clone());
    client3.close().await;

    // The other ringing recipient sees the shrunk invited list; empty lists are
    // a live state for a ringing recipient
    let updates = client2
        .recv_until_timeout_with_filter(Duration::from_millis(500), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id && update.invited_targets.is_empty())
        })
        .await;
    assert_eq!(
        updates.len(),
        1,
        "the remaining ringing recipient should receive a CallUpdate without the lost target"
    );

    // The caller gets explicit feedback about the lost target
    let cancelled = client1
        .recv_until_timeout_with_filter(Duration::from_millis(500), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled)
                if cancelled.call_id == call_id
                    && cancelled.reason == vacs_protocol::ws::server::CallCancelReason::Disconnected
                    && cancelled.targets.contains(&client3_target))
        })
        .await;
    assert_eq!(
        cancelled.len(),
        1,
        "the caller should receive CallCancelled for the disconnected ringing target"
    );

    Ok(())
}

#[test(tokio::test)]
async fn call_error_with_call_failure_reason() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;

    // client2 fails to handle the call locally and reports a generic call failure,
    // as the client maps e.g. WebRTC setup errors to CallFailure
    client2
        .send(ClientMessage::CallError(
            vacs_protocol::ws::shared::CallError {
                call_id,
                reason: vacs_protocol::ws::shared::CallErrorReason::CallFailure,
                message: None,
            },
        ))
        .await?;

    // The only ringing target failed, so the caller must learn the call is over
    let cancelled_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallCancelled(_))
        })
        .await;
    assert_eq!(
        cancelled_messages.len(),
        1,
        "client1 should receive CallCancelled after the only target errored"
    );
    match &cancelled_messages[0] {
        ServerMessage::CallCancelled(cancelled) => {
            assert_eq!(
                cancelled.targets,
                HashSet::from([CallTarget::Client(client2.id().clone())]),
                "CallCancelled targets mismatch"
            );
            assert_eq!(
                cancelled.reason,
                vacs_protocol::ws::server::CallCancelReason::Errored(
                    vacs_protocol::ws::shared::CallErrorReason::CallFailure
                ),
                "CallCancelReason mismatch"
            );
        }
        message => panic!("Unexpected message: {:?}, expected CallCancelled", message),
    };

    // The failed call must not leave the caller marked busy
    let new_call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: new_call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client3.id().clone())]),
                prio: false,
            },
        ))
        .await?;

    let error_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(_))
        })
        .await;
    assert!(
        error_messages.is_empty(),
        "client1 should not be considered busy after its call failed, but received: {:?}",
        error_messages
    );

    let invite_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client3 should receive CallInvite for the new call"
    );

    Ok(())
}

#[test(tokio::test)]
async fn target_not_found() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 5).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: CallId::new(),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(ClientId::from("client69"))]),
                prio: false,
            },
        ))
        .await?;

    // Expect empty offer/invite on client2 (which is fine, it's not target)
    let call_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(
                m,
                ServerMessage::WebrtcOffer(_) | ServerMessage::CallInvitation(_)
            )
        })
        .await;

    assert!(
        call_messages.is_empty(),
        "client2 should have received no messages, but received: {:?}",
        call_messages
    );

    let peer_not_found_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(_))
        })
        .await;

    assert_eq!(
        peer_not_found_messages.len(),
        1,
        "client1 should have received exactly one CallError messages"
    );

    match &peer_not_found_messages[0] {
        ServerMessage::CallError(error) => {
            assert_eq!(
                error.reason,
                vacs_protocol::ws::shared::CallErrorReason::TargetsNotFound(HashSet::from([
                    CallTarget::Client(ClientId::from("client69"))
                ])),
                "CallErrorReason mismatch"
            );
        }
        message => panic!(
            "Unexpected message: {:?}, expected Error from server",
            message
        ),
    };

    for (i, client) in clients.iter_mut().enumerate() {
        let call_offer_messages = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::WebrtcOffer(_) | ServerMessage::Error(_))
            })
            .await;

        assert!(
            call_offer_messages.is_empty(),
            "client{} should have received no messages, but received: {:?}",
            i + 3,
            call_offer_messages
        );
    }

    Ok(())
}

#[test(tokio::test)]
async fn webrtc_messages_to_non_participants_are_dropped() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    // Establish a call between client1 and client2
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(_))
        })
        .await;
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let _ = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && update.joined_participants.contains_key(client2.id()))
        })
        .await;

    // client1 addresses WebRTC messages to client3, who is not a participant
    client1
        .send(ClientMessage::WebrtcOffer(
            vacs_protocol::ws::shared::WebrtcOffer {
                call_id,
                from_client_id: client1.id().clone(),
                to_client_id: client3.id().clone(),
                sdp: "sdp1".to_string(),
            },
        ))
        .await?;
    client1
        .send(ClientMessage::WebrtcIceCandidate(
            vacs_protocol::ws::shared::WebrtcIceCandidate {
                call_id,
                from_client_id: client1.id().clone(),
                to_client_id: client3.id().clone(),
                candidate: "candidate1".to_string(),
            },
        ))
        .await?;
    client2
        .send(ClientMessage::WebrtcAnswer(
            vacs_protocol::ws::shared::WebrtcAnswer {
                call_id,
                from_client_id: client2.id().clone(),
                to_client_id: client3.id().clone(),
                sdp: "sdp2".to_string(),
            },
        ))
        .await?;

    let relayed_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(200), |m| {
            matches!(
                m,
                ServerMessage::WebrtcOffer(_)
                    | ServerMessage::WebrtcAnswer(_)
                    | ServerMessage::WebrtcIceCandidate(_)
            )
        })
        .await;
    assert!(
        relayed_messages.is_empty(),
        "client3 is not a participant and must not receive relayed WebRTC messages, but received: {:?}",
        relayed_messages
    );

    // The drop is silent: the senders get no error for the benign left-the-call race
    for (name, client) in [("client1", &mut client1), ("client2", &mut client2)] {
        let error_messages = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallError(_))
            })
            .await;
        assert!(
            error_messages.is_empty(),
            "{} should receive no error for a dropped relay, but received: {:?}",
            name,
            error_messages
        );
    }

    Ok(())
}

#[test(tokio::test)]
async fn partial_targets_not_found_still_rings_online_target() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 2).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    let offline_target = CallTarget::Client(ClientId::from("client69"));
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([
                    CallTarget::Client(client2.id().clone()),
                    offline_target.clone(),
                ]),
                prio: false,
            },
        ))
        .await?;

    // The unresolvable target is reported, naming only the offline one
    let error_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(_))
        })
        .await;
    assert_eq!(
        error_messages.len(),
        1,
        "client1 should receive exactly one CallError message"
    );
    match &error_messages[0] {
        ServerMessage::CallError(error) => {
            assert_eq!(
                error.reason,
                vacs_protocol::ws::shared::CallErrorReason::TargetsNotFound(HashSet::from([
                    offline_target
                ])),
                "CallErrorReason mismatch"
            );
        }
        message => panic!("Unexpected message: {:?}, expected CallError", message),
    };

    // The resolvable target still rings
    let invite_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == call_id)
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client2 should still receive CallInvitation despite the offline co-target"
    );

    // And the call is fully usable: accepting it produces the acceptance update
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let accept_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && update.joined_participants.contains_key(client2.id()))
        })
        .await;
    assert_eq!(
        accept_messages.len(),
        1,
        "client1 should receive a call update with client2 joined"
    );

    Ok(())
}

#[test(tokio::test)]
async fn all_targets_not_found_leaves_no_call_state() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 2).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    let offline_targets = HashSet::from([
        CallTarget::Client(ClientId::from("client69")),
        CallTarget::Client(ClientId::from("client70")),
    ]);
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: CallId::new(),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: offline_targets.clone(),
                prio: false,
            },
        ))
        .await?;

    let error_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(error)
            if error.reason
                == vacs_protocol::ws::shared::CallErrorReason::TargetsNotFound(
                    offline_targets.clone()
                ))
        })
        .await;
    assert_eq!(
        error_messages.len(),
        1,
        "client1 should receive TargetsNotFound naming both offline targets"
    );

    // No call state was created: the caller is free to place a new call at once
    let new_call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: new_call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let error_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(_))
        })
        .await;
    assert!(
        error_messages.is_empty(),
        "client1 should not be considered busy after an all-offline invite, but received: {:?}",
        error_messages
    );
    let invite_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == new_call_id)
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client2 should receive CallInvitation for the follow-up call"
    );

    Ok(())
}

#[test(tokio::test)]
async fn empty_targets_rejected() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 2).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: CallId::new(),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::new(),
                prio: false,
            },
        ))
        .await?;

    let error_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(error)
                if error.reason == vacs_protocol::ws::shared::CallErrorReason::Other)
        })
        .await;
    assert_eq!(
        error_messages.len(),
        1,
        "client1 should receive CallError for an invite without targets"
    );

    // The rejected invite must not leave the caller marked busy
    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let invite_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == call_id)
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client2 should receive CallInvitation for the follow-up call"
    );

    Ok(())
}

/// Invites `callee` into `call_id` and lets it accept, so that the call either
/// starts or grows by one participant.
async fn join_call(
    caller: &mut TestClient,
    callee: &mut TestClient,
    call_id: CallId,
) -> anyhow::Result<()> {
    caller
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: caller.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(callee.id().clone())]),
                prio: false,
            },
        ))
        .await?;

    let invitations = callee
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == call_id)
        })
        .await;
    assert_eq!(invitations.len(), 1, "callee should receive CallInvitation");

    callee
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: callee.id().clone(),
            },
        ))
        .await?;

    let callee_id = callee.id().clone();
    for client in [caller, callee] {
        let updates = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && update.joined_participants.contains_key(&callee_id))
            })
            .await;
        assert_eq!(updates.len(), 1, "participant should see the callee join");
    }

    Ok(())
}

#[test(tokio::test)]
async fn auto_hangup_drops_a_ringing_target_without_ending_the_call() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    join_call(&mut client1, &mut client2, call_id).await?;

    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client3.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let invitations = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == call_id)
        })
        .await;
    assert_eq!(
        invitations.len(),
        1,
        "client3 should receive CallInvitation"
    );

    client1
        .send(ClientMessage::CallDropTarget(
            vacs_protocol::ws::client::CallDropTarget {
                call_id,
                target: CallTarget::Client(client3.id().clone()),
                reason: vacs_protocol::ws::client::CallDropReason::AutoHangup,
            },
        ))
        .await?;

    let cancelled_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled)
                if cancelled.call_id == call_id
                    && cancelled.reason == vacs_protocol::ws::server::CallCancelReason::Errored(
                        vacs_protocol::ws::shared::CallErrorReason::AutoHangup))
        })
        .await;
    assert_eq!(
        cancelled_messages.len(),
        1,
        "client3 should receive CallCancelled for the timed out invitation"
    );

    let client2_id = client2.id().clone();
    for client in [&mut client1, &mut client2] {
        let updates = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && update.invited_targets.is_empty()
                        && update.joined_participants.contains_key(&client2_id))
            })
            .await;
        assert_eq!(
            updates.len(),
            1,
            "remaining participants should see the dropped invitation while the call continues"
        );
    }

    Ok(())
}

#[test(tokio::test)]
async fn conference_leader_drops_a_participant() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    join_call(&mut client1, &mut client2, call_id).await?;
    join_call(&mut client1, &mut client3, call_id).await?;

    client2
        .send(ClientMessage::CallDropTarget(
            vacs_protocol::ws::client::CallDropTarget {
                call_id,
                target: CallTarget::Client(client3.id().clone()),
                reason: vacs_protocol::ws::client::CallDropReason::Requested,
            },
        ))
        .await?;
    let error_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(error)
            if error.call_id == call_id
                && matches!(
                    error.reason,
                    vacs_protocol::ws::shared::CallErrorReason::NotConferenceLeader(_)
                ))
        })
        .await;
    assert_eq!(
        error_messages.len(),
        1,
        "client2 should be refused as it does not lead the conference"
    );

    client1
        .send(ClientMessage::CallDropTarget(
            vacs_protocol::ws::client::CallDropTarget {
                call_id,
                target: CallTarget::Client(client3.id().clone()),
                reason: vacs_protocol::ws::client::CallDropReason::Requested,
            },
        ))
        .await?;

    let client1_id = client1.id().clone();
    let end_messages = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallEnd(end)
                if end.call_id == call_id && end.ending_client_id == client1_id)
        })
        .await;
    assert_eq!(
        end_messages.len(),
        1,
        "the dropped participant should receive CallEnd naming the conference leader"
    );

    let client3_id = client3.id().clone();
    for client in [&mut client1, &mut client2] {
        let updates = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(m, ServerMessage::CallUpdate(update)
                    if update.call_id == call_id
                        && !update.joined_participants.contains_key(&client3_id))
            })
            .await;
        assert_eq!(
            updates.len(),
            1,
            "remaining participants should see the dropped participant leave"
        );
    }

    Ok(())
}

#[test(tokio::test)]
async fn dropping_the_only_ringing_target_ends_the_call() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let invitations = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == call_id)
        })
        .await;
    assert_eq!(
        invitations.len(),
        1,
        "client2 should receive CallInvitation"
    );

    client1
        .send(ClientMessage::CallDropTarget(
            vacs_protocol::ws::client::CallDropTarget {
                call_id,
                target: CallTarget::Client(client2.id().clone()),
                reason: vacs_protocol::ws::client::CallDropReason::AutoHangup,
            },
        ))
        .await?;

    let cancelled_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled) if cancelled.call_id == call_id)
        })
        .await;
    assert_eq!(
        cancelled_messages.len(),
        1,
        "client2 should receive CallCancelled for the timed out invitation"
    );

    let updates = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && update.invited_targets.is_empty()
                    && update.joined_participants.is_empty())
        })
        .await;
    assert_eq!(
        updates.len(),
        1,
        "client1 should be told that the call is over"
    );

    // The ended call must not leave the caller marked busy
    let next_call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id: next_call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client3.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let invitations = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation)
                if invitation.call_id == next_call_id)
        })
        .await;
    assert_eq!(
        invitations.len(),
        1,
        "client3 should receive CallInvitation for the follow-up call"
    );

    Ok(())
}

/// A conference member reporting a call-scoped error leaves the call; the
/// survivors only get the roster change, never the reason, since their clients
/// treat call-scoped reasons as their own call failing.
#[test(tokio::test)]
async fn conference_member_call_failure_only_updates_survivors() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    join_call(&mut client1, &mut client2, call_id).await?;
    join_call(&mut client1, &mut client3, call_id).await?;

    client3
        .send(ClientMessage::CallError(
            vacs_protocol::ws::shared::CallError {
                call_id,
                reason: vacs_protocol::ws::shared::CallErrorReason::CallFailure,
                message: None,
            },
        ))
        .await?;

    for client in [&mut client1, &mut client2] {
        let messages = client
            .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
                matches!(
                    m,
                    ServerMessage::CallUpdate(_)
                        | ServerMessage::CallError(_)
                        | ServerMessage::CallEnd(_)
                )
            })
            .await;
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, ServerMessage::CallError(_) | ServerMessage::CallEnd(_))),
            "survivor must not be told the call failed, got {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, ServerMessage::CallUpdate(update)
                if update.call_id == call_id
                    && !update.joined_participants.contains_key(client3.id()))),
            "survivor should see the erroring member leave, got {messages:?}"
        );
    }

    Ok(())
}

/// A busy callee accepting counts as that callee failing the target, so a
/// target it was the only client of fails out for the caller.
#[test(tokio::test)]
async fn busy_accept_fails_the_ringing_target() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let busy_call_id = CallId::new();
    join_call(&mut client2, &mut client3, busy_call_id).await?;

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([CallTarget::Client(client2.id().clone())]),
                prio: false,
            },
        ))
        .await?;
    let invitations = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvitation(invitation) if invitation.call_id == call_id)
        })
        .await;
    assert_eq!(
        invitations.len(),
        1,
        "busy client still receives the invitation"
    );

    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;

    let errors = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallError(error)
                if error.call_id == call_id
                    && error.reason == vacs_protocol::ws::shared::CallErrorReason::CallActive)
        })
        .await;
    assert_eq!(
        errors.len(),
        1,
        "busy client is told its accept was refused"
    );

    let cancelled = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallCancelled(cancelled)
            if cancelled.call_id == call_id
                && cancelled.reason
                    == vacs_protocol::ws::server::CallCancelReason::Errored(
                        vacs_protocol::ws::shared::CallErrorReason::CallActive
                    ))
        })
        .await;
    assert_eq!(cancelled.len(), 1, "caller learns the busy target failed");

    let busy_participants = client3
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallUpdate(_) | ServerMessage::CallEnd(_))
        })
        .await;
    assert!(
        busy_participants.is_empty(),
        "the busy client's own call is untouched"
    );

    Ok(())
}

/// A refused drop is answered with the reason and the authoritative call state,
/// so a client that applied the drop locally converges back.
#[test(tokio::test)]
async fn refused_drop_carries_the_authoritative_call_state() -> anyhow::Result<()> {
    let test_app = TestApp::new().await;
    let mut clients = setup_n_test_clients(test_app.addr(), 3).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);
    let mut client3 = clients.remove(0);

    let call_id = CallId::new();
    join_call(&mut client1, &mut client2, call_id).await?;
    join_call(&mut client1, &mut client3, call_id).await?;

    client2
        .send(ClientMessage::CallDropTarget(
            vacs_protocol::ws::client::CallDropTarget {
                call_id,
                target: CallTarget::Client(client3.id().clone()),
                reason: vacs_protocol::ws::client::CallDropReason::Requested,
            },
        ))
        .await?;

    let messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(
                m,
                ServerMessage::CallError(_) | ServerMessage::CallUpdate(_)
            )
        })
        .await;
    assert!(
        messages.iter().any(|m| matches!(m, ServerMessage::CallError(error)
            if matches!(error.reason, vacs_protocol::ws::shared::CallErrorReason::NotConferenceLeader(_)))),
        "client2 should be refused, got {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, ServerMessage::CallUpdate(update)
            if update.call_id == call_id
                && update.joined_participants.contains_key(client3.id())
                && update.conference_leader.as_ref() == Some(client1.id()))),
        "the refusal is followed by the current call state, got {messages:?}"
    );

    Ok(())
}
