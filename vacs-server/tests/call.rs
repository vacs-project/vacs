use std::time::Duration;
use test_log::test;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::ServerMessage;
use vacs_protocol::ws::shared::{CallId, CallTarget};
use vacs_server::test_utils::TestEnv;

#[test(tokio::test)]
async fn call_offer() -> anyhow::Result<()> {
    let env = TestEnv::builder().default_users(5).build().await;
    let mut clients = env.setup_clients(5).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    let call_id = CallId::new();
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::shared::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                target: CallTarget::Client(client2.id().clone()),
                prio: false,
            },
        ))
        .await?;

    let invite_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvite(_))
        })
        .await;
    assert_eq!(
        invite_messages.len(),
        1,
        "client2 should receive CallInvite"
    );

    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::shared::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;

    let accept_messages = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallAccept(_))
        })
        .await;
    assert_eq!(
        accept_messages.len(),
        1,
        "client1 should receive CallAccept"
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
    let env = TestEnv::builder().default_users(5).build().await;
    let mut clients = env.setup_clients(5).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    let call_id = CallId::new();
    // Setup call first
    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::shared::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                target: CallTarget::Client(client2.id().clone()),
                prio: false,
            },
        ))
        .await?;
    let _ = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallInvite(_))
        })
        .await;
    client2
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::shared::CallAccept {
                call_id,
                accepting_client_id: client2.id().clone(),
            },
        ))
        .await?;
    let _ = client1
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(m, ServerMessage::CallAccept(_))
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
async fn target_not_found() -> anyhow::Result<()> {
    let env = TestEnv::builder().default_users(5).build().await;
    let mut clients = env.setup_clients(5).await;

    let mut client1 = clients.remove(0);
    let mut client2 = clients.remove(0);

    client1
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::shared::CallInvite {
                call_id: CallId::new(),
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: client1.id().clone(),
                    position_id: None,
                    station_id: None,
                },
                target: CallTarget::Client(ClientId::from("9999999")),
                prio: false,
            },
        ))
        .await?;

    // Expect empty offer/invite on client2 (which is fine, it's not target)
    let call_messages = client2
        .recv_until_timeout_with_filter(Duration::from_millis(100), |m| {
            matches!(
                m,
                ServerMessage::WebrtcOffer(_) | ServerMessage::CallInvite(_)
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
        "client1 should have received exactly one CallError message"
    );

    match &peer_not_found_messages[0] {
        ServerMessage::CallError(error) => {
            assert_eq!(
                error.reason,
                vacs_protocol::ws::shared::CallErrorReason::TargetNotFound,
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
