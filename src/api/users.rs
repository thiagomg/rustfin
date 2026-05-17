use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::api::AppState;

pub async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<serde_json::Value> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, display_name FROM users WHERE id = ?",
    )
    .bind(&user_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    match row {
        Some((id, display_name)) => Json(json!({
            "Id": id,
            "Name": display_name,
            "ServerId": "music-server-1",
            "HasPassword": true,
            "Configuration": {},
            "Policy": {
                "IsAdministrator": true,
                "EnableAllFolders": true,
                "EnableAllDevices": true,
                "EnableAllChannels": true,
                "EnableContentDeletion": false,
                "EnableContentDownloading": true,
                "EnableSync": true,
                "EnableMediaConversion": true,
                "EnabledDevices": [],
                "EnableLiveTvAccess": false,
                "EnableLiveTvManagement": false,
                "EnablePlaybackRemuxing": true,
                "ForceRemoteSourceTranscoding": false,
                "IsAdministrator": true,
            },
            "PrimaryImageTag": null,
        })),
        None => Json(json!({
            "Id": user_id,
            "Name": "Unknown",
            "ServerId": "music-server-1",
        })),
    }
}
