use api::AppState;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;

mod api;
mod db;
mod library;

fn find_project_root() -> PathBuf {
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    for _ in 0..10 {
        if dir.join("Cargo.toml").exists() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustfin=debug,tower_http=info".into()),
        )
        .init();

    let project_root = find_project_root();

    let music_dir = std::env::var("MUSIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| project_root.join("music"));
    let db_path = std::env::var("DATABASE_URL")
        .map(PathBuf::from)
        .unwrap_or_else(|_| project_root.join("data").join("music-server.db"));
    let bind = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8096".to_string());
    let server_name = std::env::var("SERVER_NAME").unwrap_or_else(|_| "Rust Music Server".to_string());

    let music_dir_str = music_dir.to_string_lossy().to_string();
    let db_path_str = db_path.to_string_lossy().to_string();

    info!("Starting music server on {bind}");
    info!("Music directory: {music_dir_str}");
    info!("Database: {db_path_str}");

    let pool = db::init_pool(&db_path_str)
        .await
        .expect("Failed to initialize database");

    if music_dir.exists() {
        library::Library::scan_and_import(&pool, &music_dir).await;
    } else {
        info!("Music directory not found, creating: {music_dir_str}");
        tokio::fs::create_dir_all(&music_dir).await.ok();
    }

    let state = AppState {
        db: pool,
        music_dir: music_dir_str,
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
