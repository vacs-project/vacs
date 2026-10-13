//! Log setup and the field conventions of ADR 0003 (`docs/decisions/0003-server-logging.md`).

#[cfg(any(test, feature = "test-utils"))]
pub mod guard;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use tracing::Span;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use vacs_protocol::vatsim::ClientId;

/// Filter used when `RUST_LOG` is not set. `RUST_LOG` replaces it entirely, so an override
/// should keep its `info` base or the entry-point spans and their fields disappear. It must also
/// keep `tungstenite`, `tokio_tungstenite`, `hyper`, `hyper_util`, `reqwest` and `h2` at INFO or
/// above: below that they log websocket frames and request URLs, tokens included (ADR 0003).
pub const DEFAULT_FILTER: &str = "info,vacs_=trace,vacs_vatsim::coverage=debug,axum::rejection=trace,axum_login=warn,tower_sessions=warn";

/// Installs the global subscriber: JSON lines when `RUST_LOG_JSON` is set, text otherwise.
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let fmt_layer = if std::env::var_os("RUST_LOG_JSON").is_some() {
        json_layer().boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}

/// The production JSON formatter: event and span fields flattened to the top level, no span
/// list. Fields of spans closer to the event win over outer ones, which is why the guard layer
/// rejects shadowed fields instead of relying on that.
pub fn json_layer<S>() -> json_subscriber::fmt::Layer<S>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    json_subscriber::fmt::layer()
        .flatten_event(true)
        .flatten_span_list_on_top_level(true)
        .with_current_span(false)
        .with_span_list(false)
}

/// The `request` span of the current HTTP request. Handlers run inside spans of the auth layer,
/// so recording on `Span::current()` would miss it.
#[derive(Clone)]
pub struct RequestSpan(pub Span);

impl RequestSpan {
    /// Records the client the request authenticated as. Call it once authentication succeeds.
    pub fn record_client_id(&self, client_id: &ClientId) {
        self.0
            .record("client_id", tracing::field::display(client_id));
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestSpan {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(parts
            .extensions
            .get::<RequestSpan>()
            .cloned()
            .unwrap_or(RequestSpan(Span::none())))
    }
}

/// The network a client address belongs to, as logged: the /24 of an IPv4 address and the /48
/// of an IPv6 address. IPv4-mapped and 6to4 addresses are reduced to their IPv4 address first.
pub fn client_network(ip: IpAddr) -> String {
    match embedded_ipv4(ip) {
        Some(v4) => {
            let [a, b, c, _] = v4.octets();
            format!("{}/24", Ipv4Addr::new(a, b, c, 0))
        }
        None => {
            let IpAddr::V6(v6) = ip else {
                unreachable!("every IPv4 address has an embedded IPv4 address")
            };
            let [a, b, c, ..] = v6.segments();
            format!("{}/48", Ipv6Addr::new(a, b, c, 0, 0, 0, 0, 0))
        }
    }
}

fn embedded_ipv4(ip: IpAddr) -> Option<Ipv4Addr> {
    match ip {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return Some(v4);
            }
            let segments = v6.segments();
            (segments[0] == 0x2002).then(|| {
                let [hi, lo] = [segments[1], segments[2]];
                Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8)
            })
        }
    }
}

const MAX_CLIENT_VALUE_LEN: usize = 64;

/// A client-chosen string as logged: cut to 64 bytes on a character boundary, so a client cannot
/// write arbitrarily long lines.
pub fn bounded(value: &str) -> &str {
    if value.len() <= MAX_CLIENT_VALUE_LEN {
        return value;
    }
    let end = (0..=MAX_CLIENT_VALUE_LEN)
        .rev()
        .find(|&i| value.is_char_boundary(i))
        .unwrap_or(0);
    &value[..end]
}

/// The `peer_ids` field value: the given CIDs sorted, deduplicated and comma-separated.
pub fn peer_ids<'a>(ids: impl IntoIterator<Item = &'a ClientId>) -> String {
    let mut ids: Vec<&str> = ids.into_iter().map(ClientId::as_str).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn json_lines_are_flat() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry()
            .with(json_layer().without_time().with_writer(buffer.clone()));

        tracing::subscriber::with_default(subscriber, || {
            let connection = tracing::info_span!(
                "connection",
                conn_id = "01928c5e-8d2a-7f7e-9c41-2e6a7d9b0f13",
                client_id = tracing::field::Empty
            );
            let _connection = connection.enter();
            connection.record("client_id", "1255177");
            let _call =
                tracing::info_span!("call_message", call_id = "c1", kind = "call_end").entered();
            tracing::info!(
                reason = "pong_timeout",
                duration_ms = 5412318u64,
                text = "quote \" newline \n control \u{1}",
                "Client disconnected"
            );
        });

        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let line: serde_json::Value = serde_json::from_str(output.trim_end()).unwrap();
        assert_eq!(
            line,
            serde_json::json!({
                "level": "INFO",
                "target": "vacs_server::logging::tests",
                "name": "call_message",
                "message": "Client disconnected",
                "reason": "pong_timeout",
                "duration_ms": 5412318,
                "text": "quote \" newline \n control \u{1}",
                "conn_id": "01928c5e-8d2a-7f7e-9c41-2e6a7d9b0f13",
                "client_id": "1255177",
                "call_id": "c1",
                "kind": "call_end",
            })
        );
        assert_eq!(output.lines().count(), 1);
    }

    #[test]
    fn default_filter_keeps_our_detail_and_drops_dependency_noise() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_writer(buffer.clone())
                .with_filter(EnvFilter::new(DEFAULT_FILTER)),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::trace!(target: "vacs_server::ws", "kept: server trace");
            tracing::trace!(target: "vacs_vatsim::slurper", "kept: vatsim trace");
            tracing::trace!(target: "vacs_vatsim::coverage::network", "dropped: coverage trace");
            tracing::debug!(target: "vacs_vatsim::coverage::network", "kept: coverage debug");
            tracing::debug!(target: "hyper::client", "dropped: dependency debug");
            tracing::info!(target: "fred::router", "kept: dependency info");
            tracing::info!(target: "axum_login::service", "dropped: axum-login info");
            tracing::info!(target: "tower_sessions_core::session", "dropped: tower-sessions info");
            tracing::trace!(target: "axum::rejection", "kept: rejection trace");
        });

        let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output.matches("kept:").count(), 5, "{output}");
        assert!(!output.contains("dropped:"), "{output}");
    }

    #[test]
    fn client_network_truncates_ipv4_to_24() {
        assert_eq!(
            client_network("203.0.113.77".parse().unwrap()),
            "203.0.113.0/24"
        );
    }

    #[test]
    fn client_network_truncates_ipv6_to_48() {
        assert_eq!(
            client_network("2001:db8:1234:5678::1".parse().unwrap()),
            "2001:db8:1234::/48"
        );
    }

    #[test]
    fn client_network_reduces_ipv4_mapped_addresses() {
        assert_eq!(
            client_network("::ffff:198.51.100.9".parse().unwrap()),
            "198.51.100.0/24"
        );
    }

    #[test]
    fn client_network_reduces_6to4_addresses() {
        assert_eq!(
            client_network("2002:c633:6409::1".parse().unwrap()),
            "198.51.100.0/24"
        );
    }

    #[test]
    fn bounded_cuts_long_values_on_a_character_boundary() {
        assert_eq!(bounded("3.0.0"), "3.0.0");
        assert_eq!(bounded(&"a".repeat(100)).len(), 64);
        let multibyte = format!("{}\u{e9}", "a".repeat(63));
        assert_eq!(bounded(&multibyte), "a".repeat(63));
    }

    #[test]
    fn peer_ids_are_sorted_and_deduplicated() {
        let ids = [
            ClientId::from("1300000"),
            ClientId::from("1200000"),
            ClientId::from("1300000"),
        ];
        assert_eq!(peer_ids(&ids), "1200000,1300000");
        assert_eq!(peer_ids(&ids[..1]), "1300000");
    }
}
