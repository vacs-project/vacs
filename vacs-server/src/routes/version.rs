use crate::state::AppState;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/update", get(get::update))
}

mod get {
    use crate::http::error::{AppError, ProblemDetails};
    use crate::http::{ApiMaybe, MaybeJsonOrProblem};
    use crate::metrics::VersionMetrics;
    use crate::release::catalog::BundleType;
    use crate::state::AppState;
    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum_client_ip::ClientIp;
    use semver::Version;
    use serde::Deserialize;
    use std::sync::Arc;
    use vacs_protocol::http::version::{Release, ReleaseChannel};

    #[derive(Debug, Deserialize)]
    pub struct VersionUpdateParams {
        version: String,
        target: String,
        arch: String,
        bundle_type: BundleType,
        channel: Option<ReleaseChannel>,
    }

    pub async fn update(
        Query(params): Query<VersionUpdateParams>,
        State(state): State<Arc<AppState>>,
        ClientIp(client_ip): ClientIp,
    ) -> ApiMaybe<Release> {
        if let Err(until) = state.rate_limiters().check_version_update(client_ip) {
            return Err(AppError::TooManyRequests(until.as_secs()));
        }

        let client_ver = match Version::parse(&params.version) {
            Ok(v) => v,
            Err(err) => {
                tracing::debug!(%err, "Failed to parse client version");
                return Ok(MaybeJsonOrProblem::problem(
                    ProblemDetails::from(StatusCode::BAD_REQUEST)
                        .with_title("Invalid version format")
                        .with_detail(format!("'{}' is not valid SemVer", params.version).as_str()),
                ));
            }
        };

        let channel = params.channel.unwrap_or_default();

        match state
            .updates
            .check(
                &channel,
                &client_ver,
                &params.target,
                &params.arch,
                &params.bundle_type,
            )
            .await
        {
            Ok(Some(rel)) => {
                VersionMetrics::check(
                    &channel,
                    &client_ver,
                    &params.target,
                    &params.arch,
                    &params.bundle_type,
                    true,
                );
                Ok(MaybeJsonOrProblem::ok(rel))
            }
            Ok(None) => {
                VersionMetrics::check(
                    &channel,
                    &client_ver,
                    &params.target,
                    &params.arch,
                    &params.bundle_type,
                    false,
                );
                Ok(MaybeJsonOrProblem::no_content())
            }
            Err(err) => Err(err),
        }
    }
}
