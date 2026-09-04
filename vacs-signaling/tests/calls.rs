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
            vacs_protocol::ws::shared::CallInvite {
                call_id,
                source: vacs_protocol::ws::shared::CallSource {
                    client_id: ClientId::from("1000001"),
                    position_id: None,
                    station_id: None,
                },
                target: vacs_protocol::ws::shared::CallTarget::Client(ClientId::from("1000002")),
                prio: false,
            },
        ))
        .await
        .unwrap();

    // 2. Client 1 receives Call Invite
    let event = clients[1]
        .recv_with_timeout_and_filter(Duration::from_millis(100), |e| {
            matches!(e, SignalingEvent::Message(ServerMessage::CallInvite(vacs_protocol::ws::shared::CallInvite {
                call_id: received_call_id,
                source,
                ..
            })) if *received_call_id == call_id && source.client_id.as_str() == "1000001")
        })
        .await;
    assert!(event.is_some());

    // 3. Client 1 accepts Call
    clients[1]
        .client
        .send(ClientMessage::CallAccept(
            vacs_protocol::ws::shared::CallAccept {
                call_id,
                accepting_client_id: ClientId::from("1000002"),
            },
        ))
        .await
        .unwrap();

    // 4. Client 0 receives Call Accept
    let event = clients[0]
        .recv_with_timeout_and_filter(Duration::from_millis(100), |e| {
            matches!(e, SignalingEvent::Message(ServerMessage::CallAccept(vacs_protocol::ws::shared::CallAccept {
                call_id: received_call_id,
                accepting_client_id,
                ..
            })) if *received_call_id == call_id && accepting_client_id.as_str() == "1000002")
        })
        .await;
    assert!(event.is_some());

    // 5. Client 0 sends WebRTC Offer
    clients[0]
        .client
        .send(ClientMessage::WebrtcOffer(
            vacs_protocol::ws::shared::WebrtcOffer {
                call_id,
                from_client_id: ClientId::from("1000001"),
                to_client_id: ClientId::from("1000002"),
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
            })) if from_client_id.as_str() == "1000001" && sdp == "sdp0")
        })
        .await;
    assert!(event.is_some());

    // 7. Client 1 sends WebRTC Answer
    clients[1]
        .client
        .send(ClientMessage::WebrtcAnswer(
            vacs_protocol::ws::shared::WebrtcAnswer {
                call_id,
                from_client_id: ClientId::from("1000002"),
                to_client_id: ClientId::from("1000001"),
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
            })) if from_client_id.as_str() == "1000002" && sdp == "sdp1")
        })
        .await;
    assert!(event.is_some());
}
