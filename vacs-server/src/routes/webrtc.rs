use crate::state::AppState;
use axum::Router;
use axum::routing::get;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/ice-config", get(get::ice_config))
}

mod get {
    use super::*;
    use crate::auth::extractor::AuthenticatedUser;
    use crate::http::ApiResult;
    use axum::Json;
    use axum::extract::State;
    use vacs_protocol::http::webrtc::IceConfig;

    pub async fn ice_config(
        auth: AuthenticatedUser,
        State(state): State<Arc<AppState>>,
    ) -> ApiResult<IceConfig> {
        tracing::debug!(user = ?auth.user, "Retrieving ICE config for user");
        let config = state
            .ice_config_provider
            .get_ice_config(&auth.user.cid)
            .await?;

        Ok(Json(config))
    }
}
