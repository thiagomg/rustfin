use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::AppState;

const PUBLIC_PATHS: &[&str] = &[
    "/System/Info/Public",
    "/Users/AuthenticateByName",
    "/Users/authenticatebyname",
];

#[derive(Deserialize)]
pub struct LoginRequest {
    pub Username: String,
    #[serde(alias = "PW")]
    pub Pw: Option<String>,
    pub Password: Option<String>,
}

pub async fn authenticate_by_name(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let raw_body = String::from_utf8_lossy(&body);
    tracing::info!("Raw login body: {}", raw_body);

    let body: LoginRequest = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to parse login request: {e}");
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"Status": "Error", "Message": "Invalid JSON"})),
            ));
        }
    };

    tracing::info!(
        "Login attempt: username='{}' Pw_present={} Pw_len={} Password_present={} Password_len={}",
        body.Username,
        body.Pw.is_some(),
        body.Pw.as_ref().map(|s| s.len()).unwrap_or(0),
        body.Password.is_some(),
        body.Password.as_ref().map(|s| s.len()).unwrap_or(0),
    );

    tracing::info!("Login attempt: username='{}'", body.Username);

    let user: Option<(String, String, String)> = match sqlx::query_as(
        "SELECT id, password_hash, password_sha FROM users WHERE username = ?",
    )
    .bind(&body.Username)
    .fetch_optional(&state.db)
    .await
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Database error during login: {e}");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"Status": "Error", "Message": "Database error"})),
            ));
        }
    };

    let (user_id, password_hash, password_sha) = match user {
        Some(u) => u,
        None => {
            tracing::warn!("Login failed: user '{}' not found", body.Username);
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"Status": "Unauthorized", "Message": "Invalid username or password"})),
            ))
        }
    };

    let password_ok = if let Some(pw) = &body.Pw {
        if !pw.is_empty() {
            if pw.len() == 64 && pw.chars().all(|c| c.is_ascii_hexdigit()) {
                pw.eq_ignore_ascii_case(&password_sha)
            } else {
                bcrypt::verify(pw, &password_hash).unwrap_or(false)
            }
        } else {
            false
        }
    } else if let Some(password) = &body.Password {
        if !password.is_empty() {
            bcrypt::verify(password, &password_hash).unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    };

    if !password_ok {
        tracing::warn!("Login failed: wrong password for '{}'", body.Username);
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"Status": "Unauthorized", "Message": "Invalid username or password"})),
        ));
    }

    tracing::info!("Login successful: user='{}'", body.Username);

    let access_token = Uuid::new_v4().to_string();
    let session_id = Uuid::new_v4().to_string();

    let emby_auth = headers
        .get("X-Emby-Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (client, device, device_id) = parse_emby_auth(&emby_auth);

    tracing::debug!("Creating session: user={}, client={}, device={}", user_id, client, device);

    if let Err(e) = sqlx::query(
        "INSERT INTO sessions (id, user_id, access_token, client, device_name, device_id) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&session_id)
    .bind(&user_id)
    .bind(&access_token)
    .bind(&client)
    .bind(&device)
    .bind(&device_id)
    .execute(&state.db)
    .await
    {
        tracing::error!("Failed to create session: {e}");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"Status": "Error", "Message": "Failed to create session"})),
        ));
    }

    tracing::info!("Session created: token={}", &access_token[..8]);

    Ok(Json(json!({
        "User": {
            "Id": user_id,
            "Name": body.Username,
            "ServerId": "music-server-1",
            "HasPassword": true,
            "Configuration": {},
            "Policy": {
                "IsAdministrator": true,
                "EnableAllFolders": true,
                "EnableAllDevices": true,
                "EnableAllChannels": true,
                "EnableContentDeletion": false,
                "EnableContentDownloading": true,
                "EnableSync": true,
                "EnableMediaConversion": true,
                "EnabledDevices": [],
                "EnableLiveTvAccess": false,
                "EnableLiveTvManagement": false,
                "EnablePlaybackRemuxing": true,
                "ForceRemoteSourceTranscoding": false,
                "EnableAudioPlaybackTranscoding": true,
                "EnableVideoPlaybackTranscoding": true,
                "EnablePlaybackAudioTranscoding": true,
                "AuthenticationProviderId": "Jellyfin.Server.Implementations.Users.DefaultAuthenticationProvider",
                "PasswordResetProviderId": "Jellyfin.Server.Implementations.Users.DefaultPasswordResetProvider",
            },
            "PrimaryImageTag": null,
        },
        "SessionInfo": {
            "Id": session_id,
            "UserId": user_id,
            "UserName": body.Username,
            "Client": client,
            "DeviceName": device,
            "DeviceId": device_id,
            "IsActive": true,
            "SupportsRemoteControl": true,
        },
        "AccessToken": access_token,
        "ServerId": "music-server-1",
    })))
}

fn parse_emby_auth(header: &str) -> (String, String, String) {
    let mut client = String::new();
    let mut device = String::new();
    let mut device_id = String::new();

    for part in header.split(',') {
        let part = part.trim();
        let value = if let Some(pos) = part.find('=') {
            part[pos + 1..].trim_matches('"').to_string()
        } else {
            continue;
        };
        if part.contains("Client=") {
            client = value;
        } else if part.contains("Device=") && !part.contains("DeviceId=") && !part.contains("DeviceName=") {
            device = value;
        } else if part.contains("DeviceId=") {
            device_id = value;
        }
    }

    (client, device, device_id)
}

pub async fn auth_middleware(
    State(_state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let path = req.uri().path();
    let method = req.method().to_string();

    // Skip auth for public endpoints
    if PUBLIC_PATHS.iter().any(|p| *p == path) {
        tracing::debug!("Skipping auth for public path: {method} {path}");
        return Ok(next.run(req).await);
    }

    tracing::info!("Auth check for: {method} {path}");

    let token = match extract_token(&req) {
        Some(t) => {
            tracing::debug!("Token found: {}...", &t[..8.min(t.len())]);
            t
        }
        None => {
            tracing::warn!("No token found for: {method} {path}");
            tracing::debug!("Headers: {:?}", req.headers());
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"Status": "Unauthorized", "Message": "Missing authorization"})),
            ))
        }
    };

    let session: Option<(String, String)> =
        match sqlx::query_as("SELECT id, user_id FROM sessions WHERE access_token = ?")
            .bind(&token)
            .fetch_optional(&_state.db)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Database error during auth: {e}");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"Status": "Error", "Message": "Database error"})),
                ))
            }
        };

    let (_session_id, user_id) = match session {
        Some(s) => {
            tracing::debug!("Auth valid: user={}", s.1);
            s
        }
        None => {
            tracing::warn!("Invalid token for: {method} {path}");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"Status": "Unauthorized", "Message": "Invalid token"})),
            ))
        }
    };

    Ok(next.run(req).await)
}

fn extract_token(req: &Request) -> Option<String> {
    for header_name in &["X-MediaBrowser-Token", "X-Emby-Token"] {
        if let Some(token) = req
            .headers()
            .get(*header_name)
            .and_then(|v| v.to_str().ok())
        {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    if let Some(auth) = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
    {
        if auth.starts_with("MediaBrowser ") || auth.starts_with("Emby ") {
            if let Some(token_start) = auth.find("Token=") {
                let after_token = &auth[token_start + 6..];
                let token = after_token
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim_matches('"')
                    .trim()
                    .to_string();
                if !token.is_empty() {
                    return Some(token);
                }
            }
        }
    }

    // Check X-Emby-Authorization header (used by some clients)
    if let Some(emby_auth) = req
        .headers()
        .get("X-Emby-Authorization")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token_start) = emby_auth.find("Token=") {
            let after_token = &emby_auth[token_start + 6..];
            let token = after_token
                .split(',')
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .trim()
                .to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }

    if let Some(query) = req.uri().query() {
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("api_key=") {
                let token = urlencoding_decode(value).unwrap_or_default();
                if !token.is_empty() {
                    return Some(token);
                }
            }
        }
    }

    None
}

fn urlencoding_decode(s: &str) -> Option<String> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                let byte = u8::from_str_radix(&hex, 16).ok()?;
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    Some(result)
}

#[derive(Clone)]
pub struct AuthUser {
    pub id: String,
}
