use axum::Router;
use axum::routing::get;
use std::sync::Arc;

use crate::state::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/font.ttf", get(get::font))
}

mod get {
    use axum::http::header;
    use axum::response::{IntoResponse, Response};

    const NOTO_SANS_MONO: &[u8] = include_bytes!(
        "../../../vacs-client/frontend/src/assets/fonts/NotoSansMono-VariableFont_wdth,wght.ttf"
    );

    pub async fn font() -> Response {
        ([(header::CONTENT_TYPE, "font/ttf")], NOTO_SANS_MONO).into_response()
    }
}
