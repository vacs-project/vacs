use reqwest::StatusCode;
use test_log::test;
use vacs_server::test_utils::TestEnv;

/// The `reload-releases` action polls this endpoint with hand-written query keys and this
/// bundle list; an empty catalog answers a well-formed request with 204.
#[test(tokio::test)]
async fn update_check_accepts_the_reload_action_queries() {
    let env = TestEnv::builder().build().await;
    let client = reqwest::Client::new();
    let url = format!("{}/version/update", env.http_base_url());

    let bundles = [
        ("windows", "x86_64", "nsis"),
        ("linux", "x86_64", "deb"),
        ("linux", "x86_64", "rpm"),
        ("linux", "x86_64", "appimage"),
        ("darwin", "x86_64", "app"),
        ("darwin", "aarch64", "app"),
    ];
    let queries = bundles
        .iter()
        .map(|&(target, arch, bundle_type)| (target, arch, bundle_type, "stable"))
        .chain([
            ("windows", "x86_64", "nsis", "beta"),
            ("windows", "x86_64", "nsis", "rc"),
        ]);

    for (target, arch, bundle_type, channel) in queries {
        let resp = client
            .get(&url)
            .query(&[
                ("version", "0.0.1"),
                ("target", target),
                ("arch", arch),
                ("bundle_type", bundle_type),
                ("channel", channel),
            ])
            .send()
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "{target}/{arch}/{bundle_type} on {channel}"
        );
    }
}
