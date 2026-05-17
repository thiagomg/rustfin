use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::api::AppState;

#[derive(Deserialize, Default)]
#[allow(dead_code)]
pub struct ItemsQuery {
    #[serde(rename = "ParentId")]
    pub parent_id: Option<String>,
    #[serde(rename = "IncludeItemTypes")]
    pub include_item_types: Option<String>,
    #[serde(rename = "Recursive")]
    pub recursive: Option<String>,
    #[serde(rename = "StartIndex")]
    pub start_index: Option<i32>,
    #[serde(rename = "Limit")]
    pub limit: Option<i32>,
    #[serde(rename = "SortBy")]
    pub sort_by: Option<String>,
    #[serde(rename = "SortOrder")]
    pub sort_order: Option<String>,
    #[serde(rename = "Fields")]
    pub fields: Option<String>,
    #[serde(rename = "Filters")]
    pub filters: Option<String>,
    #[serde(rename = "SearchTerm")]
    pub search_term: Option<String>,
    #[serde(rename = "IsFavorite")]
    pub is_favorite: Option<String>,
    #[serde(rename = "MediaTypes")]
    pub media_types: Option<String>,
    #[serde(rename = "UserId")]
    pub user_id: Option<String>,
    #[serde(rename = "Ids")]
    pub ids: Option<String>,
    #[serde(rename = "ArtistIds")]
    pub artist_ids: Option<String>,
    #[serde(rename = "AlbumId")]
    pub album_id: Option<String>,
    #[serde(rename = "AlbumIds")]
    pub album_ids: Option<String>,
}

const TICKS_PER_SECOND: i64 = 10_000_000;

pub async fn get_views(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    tracing::info!("Get views");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    Json(json!({
        "Items": [{
            "Name": "Music",
            "Id": "music-library",
            "Type": "CollectionFolder",
            "CollectionType": "music",
            "ImageTags": {},
            "BackdropImageTags": [],
            "LocationType": "FileSystem",
            "ChildCount": count,
        }],
        "TotalRecordCount": 1,
        "StartIndex": 0,
    }))
}

pub async fn get_items(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let item_types = query
        .include_item_types
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    let parent_id = query.parent_id.as_deref().unwrap_or("");
    let artist_ids = query.artist_ids.as_deref().unwrap_or("");
    let album_id = query.album_id.as_deref().unwrap_or("");
    let album_ids = query.album_ids.as_deref().unwrap_or("");
    let start_index = query.start_index.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).max(1).min(200);

    tracing::info!("GetItems: types={item_types:?} parent={parent_id} artist_ids={artist_ids} album_id={album_id} album_ids={album_ids} offset={start_index} limit={limit}");

    // If AlbumId or AlbumIds is specified, list tracks for that album
    let target_album_id = if !album_id.is_empty() {
        album_id
    } else if !album_ids.is_empty() {
        album_ids.split(',').next().unwrap_or("").trim()
    } else {
        ""
    };

    if !target_album_id.is_empty() {
        return list_tracks_for_album(&state, target_album_id, start_index, limit).await;
    }

    // If ArtistIds is specified, list albums for that artist
    if !artist_ids.is_empty() {
        let first_artist_id = artist_ids.split(',').next().unwrap_or("").trim();
        if !first_artist_id.is_empty() {
            let artist_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM artists WHERE id = ?",
            )
            .bind(first_artist_id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

            tracing::info!("ArtistIds lookup: artist_id={first_artist_id} exists={artist_count}");

            if artist_count > 0 {
                return list_albums_for_artist(&state, first_artist_id, start_index, limit).await;
            }
        }
    }

    // If parent_id is specified and valid (not the magic "music-library"), resolve it first
    if !parent_id.is_empty() && parent_id != "music-library" && parent_id != "null" {
        let artist_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM artists WHERE id = ?",
        )
        .bind(parent_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

        if artist_count > 0 {
            return list_albums_for_artist(&state, parent_id, start_index, limit).await;
        }

        let album_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM albums WHERE id = ?",
        )
        .bind(parent_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

        if album_count > 0 {
            return list_tracks_for_album(&state, parent_id, start_index, limit).await;
        }
    }

    // No parent_id or parent not found — use IncludeItemTypes to decide
    if parent_id == "music-library" || parent_id.is_empty() && item_types.is_empty() {
        return list_items_basic(&state, "MusicAlbum", start_index, limit).await;
    }

    let first_type = item_types.first().copied().unwrap_or("");

    match first_type {
        "MusicArtist" => list_items_basic(&state, "MusicArtist", start_index, limit).await,
        "MusicAlbum" => list_items_basic(&state, "MusicAlbum", start_index, limit).await,
        "Audio" => list_items_basic(&state, "Audio", start_index, limit).await,
        _ => list_items_basic(&state, "MusicAlbum", start_index, limit).await,
    }
}

async fn list_items_basic(
    state: &Arc<AppState>,
    item_type: &str,
    start_index: i32,
    limit: i32,
) -> Json<serde_json::Value> {
    match item_type {
        "MusicArtist" => list_artists(state, start_index, limit).await,
        "MusicAlbum" => list_albums(state, start_index, limit).await,
        "Audio" => list_tracks(state, start_index, limit).await,
        _ => list_albums(state, start_index, limit).await,
    }
}

async fn list_artists(
    state: &Arc<AppState>,
    start_index: i32,
    limit: i32,
) -> Json<serde_json::Value> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artists")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT a.id, a.name, COALESCE(alb.cnt, 0) FROM artists a LEFT JOIN (SELECT artist_id, COUNT(*) as cnt FROM albums GROUP BY artist_id) alb ON alb.artist_id = a.id ORDER BY a.sort_name ASC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(start_index)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, album_count)| json_artist_item(&id, &name, album_count))
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    }))
}

async fn list_albums(
    state: &Arc<AppState>,
    start_index: i32,
    limit: i32,
) -> Json<serde_json::Value> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let rows: Vec<(String, String, String, String, Option<i32>, Option<String>, i64)> = sqlx::query_as(
        "SELECT a.id, a.name, a.artist_id, COALESCE(ar.name, ''), a.year, a.image_path, COALESCE((SELECT COUNT(*) FROM tracks WHERE album_id = a.id), 0) FROM albums a LEFT JOIN artists ar ON ar.id = a.artist_id ORDER BY a.year DESC, a.name ASC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(start_index)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, artist_id, artist_name, year, image_path, song_count)| json_album_item(&id, &name, &artist_id, &artist_name, year, image_path.as_deref(), song_count))
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    }))
}

async fn list_albums_for_artist(
    state: &Arc<AppState>,
    artist_id: &str,
    start_index: i32,
    limit: i32,
) -> Json<serde_json::Value> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM albums WHERE artist_id = ?",
    )
    .bind(artist_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    tracing::info!("list_albums_for_artist: artist_id={artist_id} total={total} limit={limit} offset={start_index}");

    let rows: Vec<(String, String, String, String, Option<i32>, Option<String>, i64)> = sqlx::query_as(
        "SELECT a.id, a.name, a.artist_id, COALESCE(ar.name, ''), a.year, a.image_path, COALESCE((SELECT COUNT(*) FROM tracks WHERE album_id = a.id), 0) FROM albums a LEFT JOIN artists ar ON ar.id = a.artist_id WHERE a.artist_id = ? ORDER BY a.year DESC, a.name ASC LIMIT ? OFFSET ?",
    )
    .bind(artist_id)
    .bind(limit)
    .bind(start_index)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, art_id, artist_name, year, image_path, song_count)| json_album_item(&id, &name, &art_id, &artist_name, year, image_path.as_deref(), song_count))
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    }))
}

async fn list_tracks(
    state: &Arc<AppState>,
    start_index: i32,
    limit: i32,
) -> Json<serde_json::Value> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let rows: Vec<(String, String, f64, String, String, String, String, String, Option<i32>, Option<i32>)> =
        sqlx::query_as(
            "SELECT t.id, t.name, t.duration, t.artist_id, COALESCE(ar.name, ''), t.album_id, COALESCE(al.name, ''), COALESCE(aal.name, ''), t.track_number, t.disc_number FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id LEFT JOIN albums al ON al.id = t.album_id LEFT JOIN artists aal ON aal.id = al.artist_id ORDER BY t.created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(start_index)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, duration, artist_id, artist_name, album_id, album_name, album_artist_name, track_num, disc_num)| {
            json_track_item(&id, &name, duration, &artist_id, &artist_name, &album_id, &album_name, &album_artist_name, track_num, disc_num)
        })
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    }))
}

async fn list_tracks_for_album(
    state: &Arc<AppState>,
    album_id: &str,
    start_index: i32,
    limit: i32,
) -> Json<serde_json::Value> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE album_id = ?",
    )
    .bind(album_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let rows: Vec<(String, String, f64, String, String, String, String, String, Option<i32>, Option<i32>)> =
        sqlx::query_as(
            "SELECT t.id, t.name, t.duration, t.artist_id, COALESCE(ar.name, ''), t.album_id, COALESCE(al.name, ''), COALESCE(aal.name, ''), t.track_number, t.disc_number FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id LEFT JOIN albums al ON al.id = t.album_id LEFT JOIN artists aal ON aal.id = al.artist_id WHERE t.album_id = ? ORDER BY t.disc_number ASC, t.track_number ASC LIMIT ? OFFSET ?",
        )
        .bind(album_id)
        .bind(limit)
        .bind(start_index)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, duration, artist_id, artist_name, album_id, album_name, album_artist_name, track_num, disc_num)| {
            json_track_item(&id, &name, duration, &artist_id, &artist_name, &album_id, &album_name, &album_artist_name, track_num, disc_num)
        })
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    }))
}

pub async fn get_item(
    State(state): State<Arc<AppState>>,
    Path(params): Path<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let item_id = params.get("item_id").map(|s| s.as_str()).unwrap_or("");
    tracing::info!("Get item: {item_id}");

    if let Ok(row) = sqlx::query_as::<_, (String, String, f64, String, String, String, String, String, Option<i32>, Option<i32>, String, String, Option<i64>, Option<i32>, Option<i32>)>(
        "SELECT t.id, t.name, t.duration, t.artist_id, COALESCE(ar.name, ''), t.album_id, COALESCE(al.name, ''), COALESCE(aal.name, ''), t.track_number, t.disc_number, t.file_path, t.mime_type, t.bitrate, t.sample_rate, t.channels FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id LEFT JOIN albums al ON al.id = t.album_id LEFT JOIN artists aal ON aal.id = al.artist_id WHERE t.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    {
        if let Some((id, name, duration, artist_id, artist_name, album_id, album_name, album_artist_name, track_num, disc_num, file_path, mime_type, bitrate, sample_rate, channels)) = row {
            return Json(json_track_item_detailed(&id, &name, duration, &artist_id, &artist_name, &album_id, &album_name, &album_artist_name, track_num, disc_num, &file_path, &mime_type, bitrate, sample_rate, channels));
        }
    }

    if let Ok(row) = sqlx::query_as::<_, (String, String, String, String, Option<i32>, Option<String>, i64)>(
        "SELECT a.id, a.name, a.artist_id, COALESCE(ar.name, ''), a.year, a.image_path, COALESCE((SELECT COUNT(*) FROM tracks WHERE album_id = a.id), 0) FROM albums a LEFT JOIN artists ar ON ar.id = a.artist_id WHERE a.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    {
        if let Some((id, name, artist_id, artist_name, year, image_path, song_count)) = row {
            return Json(json_album_item(&id, &name, &artist_id, &artist_name, year, image_path.as_deref(), song_count));
        }
    }

    if let Ok(row) = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT a.id, a.name, COALESCE((SELECT COUNT(*) FROM albums WHERE artist_id = a.id), 0) FROM artists a WHERE a.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    {
        if let Some((id, name, album_count)) = row {
            return Json(json_artist_item(&id, &name, album_count));
        }
    }

    Json(json!({
        "Id": item_id,
        "Name": "Unknown",
        "Type": "Unknown",
    }))
}

pub async fn get_artists(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let start_index = query.start_index.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).max(1).min(200);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artists")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    tracing::info!("List artists: total={total} offset={start_index} limit={limit}");

    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT a.id, a.name, COALESCE(alb.cnt, 0) FROM artists a LEFT JOIN (SELECT artist_id, COUNT(*) as cnt FROM albums GROUP BY artist_id) alb ON alb.artist_id = a.id ORDER BY a.sort_name ASC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(start_index)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, album_count)| json_artist_item(&id, &name, album_count))
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": total,
        "StartIndex": start_index,
    }))
}

pub async fn get_album_artists(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    get_artists(State(state), Query(query)).await
}

pub async fn get_artist(
    State(state): State<Arc<AppState>>,
    Path(artist_id): Path<String>,
) -> Json<serde_json::Value> {
    tracing::info!("Get artist: {artist_id}");
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT a.id, a.name, COALESCE((SELECT COUNT(*) FROM albums WHERE artist_id = a.id), 0) FROM artists a WHERE a.id = ?",
    )
    .bind(&artist_id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    match row {
        Some((id, name, album_count)) => Json(json_artist_item(&id, &name, album_count)),
        None => Json(json!({"Id": artist_id, "Name": "Unknown", "Type": "MusicArtist"})),
    }
}

pub async fn get_resume_items(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let user_id = query.user_id.as_deref().unwrap_or("");
    let start_index = query.start_index.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).max(1).min(200);

    if user_id.is_empty() {
        return Json(json!({"Items": [], "TotalRecordCount": 0, "StartIndex": start_index}));
    }

    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT item_id, playback_position_ticks FROM user_data WHERE user_id = ? AND playback_position_ticks > 0 AND played = 0 ORDER BY last_played_date DESC LIMIT ? OFFSET ?",
    )
    .bind(user_id)
    .bind(limit)
    .bind(start_index)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut items = Vec::new();
    for (item_id, position) in rows {
        let item = get_item_inner(&state, &item_id).await;
        if let Some(mut item) = item {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("UserData".into(), json!({
                    "PlaybackPositionTicks": position,
                    "Played": false,
                }));
            }
            items.push(item);
        }
    }

    Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
        "StartIndex": start_index,
    }))
}

pub async fn get_latest_items(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let limit = query.limit.unwrap_or(20).max(1).min(50);

    let rows: Vec<(String, String, f64, String, String, String, String, String, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT t.id, t.name, t.duration, t.artist_id, COALESCE(ar.name, ''), t.album_id, COALESCE(al.name, ''), COALESCE(aal.name, ''), t.track_number, t.disc_number FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id LEFT JOIN albums al ON al.id = t.album_id LEFT JOIN artists aal ON aal.id = al.artist_id ORDER BY t.created_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name, duration, artist_id, artist_name, album_id, album_name, album_artist_name, track_num, disc_num)| {
            json_track_item(&id, &name, duration, &artist_id, &artist_name, &album_id, &album_name, &album_artist_name, track_num, disc_num)
        })
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
        "StartIndex": 0,
    }))
}

pub async fn get_playback_info(
    State(state): State<Arc<AppState>>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    Query(_query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let item_id = params.get("item_id").map(|s| s.as_str()).unwrap_or("");

    let track: Option<(String, String, f64, String, String, String, String, Option<i64>)> =
        sqlx::query_as(
            "SELECT t.id, t.name, t.duration, t.file_path, t.mime_type, t.artist_id, t.album_id, t.bitrate FROM tracks t WHERE t.id = ?",
        )
        .bind(item_id)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);

    match track {
        Some((id, _name, duration, file_path, mime_type, _artist_id, _album_id, bitrate)) => {
            let container = mime_type
                .rsplit('/')
                .next()
                .unwrap_or("mp3")
                .to_string();

            Json(json!({
                "MediaSources": [{
                    "Id": id,
                    "Path": file_path,
                    "Protocol": "File",
                    "Container": container,
                    "RunTimeTicks": (duration * TICKS_PER_SECOND as f64) as i64,
                    "BitRate": bitrate.unwrap_or(320000),
                    "AudioStreamIndex": 0,
                    "MediaStreams": [],
                    "Formats": [],
                    "RequiredHttpHeaders": {},
                    "SupportsDirectPlay": true,
                    "SupportsDirectStream": true,
                    "SupportsTranscoding": false,
                    "IsRemote": false,
                }],
                "PlaySessionId": Uuid::new_v4().to_string(),
            }))
        }
        None => Json(json!({
            "MediaSources": [],
            "PlaySessionId": Uuid::new_v4().to_string(),
        })),
    }
}

pub async fn post_playback_info(
    State(state): State<Arc<AppState>>,
    Path(item_id): Path<String>,
) -> Json<serde_json::Value> {
    let mut params = std::collections::HashMap::new();
    params.insert("item_id".to_string(), item_id);

    get_playback_info(
        State(state),
        Path(params),
        Query(ItemsQuery::default()),
    )
    .await
}

pub async fn update_user_data(
    State(state): State<Arc<AppState>>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let user_id = params.get("user_id").map(|s| s.as_str()).unwrap_or("");
    let item_id = params.get("item_id").map(|s| s.as_str()).unwrap_or("");

    let is_fav = body.get("IsFavorite").and_then(|v| v.as_bool());
    let played = body.get("Played").and_then(|v| v.as_bool());
    let pos = body.get("PlaybackPositionTicks").and_then(|v| v.as_i64());
    tracing::info!("UserData: user={} item={} is_favorite={is_fav:?} played={played:?} position={pos:?}", user_id, item_id);

    if user_id.is_empty() || item_id.is_empty() {
        return Json(json!({}));
    }

    let is_favorite = body.get("IsFavorite").and_then(|v| v.as_bool());
    let played = body.get("Played").and_then(|v| v.as_bool());
    let rating = body.get("Rating").and_then(|v| v.as_f64());
    let position = body.get("PlaybackPositionTicks").and_then(|v| v.as_i64());

    if let Some(fav) = is_favorite {
        sqlx::query(
            "INSERT OR REPLACE INTO user_data (user_id, item_id, played, is_favorite, play_count, playback_position_ticks)
             VALUES (?, ?,
                     COALESCE((SELECT played FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     ?, 0, 0)"
        )
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(fav)
            .execute(&state.db)
            .await
            .ok();
    }

    if let Some(p) = played {
        sqlx::query(
            "INSERT OR REPLACE INTO user_data (user_id, item_id, played, is_favorite, play_count, playback_position_ticks)
             VALUES (?, ?, ?,
                     COALESCE((SELECT is_favorite FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     COALESCE((SELECT play_count FROM user_data WHERE user_id = ? AND item_id = ?), 0) + 1, 0)"
        )
            .bind(user_id).bind(item_id).bind(p)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .execute(&state.db)
            .await
            .ok();
    }

    if let Some(r) = rating {
        sqlx::query(
            "INSERT OR REPLACE INTO user_data (user_id, item_id, played, is_favorite, play_count, playback_position_ticks, rating)
             VALUES (?, ?,
                     COALESCE((SELECT played FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     COALESCE((SELECT is_favorite FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     COALESCE((SELECT play_count FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     COALESCE((SELECT playback_position_ticks FROM user_data WHERE user_id = ? AND item_id = ?), 0), ?)"
        )
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(r)
            .execute(&state.db)
            .await
            .ok();
    }

    if let Some(pos) = position {
        sqlx::query(
            "INSERT OR REPLACE INTO user_data (user_id, item_id, played, is_favorite, play_count, playback_position_ticks, last_played_date)
             VALUES (?, ?,
                     COALESCE((SELECT played FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     COALESCE((SELECT is_favorite FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     COALESCE((SELECT play_count FROM user_data WHERE user_id = ? AND item_id = ?), 0),
                     ?, datetime('now'))"
        )
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(user_id).bind(item_id)
            .bind(pos)
            .execute(&state.db)
            .await
            .ok();
    }

    Json(json!({}))
}

pub async fn get_favorites(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let start_index = query.start_index.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(100).max(1).min(200);

    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT item_id FROM user_data WHERE user_id = ? AND is_favorite = 1 LIMIT ? OFFSET ?",
    )
    .bind(&user_id)
    .bind(limit)
    .bind(start_index)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut items = Vec::new();
    for (id,) in &ids {
        if let Some(item) = get_item_inner(&state, id).await {
            items.push(item);
        }
    }

    Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
        "StartIndex": start_index,
    }))
}

pub async fn get_genres() -> Json<serde_json::Value> {
    Json(json!({
        "Items": [],
        "TotalRecordCount": 0,
        "StartIndex": 0,
    }))
}

pub async fn get_playlists(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<serde_json::Value> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, name FROM playlists WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(&user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, name)| {
            json!({
                "Id": id,
                "Name": name,
                "Type": "Playlist",
                "MediaType": "Audio",
                "ImageTags": {},
                "BackdropImageTags": [],
            })
        })
        .collect();

    Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
        "StartIndex": 0,
    }))
}

pub async fn create_playlist(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let name = body.get("Name").and_then(|v| v.as_str()).unwrap_or("New Playlist");
    let id = Uuid::new_v4().to_string();

    sqlx::query("INSERT INTO playlists (id, name, user_id) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(&user_id)
        .execute(&state.db)
        .await
        .ok();

    Json(json!({
        "Id": id,
        "Name": name,
        "Type": "Playlist",
    }))
}

pub async fn add_to_playlist(
    State(state): State<Arc<AppState>>,
    Path(playlist_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let ids = body
        .get("Ids")
        .and_then(|v| v.as_str())
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .unwrap_or_default();

    for (i, track_id) in ids.iter().enumerate() {
        let idx = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT MAX(idx) FROM playlist_items WHERE playlist_id = ?",
        )
        .bind(&playlist_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            + 1
            + i as i32;

        sqlx::query("INSERT OR IGNORE INTO playlist_items (playlist_id, track_id, idx) VALUES (?, ?, ?)")
            .bind(&playlist_id)
            .bind(track_id)
            .bind(idx)
            .execute(&state.db)
            .await
            .ok();
    }

    Json(json!({}))
}

pub async fn remove_from_playlist(
    State(state): State<Arc<AppState>>,
    Path((playlist_id, item_id)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    sqlx::query("DELETE FROM playlist_items WHERE playlist_id = ? AND track_id = ?")
        .bind(&playlist_id)
        .bind(&item_id)
        .execute(&state.db)
        .await
        .ok();

    Json(json!({}))
}

pub async fn get_playlist_items(
    State(state): State<Arc<AppState>>,
    Path(playlist_id): Path<String>,
) -> Json<serde_json::Value> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT track_id FROM playlist_items WHERE playlist_id = ? ORDER BY idx ASC",
    )
    .bind(&playlist_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut items = Vec::new();
    for (track_id,) in rows {
        if let Ok(row) = sqlx::query_as::<_, (String, String, f64, String, String, String, String, String, Option<i32>, Option<i32>)>(
            "SELECT t.id, t.name, t.duration, t.artist_id, COALESCE(ar.name, ''), t.album_id, COALESCE(al.name, ''), COALESCE(aal.name, ''), t.track_number, t.disc_number FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id LEFT JOIN albums al ON al.id = t.album_id LEFT JOIN artists aal ON aal.id = al.artist_id WHERE t.id = ?",
        )
        .bind(&track_id)
        .fetch_optional(&state.db)
        .await
        {
            if let Some((id, name, duration, artist_id, artist_name, album_id, album_name, album_artist_name, track_num, disc_num)) = row {
                items.push(json_track_item(&id, &name, duration, &artist_id, &artist_name, &album_id, &album_name, &album_artist_name, track_num, disc_num));
            }
        }
    }

    Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
        "StartIndex": 0,
    }))
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Json<serde_json::Value> {
    let term = query.search_term.as_deref().unwrap_or("");
    let limit = query.limit.unwrap_or(20).max(1).min(100);

    if !term.is_empty() {
        tracing::info!("Search: term=\"{term}\" limit={limit}");
    }

    if term.is_empty() {
        return Json(json!({
            "SearchHints": [],
            "TotalRecordCount": 0,
        }));
    }

    let pattern = format!("%{term}%");
    let mut hints = Vec::new();

    if let Ok(rows) = sqlx::query_as::<_, (String, String)>(
        "SELECT id, name FROM artists WHERE name LIKE ? LIMIT ?",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    {
        for (id, name) in rows {
            hints.push(json!({
                "ItemId": id,
                "Name": name,
                "Type": "MusicArtist",
                "MatchedTerm": term,
                "MediaType": "Audio",
            }));
        }
    }

    if hints.len() < limit as usize {
        if let Ok(rows) = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM albums WHERE name LIKE ? LIMIT ?",
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        {
            for (id, name) in rows {
                hints.push(json!({
                    "ItemId": id,
                    "Name": name,
                    "Type": "MusicAlbum",
                    "MatchedTerm": term,
                    "MediaType": "Audio",
                }));
            }
        }
    }

    if hints.len() < limit as usize {
        if let Ok(rows) = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM tracks WHERE name LIKE ? LIMIT ?",
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        {
            for (id, name) in rows {
                hints.push(json!({
                    "ItemId": id,
                    "Name": name,
                    "Type": "Audio",
                    "MatchedTerm": term,
                    "MediaType": "Audio",
                }));
            }
        }
    }

    Json(json!({
        "SearchHints": hints,
        "TotalRecordCount": hints.len(),
    }))
}

async fn get_item_inner(
    state: &Arc<AppState>,
    item_id: &str,
) -> Option<serde_json::Value> {
    if let Ok(row) = sqlx::query_as::<_, (String, String, f64, String, String, String, String, String, Option<i32>, Option<i32>)>(
        "SELECT t.id, t.name, t.duration, t.artist_id, COALESCE(ar.name, ''), t.album_id, COALESCE(al.name, ''), COALESCE(aal.name, ''), t.track_number, t.disc_number FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id LEFT JOIN albums al ON al.id = t.album_id LEFT JOIN artists aal ON aal.id = al.artist_id WHERE t.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    {
        if let Some((id, name, duration, artist_id, artist_name, album_id, album_name, album_artist_name, track_num, disc_num)) = row {
            return Some(json_track_item(&id, &name, duration, &artist_id, &artist_name, &album_id, &album_name, &album_artist_name, track_num, disc_num));
        }
    }

    if let Ok(row) = sqlx::query_as::<_, (String, String, String, String, Option<i32>, Option<String>, i64)>(
        "SELECT a.id, a.name, a.artist_id, COALESCE(ar.name, ''), a.year, a.image_path, COALESCE((SELECT COUNT(*) FROM tracks WHERE album_id = a.id), 0) FROM albums a LEFT JOIN artists ar ON ar.id = a.artist_id WHERE a.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    {
        if let Some((id, name, artist_id, artist_name, year, image_path, song_count)) = row {
            return Some(json_album_item(&id, &name, &artist_id, &artist_name, year, image_path.as_deref(), song_count));
        }
    }

    if let Ok(row) = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT a.id, a.name, COALESCE((SELECT COUNT(*) FROM albums WHERE artist_id = a.id), 0) FROM artists a WHERE a.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    {
        if let Some((id, name, album_count)) = row {
            return Some(json_artist_item(&id, &name, album_count));
        }
    }

    None
}

fn json_artist_item(id: &str, name: &str, album_count: i64) -> serde_json::Value {
    json!({
        "Id": id,
        "Name": name,
        "Type": "MusicArtist",
        "MediaType": "Audio",
        "ImageTags": {},
        "BackdropImageTags": [],
        "LocationType": "FileSystem",
        "AlbumCount": album_count,
    })
}

fn json_album_item(id: &str, name: &str, artist_id: &str, artist_name: &str, year: Option<i32>, image_path: Option<&str>, song_count: i64) -> serde_json::Value {
    let mut image_tags = serde_json::Map::new();
    if image_path.is_some() {
        image_tags.insert("Primary".into(), serde_json::Value::String(id.to_string()));
    }

    json!({
        "Id": id,
        "Name": name,
        "Type": "MusicAlbum",
        "MediaType": "Audio",
        "AlbumArtist": artist_name,
        "AlbumArtists": [{"Name": artist_name, "Id": artist_id}],
        "ArtistItems": [{"Name": artist_name, "Id": artist_id}],
        "Artists": [artist_name],
        "ImageTags": image_tags,
        "BackdropImageTags": [],
        "BackdropImageItemId": "",
        "ProductionYear": year,
        "PremiereDate": year.map(|y| format!("{y}-01-01T00:00:00.000Z")),
        "ParentId": artist_id,
        "LocationType": "FileSystem",
        "RunTimeTicks": 0,
        "ChildCount": song_count,
        "SongCount": song_count,
    })
}

fn json_track_item(
    id: &str,
    name: &str,
    duration: f64,
    artist_id: &str,
    artist_name: &str,
    album_id: &str,
    album_name: &str,
    album_artist_name: &str,
    track_number: Option<i32>,
    disc_number: Option<i32>,
) -> serde_json::Value {
    json!({
        "Id": id,
        "Name": name,
        "Type": "Audio",
        "MediaType": "Audio",
        "IndexNumber": track_number,
        "ParentIndexNumber": disc_number,
        "RunTimeTicks": (duration * TICKS_PER_SECOND as f64) as i64,
        "Album": album_name,
        "AlbumId": album_id,
        "AlbumArtist": album_artist_name,
        "AlbumArtists": [{"Name": album_artist_name, "Id": artist_id}],
        "Artists": [artist_name],
        "ArtistItems": [{"Name": artist_name, "Id": artist_id}],
        "ParentId": album_id,
        "ImageTags": {},
        "BackdropImageTags": [],
        "LocationType": "FileSystem",
    })
}

fn json_track_item_detailed(
    id: &str,
    name: &str,
    duration: f64,
    artist_id: &str,
    artist_name: &str,
    album_id: &str,
    album_name: &str,
    album_artist_name: &str,
    track_number: Option<i32>,
    disc_number: Option<i32>,
    file_path: &str,
    mime_type: &str,
    bitrate: Option<i64>,
    sample_rate: Option<i32>,
    channels: Option<i32>,
) -> serde_json::Value {
    let container = mime_type
        .rsplit('/')
        .next()
        .unwrap_or("mp3")
        .to_string();

    json!({
        "Id": id,
        "Name": name,
        "Type": "Audio",
        "MediaType": "Audio",
        "Path": file_path,
        "IndexNumber": track_number,
        "ParentIndexNumber": disc_number,
        "RunTimeTicks": (duration * TICKS_PER_SECOND as f64) as i64,
        "Album": album_name,
        "AlbumId": album_id,
        "AlbumArtist": album_artist_name,
        "AlbumArtists": [{"Name": album_artist_name, "Id": artist_id}],
        "Artists": [artist_name],
        "ArtistItems": [{"Name": artist_name, "Id": artist_id}],
        "ParentId": album_id,
        "ImageTags": {},
        "BackdropImageTags": [],
        "BackdropImageItemId": "",
        "LocationType": "FileSystem",
        "MediaSources": [{
            "Id": id,
            "Path": file_path,
            "Protocol": "File",
            "Container": container,
            "RunTimeTicks": (duration * TICKS_PER_SECOND as f64) as i64,
            "BitRate": bitrate.unwrap_or(320000),
            "AudioStreamIndex": 0,
            "SupportsDirectPlay": true,
            "SupportsDirectStream": true,
            "SupportsTranscoding": false,
        }],
        "MediaStreams": [{
            "Codec": container,
            "Type": "Audio",
            "Index": 0,
            "SampleRate": sample_rate.unwrap_or(44100),
            "Channels": channels.unwrap_or(2),
            "BitRate": bitrate.map(|b| b as i32).unwrap_or(320000),
        }],
    })
}

pub async fn get_similar(
    State(state): State<Arc<AppState>>,
    Path(item_id): Path<String>,
) -> Json<serde_json::Value> {
    tracing::info!("Get similar items: item_id={item_id}");

    // Find the artist of the given item, then return other items by the same artist
    let artist_id: Option<String> = sqlx::query_scalar(
        "SELECT artist_id FROM tracks WHERE id = ? UNION SELECT artist_id FROM albums WHERE id = ?",
    )
    .bind(&item_id)
    .bind(&item_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let mut items = Vec::new();
    if let Some(aid) = artist_id {
        let rows: Vec<(String, String, String, String, Option<i32>, Option<String>, i64)> = sqlx::query_as(
            "SELECT a.id, a.name, a.artist_id, COALESCE(ar.name, ''), a.year, a.image_path, COALESCE((SELECT COUNT(*) FROM tracks WHERE album_id = a.id), 0) FROM albums a LEFT JOIN artists ar ON ar.id = a.artist_id WHERE a.artist_id = ? AND a.id != ? ORDER BY a.year DESC LIMIT 10",
        )
        .bind(&aid)
        .bind(&item_id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        for (id, name, art_id, artist_name, year, image_path, song_count) in rows {
            items.push(json_album_item(&id, &name, &art_id, &artist_name, year, image_path.as_deref(), song_count));
        }
    }

    Json(json!({
        "Items": items,
        "TotalRecordCount": items.len(),
    }))
}

pub async fn get_image(
    State(state): State<Arc<AppState>>,
    Path(params): Path<std::collections::HashMap<String, String>>,
) -> Result<Response, (StatusCode, String)> {
    let item_id = params.get("item_id").map(|s| s.as_str()).unwrap_or("");
    let _image_type = params.get("image_type").map(|s| s.as_str()).unwrap_or("Primary");

    let image_path: Option<String> = sqlx::query_scalar(
        "SELECT image_path FROM albums WHERE id = ? AND image_path IS NOT NULL",
    )
    .bind(item_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .flatten();

    let image_path = match image_path {
        Some(p) => p,
        None => return Err((StatusCode::NOT_FOUND, "Image not found".into())),
    };

    let file = File::open(&image_path)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, format!("File not found: {e}")))?;

    let ext = std::path::Path::new(&image_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_lowercase();

    let content_type = match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(body)
        .unwrap())
}
