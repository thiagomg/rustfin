use axum::{Json, extract::State};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

use crate::api::AppState;

#[derive(Serialize)]
pub struct PublicSystemInfo {
    pub Id: String,
    pub ServerName: String,
    pub Version: String,
    pub OperatingSystem: String,
    pub ProductName: String,
    pub StartUpWizardCompleted: bool,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub Id: String,
    pub ServerName: String,
    pub Version: String,
    pub OperatingSystem: String,
    pub ProductName: String,
    pub StartUpWizardCompleted: bool,
}

pub async fn public_info() -> Json<serde_json::Value> {
    Json(json!({
        "Id": "music-server-1",
        "ServerName": "Rust Music Server",
        "Version": "0.1.0",
        "OperatingSystem": std::env::consts::OS,
        "ProductName": "Rust Music Server",
        "StartUpWizardCompleted": true,
    }))
}

pub async fn info(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    Json(json!({
        "Id": "music-server-1",
        "ServerName": state.server_name,
        "Version": "0.1.0",
        "OperatingSystem": std::env::consts::OS,
        "ProductName": "Rust Music Server",
        "StartUpWizardCompleted": true,
        "HasPendingRestart": false,
        "IsShuttingDown": false,
        "CanSelfRestart": false,
        "CanSelfUpdate": false,
        "WebSocketPortNumber": 0,
        "CompletedInstallations": [],
        "InternalId": "1",
    }))
}

pub async fn endpoints() -> Json<serde_json::Value> {
    Json(json!([
        "System/Info",
        "System/Info/Public",
        "Users/AuthenticateByName",
        "Users/{userId}/Items",
        "Users/{userId}/Items/{itemId}",
        "Audio/{itemId}/stream",
        "Audio/{itemId}/universal",
    ]))
}

pub async fn report_playback_start(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let item_id = body.get("ItemId").and_then(|v| v.as_str()).unwrap_or("");
    let user_id = body.get("UserId").and_then(|v| v.as_str()).unwrap_or("");
    tracing::info!("Playback start: user={user_id} item={item_id}");

    if !item_id.is_empty() {
        let user_id = body.get("UserId").and_then(|v| v.as_str()).unwrap_or("");
        let position = body
            .get("PositionTicks")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let played_percentage = body
            .get("PlayedPercentage")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        if !user_id.is_empty() {
            sqlx::query(
                "INSERT OR REPLACE INTO user_data (user_id, item_id, played, is_favorite, play_count, playback_position_ticks, last_played_date)
                 VALUES (?, ?, coalesce((SELECT played FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                         coalesce((SELECT is_favorite FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                         coalesce((SELECT play_count FROM user_data WHERE user_id = ? AND item_id = ?), 0) + 1, ?, datetime('now'))"
            )
                .bind(user_id)
                .bind(item_id)
                .bind(user_id).bind(item_id)
                .bind(user_id).bind(item_id)
                .bind(user_id).bind(item_id)
                .bind(position)
                .execute(&state.db)
                .await
                .ok();
        }
    }

    Json(json!({}))
}

pub async fn report_playback_stopped(
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    tracing::debug!("Playback stopped: {body:?}");
    Json(json!({}))
}

pub async fn report_playback_progress(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let item_id = body.get("ItemId").and_then(|v| v.as_str()).unwrap_or("");
    let user_id = body.get("UserId").and_then(|v| v.as_str()).unwrap_or("");
    let position = body
        .get("PositionTicks")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    if !item_id.is_empty() && !user_id.is_empty() {
        sqlx::query(
            "UPDATE user_data SET playback_position_ticks = ? WHERE user_id = ? AND item_id = ?"
        )
            .bind(position)
            .bind(user_id)
            .bind(item_id)
            .execute(&state.db)
            .await
            .ok();
    }

    Json(json!({}))
}
