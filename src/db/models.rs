use chrono::NaiveDateTime;

#[derive(Debug, sqlx::FromRow)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ArtistRow {
    pub id: String,
    pub name: String,
    pub sort_name: String,
    pub image_path: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AlbumRow {
    pub id: String,
    pub name: String,
    pub artist_id: String,
    pub year: Option<i32>,
    pub image_path: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, sqlx::FromRow)]
pub struct TrackRow {
    pub id: String,
    pub name: String,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub duration: f64,
    pub artist_id: String,
    pub album_id: String,
    pub file_path: String,
    pub mime_type: String,
    pub bitrate: Option<i64>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, sqlx::FromRow)]
pub struct UserDataRow {
    pub user_id: String,
    pub item_id: String,
    pub played: bool,
    pub is_favorite: bool,
    pub play_count: i32,
    pub playback_position_ticks: i64,
    pub rating: Option<f64>,
    pub last_played_date: Option<NaiveDateTime>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SessionRow {
    pub id: String,
    pub user_id: String,
    pub access_token: String,
    pub client: String,
    pub device_name: String,
    pub device_id: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, sqlx::FromRow)]
pub struct PlaylistRow {
    pub id: String,
    pub name: String,
    pub user_id: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, sqlx::FromRow)]
pub struct PlaylistItemRow {
    pub playlist_id: String,
    pub track_id: String,
    pub index: i32,
    pub created_at: NaiveDateTime,
}
