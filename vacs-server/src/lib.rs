pub mod auth;
pub mod build;
pub mod config;
pub mod dataset;
pub mod health;
pub mod http;
pub mod ice;
pub mod metrics;
pub mod ratelimit;
pub mod release;
pub mod routes;
pub mod state;
pub mod store;
#[cfg(feature = "test-utils")]
pub mod test_utils;
pub mod ws;

/// User-Agent string used for all HTTP requests.
static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
