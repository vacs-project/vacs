pub mod guards;
mod labels;

use crate::metrics::labels::AsMetricLabel;
use crate::release::catalog::BundleType;
use axum_prometheus::metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use axum_prometheus::utils::SECONDS_DURATION_BUCKETS;
use axum_prometheus::{
    AXUM_HTTP_REQUESTS_DURATION_SECONDS, EndpointLabel, PrometheusMetricLayer,
    PrometheusMetricLayerBuilder,
};
use metrics::{
    Unit, counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use semver::Version;
use vacs_protocol::http::version::ReleaseChannel;
use vacs_protocol::profile::{ActiveProfile, ProfileReference};
use vacs_protocol::ws::server::LoginFailureReason;
use vacs_protocol::ws::shared::{CallErrorReason, CallSource, CallTarget};
use vacs_vatsim::FacilityType;

pub fn setup_prometheus_metric_layer() -> (PrometheusMetricLayer<'static>, PrometheusHandle) {
    register_metrics();

    PrometheusMetricLayerBuilder::new()
        .with_ignore_patterns(&["/health", "/favicon.ico"])
        .with_endpoint_label_type(EndpointLabel::MatchedPathWithFallbackFn(|_| {
            "404".to_string()
        }))
        .with_metrics_from_fn(|| {
            PrometheusBuilder::new()
                .set_buckets_for_metric(
                    Matcher::Full(AXUM_HTTP_REQUESTS_DURATION_SECONDS.to_string()),
                    SECONDS_DURATION_BUCKETS,
                )
                .unwrap()
                .set_buckets_for_metric(
                    Matcher::Full("vacs_calls_duration_seconds".to_string()),
                    &[
                        1.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0,
                        300.0,
                    ],
                )
                .unwrap()
                .set_buckets_for_metric(
                    Matcher::Full("vacs_calls_attempts_duration_seconds".to_string()),
                    &[
                        0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 45.0, 60.0, 90.0, 120.0,
                    ],
                )
                .unwrap()
                .set_buckets_for_metric(
                    Matcher::Full("vacs_clients_session_duration_seconds".to_string()),
                    &[
                        30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0, 7200.0, 10800.0, 14400.0, 21600.0,
                    ],
                )
                .unwrap()
                .set_buckets_for_metric(
                    Matcher::Full("vacs_message_size_bytes".to_string()),
                    &[
                        10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0,
                    ],
                )
                .unwrap()
                .set_buckets_for_metric(
                    Matcher::Full("vacs_vatsim_sync_duration_seconds".to_string()),
                    &[0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
                )
                .unwrap()
                .set_buckets_for_metric(
                    Matcher::Prefix("vacs_vatsim_sync_phase_duration_seconds".to_string()),
                    &[0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 5.0],
                )
                .unwrap()
                .install_recorder()
                .unwrap()
        })
        .build_pair()
}

pub fn register_metrics() {
    ClientMetrics::register();
    CallMetrics::register();
    MessageMetrics::register();
    ErrorMetrics::register();
    VersionMetrics::register();
    CoverageMetrics::register();
    ProfileMetrics::register();
    VatsimSyncMetrics::register();
    NetworkDatasetMetrics::register();
}

pub struct ClientMetrics;

impl ClientMetrics {
    pub fn login_attempt(success: bool) {
        let label = if success { "success" } else { "failure" };
        counter!("vacs_clients_login_attempts_total", "status" => label).increment(1);
    }

    pub fn login_failure(reason: LoginFailureReason) {
        let label = reason.as_metric_label();
        counter!("vacs_clients_login_failures_total", "reason" => label).increment(1);
    }

    fn register() {
        describe_gauge!(
            "vacs_clients_connected",
            Unit::Count,
            "Number of currently connected clients"
        );
        describe_counter!(
            "vacs_clients_total",
            Unit::Count,
            "Total number of client connections established"
        );
        describe_counter!(
            "vacs_clients_login_attempts_total",
            Unit::Count,
            "Total login attempts, labeled by success/failure"
        );
        describe_counter!(
            "vacs_clients_login_failures_total",
            Unit::Count,
            "Login failures by reason"
        );
        describe_counter!(
            "vacs_clients_disconnects_total",
            Unit::Count,
            "Client disconnects by reason (graceful vs forced)"
        );
        describe_histogram!(
            "vacs_clients_session_duration_seconds",
            Unit::Seconds,
            "Duration of client sessions in seconds"
        );
    }
}

pub struct CallMetrics;

impl CallMetrics {
    pub fn call_invite(source: &CallSource, target: &CallTarget, prio: bool) {
        counter!(
            "vacs_calls_invites_total",
            "source_type" => source.as_metric_label(),
            "target_type" => target.as_metric_label(),
            "priority" => if prio { "true" } else { "false" }
        )
        .increment(1);
    }

    pub fn call_error(reason: &CallErrorReason) {
        counter!("vacs_calls_errors_total", "reason" => reason.as_metric_label()).increment(1);
    }

    fn register() {
        describe_gauge!(
            "vacs_calls_active",
            Unit::Count,
            "Number of currently active calls"
        );
        describe_counter!(
            "vacs_calls_attempts_total",
            Unit::Count,
            "Total number of calls initiated, labeled by outcome (accepted, error, cancelled, no_answer, aborted)"
        );
        describe_counter!(
            "vacs_calls_total",
            Unit::Count,
            "Total number of calls established"
        );
        describe_histogram!(
            "vacs_calls_duration_seconds",
            Unit::Seconds,
            "Duration of completed calls in seconds"
        );
        describe_histogram!(
            "vacs_calls_attempts_duration_seconds",
            Unit::Seconds,
            "Duration of call attempts in seconds, labeled by outcome (accepted, error, cancelled, no_answer, aborted)"
        );
        describe_counter!(
            "vacs_calls_invites_total",
            Unit::Count,
            "Call invites by source type, target type, and priority"
        );
        describe_counter!(
            "vacs_calls_errors_total",
            Unit::Count,
            "Call errors sent to clients, labeled by error reason"
        );
    }
}

pub struct MessageMetrics;

impl MessageMetrics {
    pub fn sent(message_type: &impl AsMetricLabel, size_bytes: usize) {
        counter!(
            "vacs_messages_total",
            "direction" => "sent",
            "message_type" => message_type.as_metric_label()
        )
        .increment(1);
        histogram!("vacs_message_size_bytes", "direction" => "sent").record(size_bytes as f64);
    }

    pub fn received(message_type: &impl AsMetricLabel, size_bytes: usize) {
        counter!(
            "vacs_messages_total",
            "direction" => "received",
            "message_type" => message_type.as_metric_label()
        )
        .increment(1);
        histogram!("vacs_message_size_bytes", "direction" => "received").record(size_bytes as f64);
    }

    pub fn malformed() {
        counter!("vacs_messages_malformed_total").increment(1);
    }

    fn register() {
        describe_counter!(
            "vacs_messages_total",
            Unit::Count,
            "Total messages, by message type and direction (sent/received)"
        );
        describe_counter!(
            "vacs_messages_malformed_total",
            Unit::Count,
            "Number of malformed messages received"
        );
        describe_histogram!(
            "vacs_message_size_bytes",
            Unit::Bytes,
            "Size of WebSocket messages in bytes, by direction (sent/received)"
        );
    }
}

pub struct ErrorMetrics;

impl ErrorMetrics {
    pub fn error(error_type: &impl AsMetricLabel) {
        counter!("vacs_errors_total", "type" => error_type.as_metric_label()).increment(1);
    }

    pub fn peer_not_found() {
        counter!("vacs_errors_peer_not_found_total").increment(1);
    }

    pub fn rate_limit_exceeded(limit: impl Into<String>) {
        counter!("vacs_errors_rate_limits_exceeded_total", "limit" => limit.into()).increment(1);
    }

    fn register() {
        describe_counter!(
            "vacs_errors_total",
            Unit::Count,
            "Errors encountered by the server, labeled by error type"
        );
        describe_counter!(
            "vacs_errors_peer_not_found_total",
            Unit::Count,
            "Number of times a peer was not found"
        );
        describe_counter!(
            "vacs_errors_rate_limits_exceeded_total",
            Unit::Count,
            "Number of times rate limiting was triggered, labeled by rate limit name"
        );
    }
}

pub struct VersionMetrics;

impl VersionMetrics {
    pub fn check(
        channel: &ReleaseChannel,
        client_version: &Version,
        target: impl Into<String>,
        arch: impl Into<String>,
        bundle_type: &BundleType,
        update_available: bool,
    ) {
        counter!(
            "vacs_version_checks_total",
            "channel" => channel.as_metric_label(),
            "client_version" => client_version.to_string(),
            "target" => target.into(),
            "arch" => arch.into(),
            "bundle_type" => bundle_type.as_metric_label(),
            "result" => if update_available { "update_available" } else { "up_to_date" }
        )
        .increment(1);
    }

    fn register() {
        describe_counter!(
            "vacs_version_checks_total",
            Unit::Count,
            "Version checks labeled by version, channel, platform, architecture, bundle, and update availability"
        );
    }
}

pub struct CoverageMetrics;

impl CoverageMetrics {
    pub fn set_stations_online(count: usize) {
        gauge!("vacs_stations_online").set(count as f64);
    }

    pub fn station_change(change_type: &str) {
        counter!("vacs_stations_changes_total", "type" => change_type.to_string()).increment(1);
    }

    pub fn set_positions_vatsim_only(count: usize) {
        gauge!("vacs_positions_vatsim_only").set(count as f64);
    }

    pub fn set_positions_online(facility_type: &FacilityType, count: usize) {
        gauge!(
            "vacs_positions_online",
            "facility_type" => facility_type.as_str()
        )
        .set(count as f64);
    }

    fn register() {
        describe_gauge!(
            "vacs_stations_online",
            Unit::Count,
            "Number of currently online stations"
        );
        describe_counter!(
            "vacs_stations_changes_total",
            Unit::Count,
            "Total station state changes, labeled by type (online, offline, handoff)"
        );
        describe_gauge!(
            "vacs_positions_vatsim_only",
            Unit::Count,
            "Number of positions online on VATSIM but not using vacs"
        );
        describe_gauge!(
            "vacs_positions_online",
            Unit::Count,
            "Online positions by facility type (GND, TWR, APP, CTR, etc.)"
        );
    }
}

pub struct ProfileMetrics;

impl ProfileMetrics {
    pub fn profile_activated<T: ProfileReference>(profile: &ActiveProfile<T>) {
        let label = match profile {
            ActiveProfile::Specific(_) => "specific",
            ActiveProfile::Custom => "custom",
            ActiveProfile::None => "none",
        };
        counter!("vacs_profiles_activated_total", "type" => label).increment(1);
    }

    fn register() {
        describe_counter!(
            "vacs_profiles_activated_total",
            Unit::Count,
            "Total profile activations, labeled by type (specific, custom, none)"
        );
    }
}

pub struct VatsimSyncMetrics;

impl VatsimSyncMetrics {
    pub fn sync_completed(duration_secs: f64) {
        counter!("vacs_vatsim_sync_total").increment(1);
        histogram!("vacs_vatsim_sync_duration_seconds").record(duration_secs);
    }

    pub fn sync_phase(phase: &'static str, duration_secs: f64) {
        histogram!("vacs_vatsim_sync_phase_duration_seconds", "phase" => phase)
            .record(duration_secs);
    }

    pub fn set_controllers_seen(count: usize) {
        gauge!("vacs_vatsim_controllers_total").set(count as f64);
    }

    pub fn position_match(result: &str) {
        counter!("vacs_vatsim_position_matches_total", "result" => result.to_string()).increment(1);
    }

    fn register() {
        describe_counter!(
            "vacs_vatsim_sync_total",
            Unit::Count,
            "Total number of VATSIM data sync cycles completed"
        );
        describe_histogram!(
            "vacs_vatsim_sync_duration_seconds",
            Unit::Seconds,
            "Duration of VATSIM data sync cycles in seconds"
        );
        describe_histogram!(
            "vacs_vatsim_sync_phase_duration_seconds",
            Unit::Seconds,
            "Duration of individual VATSIM sync phases in seconds, labeled by phase (fetch, sync, unregister)"
        );
        describe_gauge!(
            "vacs_vatsim_controllers_total",
            Unit::Count,
            "Number of VATSIM controllers seen in the last sync cycle"
        );
        describe_counter!(
            "vacs_vatsim_position_matches_total",
            Unit::Count,
            "Position matching outcomes during login, labeled by result (exact, prefix, ambiguous, ambiguous_invalid, ambiguous_resolved, none)"
        );
    }
}

pub struct NetworkDatasetMetrics;

impl NetworkDatasetMetrics {
    pub fn set_dataset_size(positions: usize, stations: usize, profiles: usize) {
        gauge!("vacs_network_positions_total").set(positions as f64);
        gauge!("vacs_network_stations_total").set(stations as f64);
        gauge!("vacs_network_profiles_total").set(profiles as f64);
    }

    fn register() {
        describe_gauge!(
            "vacs_network_positions_total",
            Unit::Count,
            "Total number of positions defined in the loaded network dataset"
        );
        describe_gauge!(
            "vacs_network_stations_total",
            Unit::Count,
            "Total number of stations defined in the loaded network dataset"
        );
        describe_gauge!(
            "vacs_network_profiles_total",
            Unit::Count,
            "Total number of profiles defined in the loaded network dataset"
        );
    }
}
