use pretty_assertions::assert_eq;
use std::time::Duration;
use test_log::test;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::Bytes;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{self, ServerMessage};
use vacs_server::test_utils::{TestClient, TestEnv, cid};

#[test(tokio::test)]
async fn client_connected() -> anyhow::Result<()> {
    let env = TestEnv::builder().default_users(5).build().await;
    let mut clients = env.setup_clients(5).await;
    let client_count = clients.len();

    for (i, client) in clients.iter_mut().enumerate() {
        let messages = client.recv_until_timeout(Duration::from_millis(100)).await;

        let expected_message_count = client_count - i - 1;
        assert_eq!(
            messages.len(),
            expected_message_count,
            "Client{} did not receive expected number of messages",
            i + 1
        );

        let expected_ids: Vec<_> = (i + 2..=client_count)
            .map(|i| ClientId::from(cid(i)))
            .collect();

        for message in messages {
            if let ServerMessage::ClientConnected(server::ClientConnected { client }) = message {
                assert!(
                    expected_ids.contains(&client.id),
                    "Unexpected client ID: {:?}, expected one of: {:?}",
                    client.id,
                    expected_ids
                );
            } else {
                panic!("Unexpected message: {message:?}");
            }
        }
    }

    Ok(())
}

#[test(tokio::test)]
async fn client_disconnected() -> anyhow::Result<()> {
    let env = TestEnv::builder().default_users(5).build().await;
    let mut clients = env.setup_clients(5).await;
    let initial_client_count = clients.len();

    clients
        .last_mut()
        .unwrap()
        .send(ClientMessage::Logout)
        .await
        .expect("Failed to send logout message");

    for (i, client) in clients.iter_mut().enumerate() {
        let messages = client.recv_until_timeout(Duration::from_millis(100)).await;

        let expected_message_count = if i == initial_client_count - 1 {
            0 // last client receives no login or logout messages
        } else {
            initial_client_count - i
        };

        assert_eq!(
            messages.len(),
            expected_message_count,
            "Client{} did not receive expected number of messages",
            i + 1
        );

        let expected_ids: Vec<_> = (i + 2..=initial_client_count)
            .map(|i| ClientId::from(cid(i)))
            .collect();

        for message in messages {
            match message {
                ServerMessage::ClientConnected(server::ClientConnected { client }) => {
                    assert!(
                        expected_ids.contains(&client.id),
                        "Unexpected client ID: {:?}, expected one of: {:?}",
                        client.id,
                        expected_ids
                    );
                }
                ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
                    assert_eq!(
                        client_id,
                        ClientId::from(cid(initial_client_count)),
                        "Unexpected client ID: {:?}",
                        client_id
                    );
                }
                message => {
                    panic!("Unexpected message: {message:?}");
                }
            }
        }
    }

    Ok(())
}

#[test(tokio::test)]
async fn client_dropped() -> anyhow::Result<()> {
    let env = TestEnv::builder().default_users(5).build().await;
    let mut clients = env.setup_clients(5).await;
    let initial_client_count = clients.len();
    clients.pop();

    for (i, client) in clients.iter_mut().enumerate() {
        let messages = client.recv_until_timeout(Duration::from_millis(100)).await;

        let expected_message_count = initial_client_count - i;
        assert_eq!(
            messages.len(),
            expected_message_count,
            "Client{} did not receive expected number of messages",
            i + 1
        );

        let expected_ids: Vec<_> = (i + 2..=initial_client_count)
            .map(|i| ClientId::from(cid(i)))
            .collect();

        for message in messages {
            match message {
                ServerMessage::ClientConnected(server::ClientConnected { client }) => {
                    assert!(
                        expected_ids.contains(&client.id),
                        "Unexpected client ID: {:?}, expected one of: {:?}",
                        client.id,
                        expected_ids
                    );
                }
                ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
                    assert_eq!(
                        client_id,
                        ClientId::from(cid(initial_client_count)),
                        "Unexpected client ID: {:?}",
                        client_id
                    );
                }
                message => {
                    panic!("Unexpected message: {message:?}");
                }
            }
        }
    }

    Ok(())
}

#[test(tokio::test)]
async fn control_messages() -> anyhow::Result<()> {
    let env = TestEnv::builder().default_users(1).build().await;
    let token = env.ws_token_for(cid(1)).await.unwrap();
    let mut client = TestClient::new_with_login(
        env.ws_url(),
        cid(1).as_str(),
        &token,
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    )
    .await
    .expect("Failed to create client");

    for n in 0..10 {
        client
            .send_raw_and_expect(
                tungstenite::Message::Ping(Bytes::from(format!("ping{n}"))),
                |message| {
                    assert_eq!(
                        message,
                        tungstenite::Message::Pong(Bytes::from(format!("ping{n}")))
                    );
                },
            )
            .await
            .expect("Expected server to respond to pings");
    }

    Ok(())
}
