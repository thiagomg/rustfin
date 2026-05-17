use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::api::AppState;

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct StreamQuery {
    #[serde(rename = "Static")]
    pub r#static: Option<String>,
    #[serde(rename = "DeviceId")]
    pub device_id: Option<String>,
    #[serde(rename = "MediaSourceId")]
    pub media_source_id: Option<String>,
    #[serde(rename = "Path")]
    pub path: Option<String>,
    #[serde(rename = "AudioCodec")]
    pub audio_codec: Option<String>,
    #[serde(rename = "Container")]
    pub container: Option<String>,
    #[serde(rename = "StartTimeTicks")]
    pub start_time_ticks: Option<i64>,
}

pub async fn stream_audio(
    State(state): State<Arc<AppState>>,
    Path(item_id): Path<String>,
    Query(_query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    tracing::info!("Stream audio: item={item_id}");

    let track: Option<(String, String, String)> = sqlx::query_as(
        "SELECT t.id, t.file_path, t.mime_type FROM tracks t WHERE t.id = ?",
    )
    .bind(&item_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    let (_track_id, file_path, mime_type) = match track {
        Some(t) => t,
        None => {
            return Err((StatusCode::NOT_FOUND, "Track not found".into()));
        }
    };

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    serve_file(&file_path, &mime_type, range, false).await
}

pub async fn universal_audio(
    State(state): State<Arc<AppState>>,
    Path(item_id): Path<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    tracing::info!("Universal audio: item={item_id}");

    let track: Option<(String, String, String)> = sqlx::query_as(
        "SELECT t.id, t.file_path, t.mime_type FROM tracks t WHERE t.id = ?",
    )
    .bind(&item_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    let (_track_id, file_path, mime_type) = match track {
        Some(t) => t,
        None => {
            return Err((StatusCode::NOT_FOUND, "Track not found".into()));
        }
    };

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let container = query
        .container
        .as_deref()
        .and_then(|c| c.split(',').next())
        .unwrap_or("");

    let response_mime = if !container.is_empty() {
        format!("audio/{container}")
    } else {
        mime_type
    };

    serve_file(&file_path, &response_mime, range, true).await
}

pub async fn hls_playlist(
    State(state): State<Arc<AppState>>,
    Path(item_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let track: Option<(f64,)> = sqlx::query_as(
        "SELECT t.duration FROM tracks t WHERE t.id = ?",
    )
    .bind(&item_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    let (duration,) = match track {
        Some(t) => t,
        None => {
            return Err((StatusCode::NOT_FOUND, "Track not found".into()));
        }
    };

    let duration_secs = duration as u64;
    let playlist = format!(
        "#EXTM3U\n\
         #EXT-X-VERSION:3\n\
         #EXT-X-TARGETDURATION:{duration_secs}\n\
         #EXT-X-MEDIA-SEQUENCE:0\n\
         #EXTINF:{duration:.3},\n\
         stream?Static=true\n\
         #EXT-X-ENDLIST\n"
    );

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .body(Body::from(playlist))
        .unwrap())
}

async fn serve_file(
    file_path: &str,
    mime_type: &str,
    range: Option<String>,
    _allow_transcoding: bool,
) -> Result<Response, (StatusCode, String)> {
    let path = std::path::Path::new(file_path);

    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found".into()));
    }

    let metadata = tokio::fs::metadata(path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("File error: {e}"))
    })?;

    let file_size = metadata.len();

    if let Some(range_header) = range {
        if let Some(range_val) = range_header.strip_prefix("bytes=") {
            let parts: Vec<&str> = range_val.split('-').collect();
            let start = parts
                .first()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let end = parts
                .get(1)
                .and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        s.parse::<u64>().ok()
                    }
                })
                .unwrap_or(file_size - 1);

            if start >= file_size {
                return Err((
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    format!("Range not satisfiable: {start} >= {file_size}"),
                ));
            }

            let content_length = end - start + 1;

            let mut file = File::open(path).await.map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("File error: {e}"))
            })?;

            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Seek error: {e}"))
                })?;

            let stream = ReaderStream::new(file.take(content_length));
            let body = Body::from_stream(stream);

            let response = Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, mime_type)
                .header(header::CONTENT_LENGTH, content_length)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{file_size}"),
                )
                .header(header::ACCEPT_RANGES, "bytes")
                .body(body)
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Response error: {e}"),
                    )
                })?;

            return Ok(response);
        }
    }

    let file = File::open(path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("File error: {e}"))
    })?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CONTENT_LENGTH, file_size)
        .header(header::ACCEPT_RANGES, "bytes")
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Response error: {e}"),
            )
        })?;

    Ok(response)
}
