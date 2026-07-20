use std::path::PathBuf;
use vacs_server::test_utils::{TestClient, TestEnv, test_controller, test_user};
use vacs_vatsim::coverage::network::Network;
use vatsim_api::types::Facility;

fn lo_network() -> Network {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scenarios/datasets/full");
    Network::load_from_dir(&dir).expect("Failed to load LO test dataset")
}

/// With required active connections, the WebSocket login resolves the
/// client's position from its live VATSIM connection via the slurper.
#[test_log::test(tokio::test)]
async fn login_resolves_position_from_slurper() {
    let env = TestEnv::builder()
        .users(vec![test_user("1234567", "Max", "Mustermann")])
        .controllers(vec![test_controller(
            "1234567",
            "LOVV_E_CTR",
            "134.440",
            Facility::Enroute,
        )])
        .network(lo_network())
        .require_active_connection(true)
        .build()
        .await;

    let token = env.ws_token_for("1234567").await.unwrap();
    let _client = TestClient::new_with_login(
        env.ws_url(),
        "1234567",
        &token,
        |_, info| {
            assert_eq!(
                info.position_id.as_ref().map(|p| p.as_str()),
                Some("LOVV_E_CTR"),
                "Position should be resolved from the slurper connection"
            );
            assert_eq!(info.display_name, "LOVV_E_CTR");
            assert_eq!(info.frequency, "134.440");
            Ok(())
        },
        |_| Ok(()),
        |_| Ok(()),
    )
    .await
    .expect("WS login should succeed and resolve the position");
}

/// With required active connections, a user without a live VATSIM
/// connection is denied the WebSocket login.
#[test_log::test(tokio::test)]
async fn login_fails_without_active_connection() {
    let env = TestEnv::builder()
        .users(vec![test_user("1234567", "Max", "Mustermann")])
        .network(lo_network())
        .require_active_connection(true)
        .build()
        .await;

    let token = env.ws_token_for("1234567").await.unwrap();
    let result = TestClient::new_with_login(
        env.ws_url(),
        "1234567",
        &token,
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    )
    .await;

    let err = match result {
        Ok(_) => panic!("WS login should fail without an active VATSIM connection"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("NoActiveVatsimConnection"),
        "Unexpected login failure: {err}"
    );
}
