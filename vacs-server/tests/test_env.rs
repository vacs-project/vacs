use vacs_server::test_utils::{TestClient, TestEnv, test_controller, test_user};
use vatsim_api::types::Facility;

/// Smoke test: full OAuth flow -> WS token -> WebSocket login using TestEnv
/// with a real mock VATSIM server and real auth backend.
#[test_log::test(tokio::test)]
async fn test_env_oauth_login() {
    let env = TestEnv::builder()
        .users(vec![test_user("1234567", "Max", "Mustermann")])
        .build()
        .await;

    // Walk the full OAuth flow and obtain a WS token
    let ws_token = env.ws_token_for("1234567").await.unwrap();
    assert!(!ws_token.is_empty());

    // Use the WS token to connect and log in via WebSocket
    let _client = TestClient::new_with_login(
        env.ws_url(),
        "1234567",
        &ws_token,
        |_, info| {
            assert_eq!(info.id.as_str(), "1234567");
            Ok(())
        },
        |_| Ok(()),
        |_| Ok(()),
    )
    .await
    .expect("WS login should succeed");

    // Verify the server sees us as connected
    assert!(
        env.state()
            .clients
            .is_client_connected(&"1234567".into())
            .await
    );
}

/// Test that an authenticated HTTP client can call protected endpoints.
#[test_log::test(tokio::test)]
async fn test_env_authenticated_http_client() {
    let env = TestEnv::builder()
        .users(vec![test_user("1234567", "Max", "Mustermann")])
        .build()
        .await;

    let client = env.authenticated_http_client("1234567").await.unwrap();

    // The /auth/user endpoint should return the authenticated user's info
    let resp = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["cid"], "1234567");
}

/// Test that the datafeed integration works end-to-end: controllers seeded
/// at build time are visible, and runtime mutations take effect immediately.
#[test_log::test(tokio::test)]
async fn test_env_datafeed_controllers() {
    let env = TestEnv::builder()
        .controllers(vec![test_controller(
            "1234567",
            "LOWW_TWR",
            "119.400",
            Facility::Tower,
        )])
        .build()
        .await;

    // Seeded controller is visible through the real VatsimDataFeed
    let controllers = env.state().get_vatsim_controllers().await.unwrap();
    assert_eq!(controllers.len(), 1);
    assert_eq!(controllers[0].callsign, "LOWW_TWR");
    assert_eq!(controllers[0].frequency, "119.400");

    // Add a second controller at runtime
    env.upsert_controller(test_controller(
        "7654321",
        "LOWW_APP",
        "134.675",
        Facility::Approach,
    ))
    .await;

    let controllers = env.state().get_vatsim_controllers().await.unwrap();
    assert_eq!(controllers.len(), 2);
    assert!(controllers.iter().any(|c| c.callsign == "LOWW_APP"));

    // Remove the first controller
    assert!(env.remove_controller("1234567").await);

    let controllers = env.state().get_vatsim_controllers().await.unwrap();
    assert_eq!(controllers.len(), 1);
    assert_eq!(controllers[0].callsign, "LOWW_APP");
}
