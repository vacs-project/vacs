use crate::metrics::labels::AsMetricLabel;
use metrics::{counter, gauge, histogram};
use std::time::Instant;
use vacs_protocol::ws::server::DisconnectReason;
use vacs_protocol::ws::shared::CallErrorReason;

#[derive(Debug)]
pub struct ClientConnectionGuard {
    start_time: Instant,
    disconnect_reason: Option<DisconnectReason>,
}

impl ClientConnectionGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_disconnect_reason(&mut self, reason: DisconnectReason) {
        self.disconnect_reason = Some(reason);
    }
}

impl Default for ClientConnectionGuard {
    fn default() -> Self {
        gauge!("vacs_clients_connected").increment(1.0);
        counter!("vacs_clients_total").increment(1);

        Self {
            start_time: Instant::now(),
            disconnect_reason: None,
        }
    }
}

impl Drop for ClientConnectionGuard {
    fn drop(&mut self) {
        gauge!("vacs_clients_connected").decrement(1.0);

        histogram!("vacs_clients_session_duration_seconds")
            .record(self.start_time.elapsed().as_secs_f64());

        counter!(
            "vacs_clients_disconnects_total",
            "reason" => self.disconnect_reason.as_metric_label(),
        )
        .increment(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallAttemptOutcome {
    Accepted,
    Rejected,
    Error(CallErrorReason),
    Cancelled,
    Aborted,
}

#[derive(Debug)]
pub struct CallAttemptGuard {
    start_time: Instant,
    outcome: Option<CallAttemptOutcome>,
}

impl CallAttemptGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_outcome(&mut self, outcome: CallAttemptOutcome) {
        self.outcome = Some(outcome);
    }
}

impl Default for CallAttemptGuard {
    fn default() -> Self {
        Self {
            start_time: Instant::now(),
            outcome: None,
        }
    }
}

impl Drop for CallAttemptGuard {
    fn drop(&mut self) {
        counter!(
            "vacs_calls_attempts_total",
            "result" => self.outcome.as_metric_label(),
        )
        .increment(1);

        histogram!(
            "vacs_calls_attempts_duration_seconds",
            "result" => self.outcome.as_metric_label()
        )
        .record(self.start_time.elapsed().as_secs_f64());
    }
}

#[derive(Debug)]
pub struct CallGuard {
    start_time: Instant,
}

impl CallGuard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for CallGuard {
    fn default() -> Self {
        gauge!("vacs_calls_active").increment(1.0);
        counter!("vacs_calls_total").increment(1);

        Self {
            start_time: Instant::now(),
        }
    }
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        gauge!("vacs_calls_active").decrement(1.0);
        histogram!("vacs_calls_duration_seconds").record(self.start_time.elapsed().as_secs_f64());
    }
}
