use futures_util::{SinkExt, StreamExt};
use pretty_assertions::assert_eq;
use std::time::Duration;
use test_log::test;
use tokio_tungstenite::tungstenite;
use vacs_protocol::VACS_PROTOCOL_VERSION;
use vacs_protocol::vatsim::ClientId;
use vacs_protocol::ws::client::ClientMessage;
use vacs_protocol::ws::server::{self, ServerMessage};
use vacs_server::config::{AppConfig, CallConfig};
use vacs_server::ratelimit::RateLimiters;
use vacs_server::test_utils::{
    TestApp, TestClient, assert_message_matches, assert_raw_message_matches, connect_to_websocket,
    setup_test_clients,
};

#[test(tokio::test)]
async fn login() {
    let test_app = TestApp::new().await;

    let _client1 = TestClient::new_with_login(
        test_app.addr(),
        "client1",
        "token1",
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, "client1");
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
        test_app.addr(),
        "client2",
        "token2",
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, "client2");
            Ok(())
        },
        |clients| {
            assert_eq!(clients.len(), 1);
            assert_eq!(clients[0].id, ClientId::from("client1"));
            assert_eq!(clients[0].display_name, "client1");
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
    let test_app = TestApp::new().await;

    let _client1 = TestClient::new_with_login(
        test_app.addr(),
        "client1",
        "token1",
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, "client1");
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
            test_app.addr(),
            "client1",
            "token1",
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
    let test_app = TestApp::new().await;

    assert!(
        TestClient::new_with_login(
            test_app.addr(),
            "client1",
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
    let test_app = TestApp::new().await;

    let mut ws_stream = connect_to_websocket(test_app.addr()).await;

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
    let test_app = TestApp::new().await;

    let attempt1 = TestClient::new_with_login(
        test_app.addr(),
        "client1",
        "token1",
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    );
    let attempt2 = TestClient::new_with_login(
        test_app.addr(),
        "client1",
        "token1",
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
    let test_app = TestApp::new().await;

    let mut ws_stream = connect_to_websocket(test_app.addr()).await;

    tokio::time::sleep(Duration::from_millis(
        test_app.state().config.auth.login_flow_timeout_millis + 50,
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
    let test_app = TestApp::new().await;

    let mut clients = setup_test_clients(
        test_app.addr(),
        &[("client1", "token1"), ("client2", "token2")],
    )
    .await;

    let client1 = clients.get_mut(&ClientId::from("client1")).unwrap();
    let client_connected = client1.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_connected, |message| match message {
        ServerMessage::ClientConnected(server::ClientConnected { client }) => {
            assert_eq!(client.id, ClientId::from("client2"));
            assert_eq!(client.display_name, "client2");
        }
        _ => panic!("Unexpected message: {message:?}"),
    });

    let client2 = clients.get_mut(&ClientId::from("client2")).unwrap();
    assert!(
        client2
            .recv_with_timeout(Duration::from_millis(100))
            .await
            .is_none()
    );
}

#[test(tokio::test)]
async fn client_disconnected() {
    let test_app = TestApp::new().await;

    let mut clients = setup_test_clients(
        test_app.addr(),
        &[("client1", "token1"), ("client2", "token2")],
    )
    .await;

    let client1 = clients.get_mut(&ClientId::from("client1")).unwrap();
    let client_connected = client1.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_connected, |message| match message {
        ServerMessage::ClientConnected(server::ClientConnected { client }) => {
            assert_eq!(client.id, ClientId::from("client2"));
            assert_eq!(client.display_name, "client2");
        }
        _ => panic!("Unexpected message: {message:?}"),
    });

    client1.close().await;

    let client2 = clients.get_mut(&ClientId::from("client2")).unwrap();
    let client_disconnected = client2.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_disconnected, |message| match message {
        ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
            assert_eq!(client_id, ClientId::from("client1"));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });
}

#[test(tokio::test)]
async fn login_client_list() {
    let test_app = TestApp::new().await;

    let _clients = setup_test_clients(
        test_app.addr(),
        &[
            ("client1", "token1"),
            ("client2", "token2"),
            ("client3", "token3"),
        ],
    )
    .await;

    let _client4 = TestClient::new_with_login(
        test_app.addr(),
        "client4",
        "token4",
        |own, info| {
            assert_eq!(own, true);
            assert_eq!(info.display_name, "client4");
            Ok(())
        },
        |clients| {
            assert_eq!(clients.len(), 3);
            assert!(
                clients
                    .iter()
                    .any(|client| client.id == ClientId::from("client1"))
            );
            assert!(
                clients
                    .iter()
                    .any(|client| client.id == ClientId::from("client2"))
            );
            assert!(
                clients
                    .iter()
                    .any(|client| client.id == ClientId::from("client3"))
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
    let test_app = TestApp::new().await;

    let mut clients = setup_test_clients(
        test_app.addr(),
        &[("client1", "token1"), ("client2", "token2")],
    )
    .await;

    let client1 = clients.get_mut(&ClientId::from("client1")).unwrap();
    let client_connected = client1.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_connected, |message| match message {
        ServerMessage::ClientConnected(server::ClientConnected { client }) => {
            assert_eq!(client.id, ClientId::from("client2"));
            assert_eq!(client.display_name, "client2");
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

    let client2 = clients.get_mut(&ClientId::from("client2")).unwrap();
    let client_disconnected = client2.recv_with_timeout(Duration::from_millis(100)).await;
    assert_message_matches(client_disconnected, |message| match message {
        ServerMessage::ClientDisconnected(server::ClientDisconnected { client_id }) => {
            assert_eq!(client_id, ClientId::from("client1"));
        }
        _ => panic!("Unexpected message: {message:?}"),
    });
}

/// The compatibility range from the release policy gates the websocket login:
/// a client speaking an older protocol is refused before its credentials are
/// even looked at.
#[test(tokio::test)]
async fn incompatible_protocol_version_is_refused() {
    let test_app = TestApp::new_with_protocol_range(">=3.0.0").await;

    let mut ws_stream = connect_to_websocket(test_app.addr()).await;
    ws_stream
        .send(tungstenite::Message::from(
            ClientMessage::serialize(&ClientMessage::Login(vacs_protocol::ws::client::Login {
                token: "token1".to_string(),
                protocol_version: "2.0.0".to_string(),
                custom_profile: false,
                position_id: None,
            }))
            .unwrap(),
        ))
        .await
        .expect("Failed to send login message");

    assert_raw_message_matches(ws_stream.next().await, |response| match response {
        ServerMessage::LoginFailure(server::LoginFailure { reason }) => {
            assert_eq!(
                reason,
                server::LoginFailureReason::IncompatibleProtocolVersion,
                "Unexpected reason for LoginFailure"
            );
        }
        _ => panic!("Unexpected response: {response:?}"),
    });

    let _client = TestClient::new_with_login(
        test_app.addr(),
        "client1",
        "token1",
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    )
    .await
    .expect("A client speaking the current protocol version must still log in");
}

#[test(tokio::test)]
async fn session_info_advertises_the_configured_max_conference_size() {
    let config = AppConfig {
        call: CallConfig { max_conf_size: 3 },
        ..TestApp::default_config()
    };
    let test_app = TestApp::new_with_config(config, RateLimiters::default()).await;

    let mut ws_stream = connect_to_websocket(test_app.addr()).await;
    let login = ClientMessage::Login(vacs_protocol::ws::client::Login {
        token: "token1".to_string(),
        protocol_version: VACS_PROTOCOL_VERSION.to_string(),
        custom_profile: false,
        position_id: None,
    });
    ws_stream
        .send(tungstenite::Message::from(
            ClientMessage::serialize(&login).unwrap(),
        ))
        .await
        .expect("Failed to send Login message");

    assert_raw_message_matches(ws_stream.next().await, |response| match response {
        ServerMessage::SessionInfo(server::SessionInfo { max_conf_size, .. }) => {
            assert_eq!(max_conf_size, Some(3));
        }
        _ => panic!("Unexpected response: {response:?}"),
    });
}
