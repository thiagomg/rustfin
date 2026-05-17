pub mod models;

use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::path::Path;
use tracing::info;

pub async fn init_pool(db_path: &str) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = Path::new(db_path).parent() {
        if !parent.exists() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite:{db_path}?mode=rwc"))
        .await?;

    run_migrations(&pool).await?;

    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    info!("Running database migrations");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            display_name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS artists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            sort_name TEXT NOT NULL DEFAULT '',
            image_path TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS albums (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            artist_id TEXT NOT NULL REFERENCES artists(id),
            year INTEGER,
            image_path TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tracks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            track_number INTEGER,
            disc_number INTEGER DEFAULT 1,
            duration REAL NOT NULL DEFAULT 0,
            artist_id TEXT NOT NULL REFERENCES artists(id),
            album_id TEXT NOT NULL REFERENCES albums(id),
            file_path TEXT NOT NULL,
            mime_type TEXT NOT NULL DEFAULT 'audio/mpeg',
            bitrate INTEGER,
            sample_rate INTEGER,
            channels INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_data (
            user_id TEXT NOT NULL REFERENCES users(id),
            item_id TEXT NOT NULL,
            played INTEGER NOT NULL DEFAULT 0,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            play_count INTEGER NOT NULL DEFAULT 0,
            playback_position_ticks INTEGER NOT NULL DEFAULT 0,
            rating REAL,
            last_played_date TEXT,
            PRIMARY KEY (user_id, item_id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            access_token TEXT NOT NULL UNIQUE,
            client TEXT NOT NULL DEFAULT '',
            device_name TEXT NOT NULL DEFAULT '',
            device_id TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            user_id TEXT NOT NULL REFERENCES users(id),
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS playlist_items (
            playlist_id TEXT NOT NULL REFERENCES playlists(id),
            track_id TEXT NOT NULL REFERENCES tracks(id),
            idx INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (playlist_id, track_id)
        )",
    )
    .execute(pool)
    .await?;

    // Add password_sha column for Supersonic/Symphonium compatibility
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN password_sha TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await;

    // Insert default admin user if not exists
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE username = 'admin'")
            .fetch_optional(pool)
            .await?;

    if existing.is_none() {
        let id = uuid::Uuid::new_v4().to_string();
        let password_hash = bcrypt::hash("admin", bcrypt::DEFAULT_COST).unwrap();
        let password_sha = hex::encode(Sha256::digest(b"admin"));
        sqlx::query("INSERT INTO users (id, username, password_hash, password_sha, display_name) VALUES (?, ?, ?, ?, ?)")
            .bind(&id)
            .bind("admin")
            .bind(&password_hash)
            .bind(&password_sha)
            .bind("Admin")
            .execute(pool)
            .await?;
        info!("Created default admin user (password: admin)");
    } else {
        // Ensure existing admin has password_sha populated
        let empty_sha: Option<(String,)> =
            sqlx::query_as("SELECT id FROM users WHERE username = 'admin' AND (password_sha IS NULL OR password_sha = '')")
                .fetch_optional(pool)
                .await?;
        if let Some((admin_id,)) = empty_sha {
            let password_sha = hex::encode(Sha256::digest(b"admin"));
            sqlx::query("UPDATE users SET password_sha = ? WHERE id = ?")
                .bind(&password_sha)
                .bind(&admin_id)
                .execute(pool)
                .await?;
            info!("Updated admin user with SHA256 hash");
        }
    }

    info!("Database migrations complete");
    Ok(())
}
