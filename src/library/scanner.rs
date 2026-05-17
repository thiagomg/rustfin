use lofty::file::TaggedFileExt;
use lofty::prelude::{AudioFile, ItemKey};
use lofty::probe::Probe;
use std::path::Path;
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct TrackMetadata {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub year: Option<i32>,
    pub duration: f64,
    pub file_path: String,
    pub mime_type: String,
    pub bitrate: Option<i64>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
}

#[derive(Debug)]
pub struct ScanResult {
    pub tracks: Vec<TrackMetadata>,
    pub artists: Vec<(String, String)>,
    pub albums: Vec<(String, String, String, Option<i32>, Option<String>)>,
    pub errors: Vec<String>,
}

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "m4a", "aac", "wma", "wav", "opus", "aiff", "ape"];

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

fn mime_type_from_ext(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("mp3") => "audio/mpeg".into(),
        Some("flac") => "audio/flac".into(),
        Some("ogg") => "audio/ogg".into(),
        Some("opus") => "audio/opus".into(),
        Some("m4a") | Some("aac") => "audio/mp4".into(),
        Some("wma") => "audio/x-ms-wma".into(),
        Some("wav") => "audio/wav".into(),
        Some("aiff") => "audio/aiff".into(),
        Some("ape") => "audio/x-ape".into(),
        _ => "audio/mpeg".into(),
    }
}

pub fn scan_directory(path: &Path) -> ScanResult {
    let mut tracks = Vec::new();
    let mut artist_set = std::collections::HashSet::new();
    let mut album_set = std::collections::HashSet::new();
    let mut album_dirs: std::collections::HashMap<(String, String, Option<i32>), PathBuf> = std::collections::HashMap::new();
    let mut errors: Vec<String> = Vec::new();

    for entry in WalkDir::new(path).follow_links(true) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("Walk error: {e}"));
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        if !is_audio_file(file_path) {
            continue;
        }

        let file_path_str = file_path.to_string_lossy().to_string();

        match scan_single_file(file_path) {
            Some(meta) => {
                artist_set.insert(meta.artist.clone());
                artist_set.insert(meta.album_artist.clone());
                let album_key = (meta.album.clone(), meta.album_artist.clone(), meta.year);
                album_set.insert(album_key.clone());
                album_dirs.entry(album_key).or_insert_with(|| {
                    file_path.parent().unwrap_or(file_path).to_path_buf()
                });
                tracks.push(meta);
            }
            None => {
                errors.push(format!("Failed to scan: {file_path_str}"));
            }
        }
    }

    let mut artists: Vec<(String, String)> = artist_set
        .iter()
        .map(|name| (Uuid::new_v4().to_string(), name.clone()))
        .collect();

    fn find_cover_art(dir: &Path) -> Option<String> {
        let patterns = &["folder.jpg", "cover.jpg", "Folder.jpg", "Cover.jpg",
                         "folder.png", "cover.png", "albumart.jpg",
                         "front.jpg", "front.png", "AlbumArtSmall.jpg"];
        for name in patterns {
            let candidate = dir.join(name);
            if candidate.exists() {
                return candidate.to_str().map(|s| s.to_string());
            }
        }
        None
    }

    let mut albums = Vec::new();
    for (name, artist_name, year) in &album_set {
        let id = Uuid::new_v4().to_string();
        let artist_id = find_or_create_artist(&mut artists, artist_name);
        let image_path = album_dirs.get(&(name.clone(), artist_name.clone(), *year))
            .and_then(|dir| find_cover_art(dir));
        albums.push((id, name.clone(), artist_id, *year, image_path));
    }

    let resolved_tracks: Vec<TrackMetadata> = tracks
        .into_iter()
        .map(|track| {
            let _artist_id = find_or_create_artist(&mut artists, &track.artist);
            let album_artist_id = find_or_create_artist(&mut artists, &track.album_artist);
            let _album_id = find_or_create_album(&mut albums, &track.album, &album_artist_id, track.year);

            TrackMetadata {
                id: Uuid::new_v4().to_string(),
                title: track.title,
                artist: track.artist,
                album: track.album,
                album_artist: track.album_artist,
                track_number: track.track_number,
                disc_number: track.disc_number,
                year: track.year,
                duration: track.duration,
                file_path: track.file_path,
                mime_type: track.mime_type,
                bitrate: track.bitrate,
                sample_rate: track.sample_rate,
                channels: track.channels,
            }
        })
        .collect();

    info!("Found {} artists, {} albums, {} tracks", artists.len(), albums.len(), resolved_tracks.len());

    ScanResult {
        tracks: resolved_tracks,
        artists,
        albums,
        errors,
    }
}

fn find_or_create_artist(artists: &mut Vec<(String, String)>, name: &str) -> String {
    if let Some((id, _)) = artists.iter().find(|(_, a)| a == name) {
        return id.clone();
    }
    let id = Uuid::new_v4().to_string();
    artists.push((id.clone(), name.to_string()));
    id
}

fn find_or_create_album(
    albums: &mut Vec<(String, String, String, Option<i32>, Option<String>)>,
    name: &str,
    artist_id: &str,
    year: Option<i32>,
) -> String {
    if let Some((id, _, _, _, _)) = albums
        .iter()
        .find(|(_, n, a_id, _, _)| n == name && a_id == artist_id)
    {
        return id.clone();
    }
    let id = Uuid::new_v4().to_string();
    albums.push((id.clone(), name.to_string(), artist_id.to_string(), year, None));
    id
}

fn scan_single_file(path: &Path) -> Option<TrackMetadata> {
    let file = Probe::open(path).ok()?.read().ok()?;
    let properties = file.properties();
    let tag = file.tags().first()?;

    let title = tag
        .get_string(&ItemKey::TrackTitle)
        .unwrap_or("Unknown Title")
        .to_string();
    let artist = tag
        .get_string(&ItemKey::TrackArtist)
        .unwrap_or("Unknown Artist")
        .to_string();
    let album_name = tag
        .get_string(&ItemKey::AlbumTitle)
        .unwrap_or("Unknown Album")
        .to_string();
    let album_artist = tag
        .get_string(&ItemKey::AlbumArtist)
        .unwrap_or(&artist)
        .to_string();

    let track_number = tag
        .get_string(&ItemKey::TrackNumber)
        .and_then(|s: &str| s.parse::<i32>().ok());

    let disc_number = tag
        .get_string(&ItemKey::DiscNumber)
        .and_then(|s: &str| s.parse::<i32>().ok());

    let year = tag
        .get_string(&ItemKey::Year)
        .and_then(|s: &str| s.parse::<i32>().ok());

    let duration = properties.duration().as_secs_f64();
    let bitrate = properties.audio_bitrate();
    let sample_rate = properties.sample_rate().map(|s| s as i32);
    let channels = properties.channels().map(|c| c as i32);

    let mime_type = mime_type_from_ext(path);
    let file_path = path.to_string_lossy().to_string();

    Some(TrackMetadata {
        id: String::new(),
        title,
        album: album_name,
        album_artist,
        track_number,
        disc_number,
        year,
        duration,
        artist,
        file_path,
        mime_type,
        bitrate: bitrate.map(|b| b as i64),
        sample_rate,
        channels,
    })
}
