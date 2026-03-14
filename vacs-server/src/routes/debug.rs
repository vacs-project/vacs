use crate::state::AppState;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/coverage/station/{station_id}", get(get::coverage_station))
        .route("/coverage", get(get::coverage_snapshot))
}

mod get {
    use crate::http::ApiResult;
    use crate::state::AppState;
    use crate::state::clients::{CoverageSnapshot, StationCoverage};
    use axum::Json;
    use axum::extract::{Path, State};
    use std::sync::Arc;
    use vacs_protocol::vatsim::StationId;

    pub async fn coverage_station(
        State(state): State<Arc<AppState>>,
        Path(station_id): Path<String>,
    ) -> ApiResult<Option<StationCoverage>> {
        let station_id = StationId::from(station_id);
        let result = state.clients.station_coverage(&station_id).await;
        Ok(Json(result))
    }

    pub async fn coverage_snapshot(
        State(state): State<Arc<AppState>>,
    ) -> ApiResult<CoverageSnapshot> {
        let snapshot = state.clients.coverage_snapshot().await;
        Ok(Json(snapshot))
    }
}
