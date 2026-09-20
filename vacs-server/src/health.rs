use crate::config::AppConfig;
use anyhow::Context;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);

/// Probes the running server's `/health` route for the container health check, resolving the
/// port from the same configuration the server started with.
pub async fn probe() -> anyhow::Result<()> {
    let config = AppConfig::load().context("loading server config")?;
    probe_addr(&config.server.bind_addr).await
}

/// Requests `/health` from the server listening on `bind_addr`, over loopback when the server
/// binds the unspecified address, and fails unless it answers with a success status.
pub async fn probe_addr(bind_addr: &str) -> anyhow::Result<()> {
    let bind: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("invalid bind address {bind_addr}"))?;
    let ip = match bind.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    let url = format!("http://{}/health", SocketAddr::new(ip, bind.port()));

    let response = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        anyhow::bail!("{url} answered {status}")
    }
}

#[cfg(test)]
mod tests {
    use super::probe_addr;
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;
    use std::net::SocketAddr;

    async fn serve(status: StatusCode) -> SocketAddr {
        let app = Router::new().route("/health", get(move || async move { status }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[tokio::test]
    async fn healthy_server_passes_through_the_unspecified_bind_address() {
        let addr = serve(StatusCode::OK).await;
        probe_addr(&format!("0.0.0.0:{}", addr.port()))
            .await
            .unwrap();
        probe_addr(&addr.to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn unavailable_server_fails() {
        let addr = serve(StatusCode::SERVICE_UNAVAILABLE).await;
        let err = probe_addr(&addr.to_string()).await.unwrap_err();
        assert!(err.to_string().contains("503"), "{err}");
    }

    #[tokio::test]
    async fn nothing_listening_fails() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        assert!(probe_addr(&addr.to_string()).await.is_err());
    }

    #[tokio::test]
    async fn invalid_bind_address_fails() {
        assert!(probe_addr("not an address").await.is_err());
    }
}
