use reqwest::StatusCode;
use test_log::test;
use vacs_protocol::http::auth::UserInfo;
use vacs_protocol::http::ws::WebSocketToken;
use vacs_server::store::memory::MemoryStore;
use vacs_server::test_utils::TestEnv;

#[test(tokio::test)]
async fn user_info_without_auth() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test(tokio::test)]
async fn ws_token_without_auth() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/ws/token", env.http_base_url()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test(tokio::test)]
async fn user_info_with_valid_api_token() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(0)),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let info: UserInfo = resp.json().await.unwrap();
    assert_eq!(info.cid.as_str(), "cid0");
}

#[test(tokio::test)]
async fn user_info_with_invalid_token() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .header("Authorization", "Bearer invalid-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test(tokio::test)]
async fn ws_token_with_valid_api_token() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/ws/token", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(1)),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ws_token: WebSocketToken = resp.json().await.unwrap();
    assert!(!ws_token.token.is_empty());
}

#[test(tokio::test)]
async fn ws_token_with_invalid_token() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/ws/token", env.http_base_url()))
        .header("Authorization", "Bearer invalid-token")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test(tokio::test)]
async fn logout_revokes_api_token() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/auth/logout", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(2)),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(2)),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test(tokio::test)]
async fn different_preseeded_tokens_return_different_cids() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp0 = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(0)),
        )
        .send()
        .await
        .unwrap();
    let info0: UserInfo = resp0.json().await.unwrap();

    let resp3 = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(3)),
        )
        .send()
        .await
        .unwrap();
    let info3: UserInfo = resp3.json().await.unwrap();

    assert_eq!(info0.cid.as_str(), "cid0");
    assert_eq!(info3.cid.as_str(), "cid3");
}

#[test(tokio::test)]
async fn revoking_one_token_does_not_affect_others() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/auth/logout", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(4)),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = client
        .get(format!("{}/auth/user", env.http_base_url()))
        .header(
            "Authorization",
            format!("Bearer {}", MemoryStore::test_api_token(5)),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let info: UserInfo = resp.json().await.unwrap();
    assert_eq!(info.cid.as_str(), "cid5");
}
