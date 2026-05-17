pub mod auth;
pub mod audio;
pub mod items;
pub mod library;
pub mod system;
pub mod users;

use axum::{Router, middleware};
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub music_dir: String,
    pub server_name: String,
}

const PUBLIC_PATHS: &[&str] = &[
    "/System/Info/Public",
    "/Users/AuthenticateByName",
];

pub fn create_router(state: AppState) -> Router {
    let state = Arc::new(state);

    let app = Router::new()
        .route("/System/Info/Public", axum::routing::get(system::public_info))
        .route("/Users/AuthenticateByName", axum::routing::post(auth::authenticate_by_name))
        .route("/Users/authenticatebyname", axum::routing::post(auth::authenticate_by_name))
        .route("/System/Info", axum::routing::get(system::info))
        .route("/System/Info/Endpoints", axum::routing::get(system::endpoints))
        .route("/Users/:user_id", axum::routing::get(users::get_user))
        .route("/Users/:user_id/Items", axum::routing::get(items::get_items))
        .route("/Users/:user_id/Items/Resume", axum::routing::get(items::get_resume_items))
        .route("/Users/:user_id/Items/Latest", axum::routing::get(items::get_latest_items))
        .route("/Users/:user_id/Items/:item_id", axum::routing::get(items::get_item))
        .route("/Users/:user_id/Items/:item_id/PlaybackInfo", axum::routing::get(items::get_playback_info))
        .route("/Users/:user_id/Items/:item_id/UserData", axum::routing::post(items::update_user_data))
        .route("/Audio/:item_id/stream", axum::routing::get(audio::stream_audio))
        .route("/audio/:item_id/stream", axum::routing::get(audio::stream_audio))
        .route("/Audio/:item_id/universal", axum::routing::get(audio::universal_audio))
        .route("/audio/:item_id/universal", axum::routing::get(audio::universal_audio))
        .route("/Audio/:item_id/master.m3u8", axum::routing::get(audio::hls_playlist))
        .route("/audio/:item_id/master.m3u8", axum::routing::get(audio::hls_playlist))
        .route("/Videos/:item_id/stream", axum::routing::get(audio::stream_audio))
        .route("/videos/:item_id/stream", axum::routing::get(audio::stream_audio))
        .route("/Videos/:item_id/master.m3u8", axum::routing::get(audio::hls_playlist))
        .route("/videos/:item_id/master.m3u8", axum::routing::get(audio::hls_playlist))
        .route("/Items/:item_id/PlaybackInfo", axum::routing::get(items::get_playback_info))
        .route("/Items/:item_id/PlaybackInfo/Post", axum::routing::post(items::post_playback_info))
        .route("/Sessions/Playing", axum::routing::post(system::report_playback_start))
        .route("/Sessions/Playing/Stopped", axum::routing::post(system::report_playback_stopped))
        .route("/Sessions/Playing/Progress", axum::routing::post(system::report_playback_progress))
        .route("/Users/:user_id/Views", axum::routing::get(items::get_views))
        .route("/Artists", axum::routing::get(items::get_artists))
        .route("/Artists/:artist_id", axum::routing::get(items::get_artist))
        .route("/Artists/AlbumArtists", axum::routing::get(items::get_album_artists))
        .route("/Genres", axum::routing::get(items::get_genres))
        .route("/Users/:user_id/Favorites", axum::routing::get(items::get_favorites))
        .route("/Users/:user_id/Playlists", axum::routing::get(items::get_playlists))
        .route("/Users/:user_id/Playlists", axum::routing::post(items::create_playlist))
        .route("/Playlists/:playlist_id/Items", axum::routing::post(items::add_to_playlist))
        .route("/Playlists/:playlist_id/Items/:item_id", axum::routing::delete(items::remove_from_playlist))
        .route("/Playlists/:playlist_id/Items", axum::routing::get(items::get_playlist_items))
        .route("/Search/Hints", axum::routing::get(items::search))
        .route("/Items/:item_id/Images/:image_type", axum::routing::get(items::get_image))
        .route("/items/:item_id/Images/:image_type", axum::routing::get(items::get_image))
        .route("/Items/:item_id/Images/:image_type/:index", axum::routing::get(items::get_image))
        .route("/items/:item_id/Images/:image_type/:index", axum::routing::get(items::get_image))
        .route("/Library/Media/Refresh", axum::routing::post(library::refresh_library))
        .route("/Library/Refresh", axum::routing::post(library::refresh_library))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state)
        .layer(tower_http::cors::CorsLayer::permissive());

    app
}
