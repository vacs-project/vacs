use crate::auth::extractor::AuthenticatedUser;
use crate::http::ApiResult;
use crate::state::AppState;
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{delete, get};
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/token", get(get::token))
        .route("/", delete(delete::terminate_connection))
}

mod get {
    use super::*;
    use vacs_protocol::http::ws::WebSocketToken;

    pub async fn token(
        auth: AuthenticatedUser,
        State(state): State<Arc<AppState>>,
    ) -> ApiResult<WebSocketToken> {
        tracing::debug!(user = ?auth.user, "Generating websocket token");
        let token = state.generate_ws_auth_token(auth.user.cid.as_str()).await?;

        Ok(Json(WebSocketToken { token }))
    }
}

mod delete {
    use super::*;
    use crate::http::StatusCodeResult;
    use axum::http::StatusCode;
    use vacs_protocol::ws::server::DisconnectReason;

    pub async fn terminate_connection(
        auth: AuthenticatedUser,
        State(state): State<Arc<AppState>>,
    ) -> StatusCodeResult {
        tracing::debug!(user = ?auth.user, "Terminating existing web socket connection");
        state
            .unregister_client(&auth.user.cid, Some(DisconnectReason::Terminated))
            .await;

        Ok(StatusCode::NO_CONTENT)
    }
}
