
1. Remove PUBLIC_PATHS dead constant — it's defined in mod.rs but the auth middleware doesn't reference it (it uses inline path matching). Either remove it or refactor the middleware to use it.
2. Struct naming conventions — All API struct fields use PascalCase (e.g., pub Username, pub ParentId) which triggers 50+ warnings. Either #[allow(non_snake_case)] on the struct or adopt standard snake_case with #[serde(rename)] for JSON serialization.
3. Dead structs — UserRow, ArtistRow, AlbumRow, TrackRow, UserDataRow, SessionRow, PlaylistRow, PlaylistItemRow, AuthUser, PublicSystemInfo, SystemInfo are never constructed (only used for query_as). Could replace with anonymous tuple types or #[allow(dead_code)].
4. Hardcoded admin setup — Admin user creation and password SHA backfill is in db/mod.rs as inline SQL. Could move to a migration file or a dedicated seed script.
Performance & Features
5. Image caching — The image endpoint serves files directly with no memory caching. For frequently requested album covers, consider an in-memory LRU cache to avoid repeated disk reads.
6. No embedded cover art — The scanner only looks for external image files (folder.jpg, etc.). Many audio files have embedded cover art in their tags (APIC/cover art frame). Could extract those as a fallback when no external image exists.
7. No pagination limits — Some endpoints don't enforce limit bounds (e.g., search returns everything by default). Could cap at a sane maximum like 200.
8. Search is basic SQL LIKE — No full-text indexing. For libraries with 10k+ tracks, SQLite FTS5 would be a big improvement for search responsiveness.
9. No graceful shutdown — No signal handling for SIGTERM/SIGINT. Tokio has built-in support via tokio::signal.
Testing
10. No tests — No unit or integration tests for auth, scanning, API endpoints, or streaming.

Config: log, users
transcoding