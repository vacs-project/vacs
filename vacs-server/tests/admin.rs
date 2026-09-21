use reqwest::StatusCode;
use test_log::test;
use vacs_server::config::AdminConfig;
use vacs_server::test_utils::TestEnv;

/// Unconfigured deployments answer 404, so nobody can reload their catalog.
#[test(tokio::test)]
async fn releases_reload_without_allowed_subject() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/admin/releases/reload", env.http_base_url()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// The dataset subject must never open the release endpoint.
#[test(tokio::test)]
async fn releases_reload_does_not_fall_back_to_the_dataset_subject() {
    let env = TestEnv::builder()
        .admin(AdminConfig {
            oidc_allowed_sub: "repo:vacs-project/vacs-data:environment:production".to_string(),
            oidc_allowed_sub_releases: None,
            ..Default::default()
        })
        .build()
        .await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/admin/releases/reload", env.http_base_url()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[test(tokio::test)]
async fn releases_reload_without_token() {
    let env = TestEnv::builder()
        .admin(AdminConfig {
            oidc_allowed_sub_releases: Some(
                "repo:vacs-project/vacs:environment:production".to_string(),
            ),
            ..Default::default()
        })
        .build()
        .await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/admin/releases/reload", env.http_base_url()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test(tokio::test)]
async fn releases_reload_with_malformed_token() {
    let env = TestEnv::builder()
        .admin(AdminConfig {
            oidc_allowed_sub_releases: Some(
                "repo:vacs-project/vacs:environment:production".to_string(),
            ),
            ..Default::default()
        })
        .build()
        .await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/admin/releases/reload", env.http_base_url()))
        .bearer_auth("not-a-jwt")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
