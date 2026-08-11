use futures_util::{SinkExt, StreamExt};
use pretty_assertions::assert_eq;
use std::time::Duration;
use test_log::test;
use tokio_tungstenite::tungstenite;
use vacs_protocol::VACS_PROTOCOL_VERSION;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{self, ServerMessage};
use vacs_server::test_utils::{
    TestClient, TestEnv, assert_message_matches, assert_raw_message_matches, cid,
    connect_to_websocket,
};

#[test(tokio::test)]
async fn login() {
    let env = TestEnv::builder().default_users(2).build().await;
    let token1 = env.ws_token_for(cid(1)).await.unwrap();
    let token2 = env.ws_token_for(cid(2)).await.unwrap();

    let _client1 = TestClient::new_with_login(
        env.ws_url(),
        cid(1).as_str(),
        &token1,
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, cid(1));
            Ok(())
        },
        |clients| {
            assert_eq!(clients.len(), 0);
            Ok(())
        },
        |stations| {
            assert_eq!(stations.len(), 0);
            Ok(())
        },
    )
    .await
    .expect("Failed to log in first client");

    let _client2 = TestClient::new_with_login(
        env.ws_url(),
        cid(2).as_str(),
        &token2,
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, cid(2));
            Ok(())
        },
        |clients| {
            assert_eq!(clients.len(), 1);
            assert_eq!(clients[0].id, ClientId::from(cid(1)));
            assert_eq!(clients[0].display_name, cid(1));
            Ok(())
        },
        |stations| {
            assert_eq!(stations.len(), 0);
            Ok(())
        },
    )
    .await
    .expect("Failed to log in second client");
}

#[test(tokio::test)]
async fn duplicate_login() {
    let env = TestEnv::builder().default_users(1).build().await;
    let token = env.ws_token_for(cid(1)).await.unwrap();

    let _client1 = TestClient::new_with_login(
        env.ws_url(),
        cid(1).as_str(),
        &token,
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, cid(1));
            Ok(())
        },
        |clients| {
            assert_eq!(clients.len(), 0);
            Ok(())
        },
        |stations| {
            assert_eq!(stations.len(), 0);
            Ok(())
        },
    )
    .await
    .expect("Failed to log in first client");

    assert!(
        TestClient::new_with_login(
            env.ws_url(),
            cid(1).as_str(),
            &token,
            |_, _| Ok(()),
            |_| Ok(()),
            |_| Ok(())
        )
        .await
        .is_err_and(|err| { err.to_string() == "Login failed: DuplicateId" })
    );
}

#[test(tokio::test)]
async fn invalid_login() {
    let env = TestEnv::builder().build().await;

    assert!(
        TestClient::new_with_login(
            env.ws_url(),
            "anything",
            "",
            |_, _| Ok(()),
            |_| Ok(()),
            |_| Ok(())
        )
        .await
        .is_err_and(|err| { err.to_string() == "Login failed: InvalidCredentials" })
    );
}

#[test(tokio::test)]
async fn unauthorized_message_before_login() {
    let env = TestEnv::builder().build().await;

    let mut ws_stream = connect_to_websocket(env.ws_url()).await;

    ws_stream
        .send(tungstenite::Message::from(
            ClientMessage::serialize(&ClientMessage::ListClients).unwrap(),
        ))
        .await
        .expect("Failed to send ListClients message");

    let message_result = ws_stream.next().await;
    assert_raw_message_matches(message_result, |response| match response {
        ServerMessage::LoginFailure(server::LoginFailure { reason }) => {
            assert_eq!(
                reason,
                server::LoginFailureReason::Unauthorized,
                "Unexpected reason for LoginFailure"
            );
        }
        _ => panic!("Unexpected response: {response:?}"),
    });
}

#[test(tokio::test)]
async fn simultaneous_login_attempts() {
    let env = TestEnv::builder().default_users(1).build().await;
    let cid1 = cid(1);
    let token = env.ws_token_for(cid1.clone()).await.unwrap();

    let attempt1 = TestClient::new_with_login(
        env.ws_url(),
        cid1.as_str(),
        &token,
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    );
    let attempt2 = TestClient::new_with_login(
        env.ws_url(),
        cid1.as_str(),
        &token,
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    );

    let (attempt1_result, attempt2_result) = tokio::join!(attempt1, attempt2);

    assert!(
        (attempt1_result.is_ok() && attempt2_result.is_err())
            || (attempt1_result.is_err() && attempt2_result.is_ok()),
        "Expected one attempt to succeed and one to fail with IdTaken"
    );
}

#[test(tokio::test)]
#[cfg_attr(target_os = "windows", ignore)]
async fn login_timeout() {
    let env = TestEnv::builder().build().await;

    let mut ws_stream = connect_to_websocket(env.ws_url()).await;

    tokio::time::sleep(Duration::from_millis(
        env.state().config.auth.login_flow_timeout_millis + 50,
    ))
    .await;

    ws_stream
        .send(tungstenite::Message::from(
            ClientMessage::serialize(&ClientMessage::Login(vacs_protocol::ws::client::Login {
                token: "token".to_string(),
                protocol_version: VACS_PROTOCOL_VERSION.to_string(),
                custom_profile: false,
                position_id: None,
            }))
            .unwrap(),
        ))
        .await
        .expect("Failed to send login message");

    match ws_stream.next().await {
        Some(Ok(tungstenite::Message::Text(response))) => {
            match ServerMessage::deserialize(&response) {
                Ok(ServerMessage::LoginFailure(server::LoginFailure { reason })) => {
                    assert_eq!(reason, server::LoginFailureReason::Timeout);
                }
                _ => panic!("Unexpected response: {response:?}"),
            }
        }
        other => panic!("Unexpected response: {other:?}"),
    }
}

#[test(tokio::test)]
async fn client_connected() {
    let env = TestEnv::builder().default_users(2).build().await;
    let mut clients = env.setup_clients_map(2).await;

    let client1 = clients.get_mut(&ClientId::from(cid(1))).unwrap();
    let client_connected = client1.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_connected, |message| match message {
        ServerMessage::ClientConnected(server::ClientConnected { client }) => {
            assert_eq!(client.id, ClientId::from(cid(2)));
            assert_eq!(client.display_name, cid(2));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });

    let client2 = clients.get_mut(&ClientId::from(cid(2))).unwrap();
    assert!(
        client2
            .recv_with_timeout(Duration::from_millis(100))
            .await
            .is_none()
    );
}

#[test(tokio::test)]
async fn client_disconnected() {
    let env = TestEnv::builder().default_users(2).build().await;
    let mut clients = env.setup_clients_map(2).await;

    let client1 = clients.get_mut(&ClientId::from(cid(1))).unwrap();
    let client_connected = client1.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_connected, |message| match message {
        ServerMessage::ClientConnected(server::ClientConnected { client }) => {
            assert_eq!(client.id, ClientId::from(cid(2)));
            assert_eq!(client.display_name, cid(2));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });

    client1.close().await;

    let client2 = clients.get_mut(&ClientId::from(cid(2))).unwrap();
    let client_disconnected = client2.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_disconnected, |message| match message {
        ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
            assert_eq!(client_id, ClientId::from(cid(1)));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });
}

#[test(tokio::test)]
async fn login_client_list() {
    let env = TestEnv::builder().default_users(4).build().await;
    let _clients = env.setup_clients(3).await;

    let token4 = env.ws_token_for(cid(4)).await.unwrap();
    let _client4 = TestClient::new_with_login(
        env.ws_url(),
        cid(4).as_str(),
        &token4,
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, cid(4));
            Ok(())
        },
        |clients| {
            assert_eq!(clients.len(), 3);
            assert!(
                clients
                    .iter()
                    .any(|client| client.id == ClientId::from(cid(1)))
            );
            assert!(
                clients
                    .iter()
                    .any(|client| client.id == ClientId::from(cid(2)))
            );
            assert!(
                clients
                    .iter()
                    .any(|client| client.id == ClientId::from(cid(3)))
            );
            Ok(())
        },
        |stations| {
            assert_eq!(stations.len(), 0);
            Ok(())
        },
    )
    .await
    .expect("Failed to log in fourth client");
}

#[test(tokio::test)]
async fn logout() {
    let env = TestEnv::builder().default_users(2).build().await;
    let mut clients = env.setup_clients_map(2).await;

    let client1 = clients.get_mut(&ClientId::from(cid(1))).unwrap();
    let client_connected = client1.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_connected, |message| match message {
        ServerMessage::ClientConnected(server::ClientConnected { client }) => {
            assert_eq!(client.id, ClientId::from(cid(2)));
            assert_eq!(client.display_name, cid(2));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });

    client1.send(ClientMessage::Logout).await.unwrap();
    assert!(
        client1
            .recv_with_timeout(Duration::from_millis(100))
            .await
            .is_none()
    );

    let client2 = clients.get_mut(&ClientId::from(cid(2))).unwrap();
    let client_disconnected = client2.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_disconnected, |message| match message {
        ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
            assert_eq!(client_id, ClientId::from(cid(1)));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });
}
