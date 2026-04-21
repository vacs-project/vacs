mod admin;
mod assets;
mod auth;
mod debug;
mod root;
mod version;
mod webrtc;
mod ws;

use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::routing::get;
use axum::{Router, extract, middleware};
use axum_client_ip::{ClientIp, ClientIpSource};
use axum_login::{AuthManagerLayer, AuthnBackend};
use axum_prometheus::PrometheusMetricLayer;
use axum_prometheus::metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::SessionStore;
use tower_sessions::service::SignedCookie;
use tracing::{Span, debug_span};

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
        .layer(middleware::from_fn(
            async |request: extract::Request, next: Next| {
                let (mut parts, body) = request.into_parts();
                if let Ok(ip) = ClientIp::from_request_parts(&mut parts, &()).await {
                    Span::current().record("client_ip", ip.0.to_string());
                }
                next.run(Request::from_parts(parts, body)).await
            },
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(move |req: &Request<_>| {
                let path = req.uri().path();
                match path {
                    "/health" | "/favicon.ico" => Span::none(),
                    _ => {
                        let uri = if path == "/auth/vatsim/redirect" {
                            path.to_owned()
                        } else {
                            req.uri().to_string()
                        };
                        debug_span!(
                            "request",
                            method = %req.method(),
                            uri = uri.as_str(),
                            version = ?req.version(),
                            client_ip = tracing::field::Empty)
                    }
                }
            }),
        )
        .merge(root::untraced_routes())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            crate::config::SERVER_SHUTDOWN_TIMEOUT,
        ))
        .layer(auth_layer)
        .layer(client_ip_source.into_extension());

    if let Some(prom_layer) = prom_layer {
        app.layer(prom_layer)
    } else {
        app
    }
}

pub fn create_metrics_app(prom_handle: PrometheusHandle) -> Router {
    Router::new().route("/metrics", get(|| async move { prom_handle.render() }))
}
