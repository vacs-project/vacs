use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::shared::{CallId, WebrtcOffer};
use vacs_server::test_utils::{TestEnv, connect_to_websocket};

#[tokio::test]
async fn websocket_ping_pong() {
    let env = TestEnv::builder().build().await;
    let mut ws_stream = connect_to_websocket(env.ws_url()).await;

    ws_stream
        .send(tungstenite::Message::Ping(tungstenite::Bytes::from_static(
            b"ping",
        )))
        .await
        .expect("Failed to send ping message");

    match ws_stream.next().await {
        Some(Ok(tungstenite::Message::Pong(_))) => (),
        _ => panic!("Did not receive pong message"),
    }
}

#[tokio::test]
async fn malformed_message_content_is_not_logged() -> anyhow::Result<()> {
    const SDP: &str = "v=0\r\no=- 4611731400430051336 2 IN IP4 192.0.2.10\r\ns=-\r\n\
        a=candidate:1 1 udp 2122260223 192.0.2.10 54400 typ host\r\n";

    let env = TestEnv::builder().default_users(2).build().await;
    let mut clients = env.setup_clients(2).await;
    let mut client2 = clients.pop().unwrap();
    let mut client1 = clients.pop().unwrap();

    let call_id = CallId::new();
    let offer = ClientMessage::WebrtcOffer(WebrtcOffer {
        call_id,
        from_client_id: client1.id().clone(),
        to_client_id: client2.id().clone(),
        sdp: "sdp".to_string(),
    });
    let sdp_in_place_of_the_call_id = ClientMessage::serialize(&offer)?.replace(
        &call_id.to_string(),
        serde_json::to_string(SDP)?.trim_matches('"'),
    );
    assert!(sdp_in_place_of_the_call_id.contains("v=0"));

    client1
        .send_raw(tungstenite::Message::from(sdp_in_place_of_the_call_id))
        .await?;
    client1
        .recv_raw_until_timeout(Duration::from_millis(200))
        .await;
    client2
        .recv_raw_until_timeout(Duration::from_millis(200))
        .await;

    vacs_server::logging::guard::assert_clean();
    Ok(())
}
