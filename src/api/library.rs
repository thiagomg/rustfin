use axum::{Json, extract::State};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

use crate::api::AppState;
use crate::library::Library;

pub async fn refresh_library(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let music_path = PathBuf::from(&state.music_dir);
    Library::scan_and_import(&state.db, &music_path).await;

    Json(json!({
        "Status": "OK",
        "Message": "Library scan complete",
    }))
}
