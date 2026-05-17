pub mod scanner;

use sqlx::SqlitePool;
use std::path::Path;
use tracing::{error, info};
use uuid::Uuid;

pub struct Library;

impl Library {
    pub async fn scan_and_import(pool: &SqlitePool, music_dir: &Path) {
        info!("Scanning music directory: {}", music_dir.display());

        if !music_dir.exists() {
            tracing::warn!("Music directory does not exist: {}", music_dir.display());
            return;
        }

        let result = scanner::scan_directory(music_dir);

        for e in &result.errors {
            tracing::warn!("Scan error: {e}");
        }

        tracing::info!("Scanner found {} artists, {} albums, {} tracks ({} errors)",
            result.artists.len(), result.albums.len(), result.tracks.len(), result.errors.len());

        // Resolve artist IDs against DB — insert new, keep existing
        let mut artist_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (scan_id, name) in &result.artists {
            let existing_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM artists WHERE name = ?",
            )
            .bind(name)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);

            match existing_id {
                Some(db_id) => {
                    artist_map.insert(scan_id.clone(), db_id);
                }
                None => {
                    let new_id = Uuid::new_v4().to_string();
                    let sort_name = sort_name(name);
                    if let Err(e) = sqlx::query("INSERT INTO artists (id, name, sort_name) VALUES (?, ?, ?)")
                        .bind(&new_id)
                        .bind(name)
                        .bind(&sort_name)
                        .execute(pool)
                        .await
                    {
                        error!("Failed to insert artist {name}: {e}");
                        continue;
                    }
                    artist_map.insert(scan_id.clone(), new_id);
                }
            }
        }

        // Resolve album IDs against DB — insert new, keep existing
        let mut album_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (scan_id, name, scan_artist_id, year, image_path) in &result.albums {
            let db_artist_id = artist_map.get(scan_artist_id);

            let db_artist_id = match db_artist_id {
                Some(id) => id,
                None => {
                    tracing::warn!("Artist {scan_artist_id} not found for album {name}, skipping");
                    continue;
                }
            };

            let existing_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM albums WHERE name = ? AND artist_id = ?",
            )
            .bind(name)
            .bind(db_artist_id)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);

            match existing_id {
                Some(db_id) => {
                    if let Some(img_path) = image_path {
                        if let Err(e) = sqlx::query("UPDATE albums SET image_path = ? WHERE id = ? AND image_path IS NULL")
                            .bind(img_path)
                            .bind(&db_id)
                            .execute(pool)
                            .await
                        {
                            error!("Failed to update album image {name}: {e}");
                        }
                    }
                    if let Err(e) = sqlx::query("UPDATE albums SET year = ? WHERE id = ? AND year IS NULL")
                        .bind(year)
                        .bind(&db_id)
                        .execute(pool)
                        .await
                    {
                        error!("Failed to update album year {name}: {e}");
                    }
                    tracing::info!("Updated year for existing album: name={name} year={year:?}");
                    album_map.insert(scan_id.clone(), db_id);
                }
                None => {
                    let new_id = Uuid::new_v4().to_string();
                    if let Err(e) = sqlx::query("INSERT INTO albums (id, name, artist_id, year, image_path) VALUES (?, ?, ?, ?, ?)")
                        .bind(&new_id)
                        .bind(name)
                        .bind(db_artist_id)
                        .bind(year)
                        .bind(image_path)
                        .execute(pool)
                        .await
                    {
                        error!("Failed to insert album {name}: {e}");
                        continue;
                    }
                    tracing::info!("Inserted new album: name={name} year={year:?}");
                    album_map.insert(scan_id.clone(), new_id);
                }
            }
        }

        // Insert tracks using resolved IDs
        for track in &result.tracks {
            let artist_id = result
                .artists
                .iter()
                .find(|(_, a)| a == &track.artist)
                .or_else(|| result.artists.iter().find(|(_, a)| a == &track.album_artist))
                .and_then(|(sid, _)| artist_map.get(sid))
                .cloned();

            let album_artist_id = result
                .artists
                .iter()
                .find(|(_, a)| a == &track.album_artist)
                .and_then(|(sid, _)| artist_map.get(sid))
                .or(artist_id.as_ref());

            let album_id = result
                .albums
                .iter()
                .find(|(_, name, a_id, _, _)| name == &track.album && artist_map.get(a_id) == album_artist_id)
                .and_then(|(sid, _, _, _, _)| album_map.get(sid))
                .cloned();

            let artist_id = match artist_id {
                Some(id) => id,
                None => {
                    tracing::warn!("No artist resolved for track {}", track.title);
                    continue;
                }
            };

            let album_id = match album_id {
                Some(id) => id,
                None => {
                    tracing::warn!("No album resolved for track {}", track.title);
                    continue;
                }
            };

            let exists: bool = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM tracks WHERE file_path = ?",
            )
            .bind(&track.file_path)
            .fetch_one(pool)
            .await
            .unwrap_or(0)
                > 0;

            if !exists {
                let track_id = Uuid::new_v4().to_string();
                if let Err(e) = sqlx::query(
                    "INSERT INTO tracks (id, name, track_number, disc_number, duration, artist_id, album_id, file_path, mime_type, bitrate, sample_rate, channels) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                    .bind(&track_id)
                    .bind(&track.title)
                    .bind(track.track_number)
                    .bind(track.disc_number)
                    .bind(track.duration)
                    .bind(&artist_id)
                    .bind(&album_id)
                    .bind(&track.file_path)
                    .bind(&track.mime_type)
                    .bind(track.bitrate)
                    .bind(track.sample_rate)
                    .bind(track.channels)
                    .execute(pool)
                    .await
                {
                    error!("Failed to insert track {}: {e}", track.title);
                }
            }
        }

        info!("Library scan complete");
    }
}

fn sort_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some(stripped) = trimmed.strip_prefix("The ") {
        return format!("{stripped}, The");
    }
    if let Some(stripped) = trimmed.strip_prefix("A ") {
        return format!("{stripped}, A");
    }
    if let Some(stripped) = trimmed.strip_prefix("An ") {
        return format!("{stripped}, An");
    }
    trimmed.to_string()
}
