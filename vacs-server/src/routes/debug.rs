use crate::state::AppState;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/coverage/station/{station_id}", get(get::coverage_station))
        .route("/coverage", get(get::coverage_state))
}

mod get {
    use crate::http::ApiResult;
    use crate::state::AppState;
    use axum::Json;
    use axum::extract::{Path, State};
    use serde::Serialize;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use vacs_protocol::vatsim::{ClientId, PositionId, StationId};

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CoverageStationEntry {
        station_id: StationId,
        controlling_position_id: PositionId,
        /// The VATSIM CIDs covering this position (non-empty when vatsim-only).
        vatsim_controller_ids: BTreeSet<ClientId>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VatsimOnlyEntry {
        position_id: PositionId,
        controller_ids: BTreeSet<ClientId>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CoverageStateResponse {
        online_stations: Vec<CoverageStationEntry>,
        online_positions: BTreeMap<PositionId, BTreeSet<ClientId>>,
        vatsim_only_positions: Vec<VatsimOnlyEntry>,
    }

    pub async fn coverage_station(
        State(state): State<Arc<AppState>>,
        Path(station_id): Path<String>,
    ) -> ApiResult<Option<CoverageStationEntry>> {
        let station_id = StationId::from(station_id);
        let result = state.clients.debug_station_controller(&station_id).await;
        Ok(Json(result.map(|(position_id, vatsim_controller_ids)| {
            CoverageStationEntry {
                station_id,
                controlling_position_id: position_id,
                vatsim_controller_ids: vatsim_controller_ids.into_iter().collect(),
            }
        })))
    }

    pub async fn coverage_state(
        State(state): State<Arc<AppState>>,
    ) -> ApiResult<CoverageStateResponse> {
        let (stations, positions, vatsim_only) = state.clients.debug_state().await;
        Ok(Json(CoverageStateResponse {
            online_stations: stations
                .into_iter()
                .map(|(sid, pid, vcids)| CoverageStationEntry {
                    station_id: sid,
                    controlling_position_id: pid,
                    vatsim_controller_ids: vcids.into_iter().collect(),
                })
                .collect(),
            online_positions: positions
                .into_iter()
                .map(|(pid, cids)| (pid, cids.into_iter().collect()))
                .collect(),
            vatsim_only_positions: vatsim_only
                .into_iter()
                .map(|(pid, cids)| VatsimOnlyEntry {
                    position_id: pid,
                    controller_ids: cids.into_iter().collect(),
                })
                .collect(),
        }))
    }
}
