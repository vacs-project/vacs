mod admin;
mod assets;
mod auth;
mod debug;
mod root;
mod version;
mod webrtc;
mod ws;

use crate::logging;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use axum::{Router, extract, middleware};
use axum_client_ip::{ClientIp, ClientIpSource};
use axum_login::{AuthManagerLayer, AuthnBackend};
use axum_prometheus::PrometheusMetricLayer;
use axum_prometheus::metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;
use std::time::Instant;
use tower_http::timeout::TimeoutLayer;
use tower_sessions::SessionStore;
use tower_sessions::service::SignedCookie;
use tracing::{Instrument, info_span};

pub fn create_app<B, S>(
    auth_layer: AuthManagerLayer<B, S, SignedCookie>,
    prom_layer: Option<PrometheusMetricLayer<'static>>,
    client_ip_source: ClientIpSource,
    debug_endpoints: bool,
) -> Router<Arc<AppState>>
where
    B: AuthnBackend + Send + Sync + 'static + Clone,
    S: SessionStore + Send + Sync + 'static + Clone,
{
    let mut app = Router::new()
        .nest("/admin", admin::routes())
        .nest("/assets", assets::routes())
        .nest("/auth", auth::routes())
        .nest("/ws", ws::routes().merge(crate::ws::routes()))
        .nest("/version", version::routes())
        .nest("/webrtc", webrtc::routes())
        .merge(root::routes());

    if debug_endpoints {
        app = app.nest("/debug", debug::routes());
    }
    let app = app
        .merge(root::untraced_routes())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            crate::config::SERVER_SHUTDOWN_TIMEOUT,
        ))
        .layer(auth_layer)
        .layer(middleware::from_fn(request_span))
        .layer(client_ip_source.into_extension());

    if let Some(prom_layer) = prom_layer {
        app.layer(prom_layer)
    } else {
        app
    }
}

/// Runs every request inside an INFO `request` span and writes one completion line, except for
/// health checks and websocket upgrades, whose connection logs its own lines.
async fn request_span(request: extract::Request, next: Next) -> Response {
    let path = request.uri().path();
    if UNLOGGED_PATHS.contains(&path) {
        return next.run(request).await;
    }

    let span = info_span!(
        "request",
        method = %request.method(),
        path = %path,
        client_id = tracing::field::Empty,
    );
    let (mut parts, body) = request.into_parts();
    let client_network = ClientIp::from_request_parts(&mut parts, &())
        .await
        .ok()
        .map(|ClientIp(ip)| logging::client_network(ip));
    parts.extensions.insert(logging::RequestSpan(span.clone()));
    let request = Request::from_parts(parts, body);

    let started = Instant::now();
    let response = next.run(request).instrument(span.clone()).await;
    let status = response.status();
    let duration_ms = started.elapsed().as_millis() as u64;
    let client_ip = client_network.as_deref();
    span.in_scope(|| match status {
        StatusCode::SWITCHING_PROTOCOLS => {}
        StatusCode::GATEWAY_TIMEOUT => tracing::warn!(
            client_ip,
            status = status.as_u16(),
            duration_ms,
            "Request completed"
        ),
        _ => tracing::info!(
            client_ip,
            status = status.as_u16(),
            duration_ms,
            "Request completed"
        ),
    });
    response
}

const UNLOGGED_PATHS: &[&str] = &["/health", "/favicon.ico"];

pub fn create_metrics_app(prom_handle: PrometheusHandle) -> Router {
    Router::new().route("/metrics", get(|| async move { prom_handle.render() }))
}
