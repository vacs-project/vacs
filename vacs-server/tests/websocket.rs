use futures_util::{SinkExt, StreamExt};
use test_log::test;
use tokio_tungstenite::tungstenite;
use vacs_server::test_utils::{TestEnv, connect_to_websocket};

#[test(tokio::test)]
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
