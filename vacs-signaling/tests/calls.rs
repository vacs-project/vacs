use std::collections::HashSet;
use std::time::Duration;
use test_log::test;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::ServerMessage;
use vacs_signaling::client::SignalingEvent;
use vacs_signaling::test_utils::TestRig;

#[test(tokio::test)]
async fn call_offer_answer() {
    let mut test_rig = TestRig::new(2).await;

    let clients = test_rig.clients_mut();

    // 1. Client 0 sends Call Invite
    let call_id = vacs_protocol::ws::shared::CallId::new();
    clients[0]
        .client
        .send(ClientMessage::CallInvite(
            vacs_protocol::ws::client::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: ClientId::from("client0"),
                    position_id: None,
                    station_id: None,
                },
                targets: HashSet::from([vacs_protocol::ws::shared::CallTarget::Client(
                    ClientId::from("client1"),
                )]),
                prio: false,
            },
        ))
        .await
        .unwrap();

    // 2. Client 1 receives Call Invite
    let event = clients[1]
        .recv_with_timeout_and_filter(Duration::from_millis(100), |e| {
            matches!(e, SignalingEvent::Message(ServerMessage::CallInvitation(vacs_protocol::ws::server::CallInvitation {
                call_id: received_call_id,
                source,
                ..
            })) if *received_call_id == call_id && source.client_id.as_str() == "client0")
        })
        .await;
    assert!(event.is_some());

    // 3. Client 1 accepts Call
    clients[1]
        .client
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::client::CallAccept {
                call_id,
                accepting_client_id: ClientId::from("client1"),
            },
        ))
        .await
        .unwrap();

    // 4. Client 0 receives Call Accept
    let event = clients[0]
        .recv_with_timeout_and_filter(Duration::from_millis(100), |e| {
            matches!(e, SignalingEvent::Message(ServerMessage::CallAcceptance(vacs_protocol::ws::server::CallAcceptance {
                call_id: received_call_id,
                target,
                accepting_client_id,
                ..
            })) if matches!(target, vacs_protocol::ws::shared::CallTarget::Client(client_id) if client_id.as_str() == "client1") && *received_call_id == call_id && accepting_client_id.as_str() == "client1")
        })
        .await;
    assert!(event.is_some());

    // 5. Client 0 sends WebRTC Offer
    clients[0]
        .client
        .send(ClientMessage::WebrtcOffer(
            vacs_protocol::ws::shared::WebrtcOffer {
                call_id,
                from_client_id: ClientId::from("client0"),
                to_client_id: ClientId::from("client1"),
                sdp: "sdp0".to_string(),
            },
        ))
        .await
        .unwrap();

    // 6. Client 1 receives WebRTC Offer
    let event = clients[1]
        .recv_with_timeout_and_filter(Duration::from_millis(100), |e| {
            matches!(e, SignalingEvent::Message(ServerMessage::WebrtcOffer(vacs_protocol::ws::shared::WebrtcOffer {
                from_client_id,
                sdp,
                ..
            })) if from_client_id.as_str() == "client0" && sdp == "sdp0")
        })
        .await;
    assert!(event.is_some());

    // 7. Client 1 sends WebRTC Answer
    clients[1]
        .client
        .send(ClientMessage::WebrtcAnswer(
            vacs_protocol::ws::shared::WebrtcAnswer {
                call_id,
                from_client_id: ClientId::from("client1"),
                to_client_id: ClientId::from("client0"),
                sdp: "sdp1".to_string(),
            },
        ))
        .await
        .unwrap();

    // 8. Client 0 receives WebRTC Answer
    let event = clients[0]
        .recv_with_timeout_and_filter(Duration::from_millis(100), |e| {
            matches!(e, SignalingEvent::Message(ServerMessage::WebrtcAnswer(vacs_protocol::ws::shared::WebrtcAnswer {
                from_client_id,
                sdp,
                ..
            })) if from_client_id.as_str() == "client1" && sdp == "sdp1")
        })
        .await;
    assert!(event.is_some());
}
