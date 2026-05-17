use api::AppState;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;

mod api;
mod db;
mod library;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "music_server=debug,tower_http=info".into()),
        )
        .init();

    let music_dir = std::env::var("MUSIC_DIR").unwrap_or_else(|_| "./music".to_string());
    let db_path = std::env::var("DATABASE_URL").unwrap_or_else(|_| "./data/music-server.db".to_string());
    let bind = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8096".to_string());
    let server_name = std::env::var("SERVER_NAME").unwrap_or_else(|_| "Rust Music Server".to_string());

    info!("Starting music server on {bind}");
    info!("Music directory: {music_dir}");
    info!("Database: {db_path}");

    let pool = db::init_pool(&db_path)
        .await
        .expect("Failed to initialize database");

    // Scan music library
    let music_path = PathBuf::from(&music_dir);
    if music_path.exists() {
        library::Library::scan_and_import(&pool, &music_path).await;
    } else {
        info!("Music directory not found, creating: {music_dir}");
        tokio::fs::create_dir_all(&music_path).await.ok();
    }

    let state = AppState {
        db: pool,
        music_dir,
        server_name,
    };

    let app = api::create_router(state);

    let addr: SocketAddr = bind.parse().expect("Invalid bind address");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");

    info!("Server listening on {addr}");

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}
