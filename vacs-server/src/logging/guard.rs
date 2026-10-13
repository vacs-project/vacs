//! A test layer that enforces the field rules and the never-log list of ADR 0003.
//!
//! Violations are collected rather than panicking inside the layer, because the offending event
//! may run on a spawned task whose panic the test would never see. [`assert_clean`] fails the
//! test that observes them.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Once};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const RESERVED_FIELDS: &[&str] = &["timestamp", "level", "target", "name"];
const IP_LINES: &[&str] = &["Client connected", "Request completed"];
const IDENTIFIER_FIELDS: &[&str] = &["conn_id", "client_id", "peer_ids", "call_id"];
const FORBIDDEN_CONTENT: &[&str] = &[
    "v=0\r\n",
    "v=0\\r\\n",
    "a=candidate",
    "candidate:",
    "ws.token.",
    "api.token.",
];

const MAX_FIELD_LEN: usize = 2048;
/// Client crates that share the test process. Their lines go to client log files, not to the
/// server logs ADR 0003 governs, and they name the server's address.
const CLIENT_TARGETS: &[&str] = &["vacs_signaling"];

static VIOLATIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static FULL_IPV4: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(^|[^\d.])\d{1,3}(\.\d{1,3}){3}($|[^\d./])").expect("valid regex")
});
static INSTALLED: AtomicBool = AtomicBool::new(false);
static SECRETS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Installs a global subscriber with the guard layer, once per test binary. Output goes to the
/// test writer, filtered by `RUST_LOG` or [`super::DEFAULT_FILTER`]; the guard sees everything
/// the default filter lets through, whatever `RUST_LOG` says.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let output_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(super::DEFAULT_FILTER));
        let installed = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_test_writer()
                    .with_filter(output_filter),
            )
            .with(LogGuard.with_filter(EnvFilter::new(super::DEFAULT_FILTER)))
            .try_init()
            .is_ok();
        INSTALLED.store(installed, Ordering::SeqCst);
    });
}

/// Registers a value that must never appear in a log line, such as a token a test minted.
pub fn forbid(secret: impl Into<String>) {
    SECRETS.lock().insert(secret.into());
}

/// Fails if any line written so far broke a rule. Violations are global to the test binary, so
/// the first test to check after a violation fails, which may not be the one that caused it.
///
/// Also fails if [`init`] could not install the guard, for example because `test_log` installed
/// its own subscriber first, since every check would then pass vacuously.
pub fn assert_clean() {
    assert!(
        INSTALLED.load(Ordering::SeqCst),
        "the ADR 0003 log guard is not installed; another global subscriber (test_log?) won"
    );
    let violations = std::mem::take(&mut *VIOLATIONS.lock());
    assert!(
        violations.is_empty(),
        "log lines violate ADR 0003:\n{}",
        violations.join("\n")
    );
}

#[derive(Clone, Default)]
struct Fields(Vec<(&'static str, String)>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0.push((field.name(), format!("{value:?}")));
    }
}

/// The layer that checks every span and event against ADR 0003 and records violations for
/// [`assert_clean`].
pub struct LogGuard;

impl LogGuard {
    fn check_fields(
        &self,
        line: Line<'_>,
        fields: &[(&'static str, String)],
        ancestors: &[AncestorFields],
    ) {
        let context = line.context;
        let secrets = SECRETS.lock();
        for (name, value) in fields {
            let reserved = RESERVED_FIELDS.contains(name) || (line.is_span && *name == "message");
            if reserved {
                violation(format!("{context}: reserved field `{name}`"));
            }
            if *name == "client_ip" {
                let truncated = value.ends_with("/24") || value.ends_with("/48");
                if !truncated || !IP_LINES.contains(&line.message) {
                    violation(format!(
                        "{context}: `client_ip` must be truncated and only on {IP_LINES:?}: {value}"
                    ));
                }
            }
            let server_side = !CLIENT_TARGETS.iter().any(|t| line.target.starts_with(t));
            let has_ip = FULL_IPV4.is_match(value) || value.contains("::1");
            if server_side && *name != "client_ip" && has_ip {
                violation(format!(
                    "{context}: `{name}` carries an IP address: {value}"
                ));
            }
            if value.len() > MAX_FIELD_LEN {
                violation(format!(
                    "{context}: `{name}` is {} bytes long, client-chosen values must be bounded",
                    value.len()
                ));
            }
            if *name == "path" && value.contains('?') {
                violation(format!("{context}: `path` carries a query string: {value}"));
            }
            if *name == "peer_ids" {
                let sender = ancestors
                    .iter()
                    .flat_map(|(_, outer)| outer.iter())
                    .chain(fields.iter())
                    .find(|(outer_name, _)| *outer_name == "client_id");
                if let Some((_, sender)) = sender
                    && value.split(',').any(|peer| peer == sender)
                {
                    violation(format!(
                        "{context}: `peer_ids` names the sender {sender}: {value}"
                    ));
                }
            }
            if IDENTIFIER_FIELDS.contains(name) && value.contains(['"', '(', ' ']) {
                violation(format!(
                    "{context}: `{name}` is not Display-formatted: {value}"
                ));
            }
            if let Some(hit) = FORBIDDEN_CONTENT
                .iter()
                .map(|s| s.to_string())
                .chain(secrets.iter().cloned())
                .find(|needle| value.contains(needle.as_str()))
            {
                violation(format!("{context}: `{name}` contains {hit:?}: {value}"));
            }
            for (span_name, outer) in ancestors {
                if outer.iter().any(|(outer_name, _)| outer_name == name) {
                    violation(format!(
                        "{context}: `{name}` shadows the field of enclosing span `{span_name}`"
                    ));
                }
            }
        }
    }
}

struct Line<'a> {
    context: &'a str,
    target: &'a str,
    message: &'a str,
    is_span: bool,
}

type AncestorFields = (&'static str, Vec<(&'static str, String)>);

fn ancestor_fields<'a, S>(
    spans: impl Iterator<Item = tracing_subscriber::registry::SpanRef<'a, S>>,
) -> Vec<AncestorFields>
where
    S: for<'b> LookupSpan<'b> + 'a,
{
    spans
        .map(|span| {
            let names = span
                .extensions()
                .get::<Fields>()
                .map(|fields| fields.0.clone())
                .unwrap_or_default();
            (span.name(), names)
        })
        .collect()
}

fn violation(message: String) {
    VIOLATIONS.lock().push(message);
}

impl<S> Layer<S> for LogGuard
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        let context = format!("span `{}` ({})", span.name(), span.metadata().target());
        let line = Line {
            context: &context,
            target: span.metadata().target(),
            message: "",
            is_span: true,
        };
        self.check_fields(line, &fields.0, &ancestor_fields(span.scope().skip(1)));
        span.extensions_mut().insert(fields);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut recorded = Fields::default();
        values.record(&mut recorded);
        let context = format!("span `{}` ({})", span.name(), span.metadata().target());
        let line = Line {
            context: &context,
            target: span.metadata().target(),
            message: "",
            is_span: true,
        };
        self.check_fields(line, &recorded.0, &ancestor_fields(span.scope().skip(1)));
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<Fields>() {
            for (name, value) in recorded.0 {
                if fields.0.iter().any(|(existing, _)| *existing == name) {
                    violation(format!(
                        "{context}: `{name}` recorded twice on the same span"
                    ));
                }
                fields.0.push((name, value));
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields
            .0
            .iter()
            .find(|(name, _)| *name == "message")
            .map(|(_, value)| value.as_str())
            .unwrap_or_default();
        let context = format!("event {:?} ({})", message, event.metadata().target());
        let ancestors = ctx
            .event_scope(event)
            .map(ancestor_fields)
            .unwrap_or_default();
        let line = Line {
            context: &context,
            target: event.metadata().target(),
            message,
            is_span: false,
        };
        self.check_fields(line, &fields.0, &ancestors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn violations_of(f: impl FnOnce()) -> Vec<String> {
        let subscriber = tracing_subscriber::registry().with(LogGuard);
        tracing::subscriber::with_default(subscriber, f);
        std::mem::take(&mut *VIOLATIONS.lock())
    }

    #[test]
    fn guard_rules() {
        // One test, because the violation list is global to the binary.
        let clean = violations_of(|| {
            let _span = tracing::info_span!("connection", conn_id = "c1").entered();
            tracing::info!(peer_ids = "1,2", "Fine");
        });
        assert_eq!(clean, Vec::<String>::new());

        let shadowed = violations_of(|| {
            let span = tracing::info_span!("connection", client_id = tracing::field::Empty);
            let _span = span.enter();
            span.record("client_id", "1");
            tracing::info!(client_id = "2", "Shadowed");
        });
        assert_eq!(shadowed.len(), 1, "{shadowed:?}");

        let debug_id = violations_of(|| {
            tracing::info!(client_id = ?vacs_protocol::vatsim::ClientId::from("1"), "Debug");
        });
        assert_eq!(debug_id.len(), 1, "{debug_id:?}");

        forbid("secret-token-123");
        let leaked = violations_of(|| {
            tracing::trace!(key = "ws.token.abc", "Store get");
            tracing::trace!(sdp = "v=0\r\ns=-", "Offer");
            tracing::trace!("Token is secret-token-123");
        });
        assert_eq!(leaked.len(), 3, "{leaked:?}");

        let reserved = violations_of(|| {
            let _task = tracing::info_span!("task", name = "sync").entered();
            let _message = tracing::info_span!("call_message", message = "x").entered();
        });
        assert_eq!(reserved.len(), 2, "{reserved:?}");

        let nested_spans = violations_of(|| {
            let _outer = tracing::info_span!("connection", client_id = "1").entered();
            let _declared = tracing::info_span!("inner", client_id = "1").entered();
            let recorded = tracing::info_span!("call_message", call_id = tracing::field::Empty);
            recorded.record("call_id", "c1");
            let _recorded = recorded.entered();
            let _shadow = tracing::info_span!("again", call_id = tracing::field::Empty);
            _shadow.record("call_id", "c2");
        });
        assert_eq!(nested_spans.len(), 2, "{nested_spans:?}");

        let recorded_twice = violations_of(|| {
            let span = tracing::info_span!("call_message", call_id = tracing::field::Empty);
            span.record("call_id", "c1");
            span.record("call_id", "c1");
        });
        assert_eq!(recorded_twice.len(), 1, "{recorded_twice:?}");

        let more_needles = violations_of(|| {
            tracing::trace!(sdp = ?"v=0\r\ns=-", "Offer");
            tracing::trace!(line = "a=candidate:1 1 udp", "Candidate");
            tracing::trace!(candidate = "candidate:1 1 udp 2122260223", "Candidate");
            tracing::trace!(key = "api.token.abc", "Store get");
        });
        assert_eq!(more_needles.len(), 4, "{more_needles:?}");

        let ip = violations_of(|| {
            tracing::info!(client_ip = "203.0.113.0/24", "Client connected");
            tracing::info!(client_ip = "2001:db8:1::/48", "Request completed");
            tracing::info!(client_ip = "203.0.113.77", "Client connected");
            tracing::info!(client_ip = "203.0.113.0/24", "Rate limited");
        });
        assert_eq!(ip.len(), 2, "{ip:?}");

        let ip_elsewhere = violations_of(|| {
            tracing::info!(addr = "192.0.2.10:54400", "Peer address");
            tracing::info!("Connection from 127.0.0.1 failed");
            tracing::info!(addr = "[::1]:8080", "Loopback");
            tracing::info!(version = "3.0.0", call_id = "c1", "No address");
        });
        assert_eq!(ip_elsewhere.len(), 3, "{ip_elsewhere:?}");

        let unbounded = violations_of(|| {
            tracing::info!(
                protocol_version = "9".repeat(4096).as_str(),
                "Login rejected"
            );
        });
        assert_eq!(unbounded.len(), 1, "{unbounded:?}");

        let query = violations_of(|| {
            let _span =
                tracing::info_span!("request", path = "/auth/vatsim/redirect?code=x").entered();
        });
        assert_eq!(query.len(), 1, "{query:?}");

        let sender_as_peer = violations_of(|| {
            let _span = tracing::info_span!("connection", client_id = "1").entered();
            tracing::info!(peer_ids = "2,3", "Fine");
            tracing::info!(peer_ids = "1,2", "Includes the sender");
        });
        assert_eq!(sender_as_peer.len(), 1, "{sender_as_peer:?}");
    }
}
