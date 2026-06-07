use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Rusqlite(#[from] rusqlite::Error),
}

#[derive(Clone, Serialize)]
pub struct MediaRow {
    pub position_sec: f64,
    pub duration_sec: Option<f64>,
    pub playback_speed: f64,
}

pub struct LibraryDb {
    conn: Connection,
}

impl LibraryDb {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS media_state (
                file_key TEXT PRIMARY KEY,
                position_sec REAL NOT NULL DEFAULT 0,
                duration_sec REAL,
                playback_speed REAL NOT NULL DEFAULT 1.0,
                title TEXT,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_media_updated ON media_state(updated_at);
            ",
        )?;
        let _ = conn.execute("ALTER TABLE media_state ADD COLUMN artist TEXT", []);
        let _ = conn.execute("ALTER TABLE media_state ADD COLUMN album TEXT", []);
        let _ = conn.execute("ALTER TABLE media_state ADD COLUMN listened_at INTEGER", []);
        let _ = conn.execute(
            "ALTER TABLE media_state ADD COLUMN file_identity INTEGER",
            [],
        );
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_version (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                version INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_version (id, version) VALUES (1, 0);

            CREATE TABLE IF NOT EXISTS library_roots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                label TEXT NOT NULL,
                content_kind TEXT NOT NULL DEFAULT 'audiobook',
                scan_rule TEXT NOT NULL DEFAULT 'subfolder-is-item',
                scan_subfolders INTEGER NOT NULL DEFAULT 1,
                is_available INTEGER NOT NULL DEFAULT 1,
                last_scan_at INTEGER,
                last_scan_status TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS collections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                root_id INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                sort_title TEXT NOT NULL,
                layout_kind TEXT NOT NULL DEFAULT 'flat_multi',
                author TEXT,
                narrator TEXT,
                artist TEXT,
                album TEXT,
                series TEXT,
                series_index INTEGER,
                cover_path TEXT,
                listened_at INTEGER,
                is_manual INTEGER NOT NULL DEFAULT 0,
                grouping_rule TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(root_id, title, kind)
            );
            CREATE INDEX IF NOT EXISTS idx_collections_root ON collections(root_id);
            CREATE INDEX IF NOT EXISTS idx_collections_kind ON collections(kind);

            CREATE TABLE IF NOT EXISTS collection_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                path TEXT NOT NULL UNIQUE,
                display_title TEXT NOT NULL,
                label TEXT NOT NULL,
                track_order INTEGER NOT NULL,
                disc_index INTEGER NOT NULL DEFAULT 0,
                track_index INTEGER NOT NULL DEFAULT 0,
                file_size INTEGER NOT NULL DEFAULT 0,
                file_mtime INTEGER NOT NULL DEFAULT 0,
                inode INTEGER,
                partial_hash TEXT,
                unavailable INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cf_collection ON collection_files(collection_id, track_order);

            CREATE TABLE IF NOT EXISTS user_playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                kind TEXT NOT NULL DEFAULT 'music',
                is_pinned INTEGER NOT NULL DEFAULT 0,
                cover_path TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS user_playlist_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                playlist_id INTEGER NOT NULL REFERENCES user_playlists(id) ON DELETE CASCADE,
                collection_file_id INTEGER NOT NULL REFERENCES collection_files(id) ON DELETE CASCADE,
                track_order INTEGER NOT NULL,
                added_at INTEGER NOT NULL,
                UNIQUE(playlist_id, collection_file_id)
            );

            CREATE TABLE IF NOT EXISTS metadata_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                lookup_key TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                fetched_at INTEGER NOT NULL,
                UNIQUE(source, lookup_key)
            );
            UPDATE schema_version SET version = 2 WHERE id = 1;
            ",
        )?;
        let _ = conn.execute(
            "ALTER TABLE user_playlists ADD COLUMN default_playback_speed REAL",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE media_state ADD COLUMN speed_custom INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute_batch("UPDATE schema_version SET version = 3 WHERE id = 1;");
        let _ = conn.execute(
            "ALTER TABLE collection_files ADD COLUMN title_manual INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute_batch("UPDATE schema_version SET version = 4 WHERE id = 1;");
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn link_file_identity(&mut self, file_key: &str, identity: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_state SET file_identity = ?1 WHERE file_key = ?2",
            params![identity, file_key],
        )?;
        Ok(())
    }

    pub fn update_media_path(&mut self, old_key: &str, new_key: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_state SET file_key = ?1 WHERE file_key = ?2",
            params![new_key, old_key],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&mut self, key: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM app_settings WHERE key = ?1", [key])?;
        Ok(())
    }

    pub fn get_media(&self, file_key: &str) -> rusqlite::Result<Option<MediaRow>> {
        self.conn
            .query_row(
                "SELECT position_sec, duration_sec, playback_speed FROM media_state WHERE file_key = ?1",
                [file_key],
                |r| {
                    Ok(MediaRow {
                        position_sec: r.get(0)?,
                        duration_sec: r.get(1)?,
                        playback_speed: r.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn is_speed_custom(&self, file_key: &str) -> rusqlite::Result<bool> {
        self.conn
            .query_row(
                "SELECT speed_custom FROM media_state WHERE file_key = ?1",
                [file_key],
                |r| r.get::<_, i32>(0),
            )
            .optional()
            .map(|o| o.map(|v| v != 0).unwrap_or(false))
    }

    /// Returns `(duration_sec, artist, album, listened_at)` when the row exists; `None` if no row.
    pub fn get_media_display_meta(
        &self,
        file_key: &str,
    ) -> rusqlite::Result<Option<(Option<f64>, Option<String>, Option<String>, Option<i64>)>> {
        self.conn
            .query_row(
                "SELECT duration_sec, artist, album, listened_at FROM media_state WHERE file_key = ?1",
                [file_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
    }

    /// Toggle the per-track "listened" flag. `listened_at=Some(ts)` marks it done; `None` clears it.
    pub fn set_listened_at(
        &mut self,
        file_key: &str,
        listened_at: Option<i64>,
        now_unix: i64,
    ) -> rusqlite::Result<()> {
        let playback_speed = self
            .get_media(file_key)
            .ok()
            .flatten()
            .map(|m| m.playback_speed)
            .unwrap_or_else(|| self.get_default_speed());
        let position_sec = self
            .get_media(file_key)
            .ok()
            .flatten()
            .map(|m| m.position_sec)
            .unwrap_or(0.0);
        let duration_sec = self
            .get_media(file_key)
            .ok()
            .flatten()
            .and_then(|m| m.duration_sec);
        self.conn.execute(
            "INSERT INTO media_state (file_key, position_sec, duration_sec, playback_speed, updated_at, listened_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(file_key) DO UPDATE SET
                listened_at = excluded.listened_at,
                updated_at = excluded.updated_at",
            params![file_key, position_sec, duration_sec, playback_speed, now_unix, listened_at],
        )?;
        Ok(())
    }

    /// Remove the per-file row (used when the file is deleted from disk).
    pub fn delete_media_row(&mut self, file_key: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM media_state WHERE file_key = ?1", [file_key])?;
        Ok(())
    }

    /// Merge tag fields into an existing row (or insert a stub row if missing).
    pub fn merge_media_tags(
        &mut self,
        file_key: &str,
        artist: Option<&str>,
        album: Option<&str>,
        now_unix: i64,
    ) -> rusqlite::Result<()> {
        let playback_speed = self
            .get_media(file_key)
            .ok()
            .flatten()
            .map(|m| m.playback_speed)
            .unwrap_or_else(|| self.get_default_speed());
        let position_sec = self
            .get_media(file_key)
            .ok()
            .flatten()
            .map(|m| m.position_sec)
            .unwrap_or(0.0);
        let duration_sec = self
            .get_media(file_key)
            .ok()
            .flatten()
            .and_then(|m| m.duration_sec);
        self.conn.execute(
            "INSERT INTO media_state (file_key, position_sec, duration_sec, playback_speed, updated_at, artist, album)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(file_key) DO UPDATE SET
                artist = COALESCE(excluded.artist, media_state.artist),
                album = COALESCE(excluded.album, media_state.album),
                updated_at = excluded.updated_at",
            params![
                file_key,
                position_sec,
                duration_sec,
                playback_speed,
                now_unix,
                artist,
                album,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_position_duration(
        &mut self,
        file_key: &str,
        position_sec: f64,
        duration_sec: Option<f64>,
        now_unix: i64,
    ) -> rusqlite::Result<()> {
        let playback_speed = self
            .get_media(file_key)
            .ok()
            .flatten()
            .map(|m| m.playback_speed)
            .unwrap_or(1.0);
        self.conn.execute(
            "INSERT INTO media_state (file_key, position_sec, duration_sec, playback_speed, speed_custom, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)
             ON CONFLICT(file_key) DO UPDATE SET
                position_sec = excluded.position_sec,
                duration_sec = COALESCE(excluded.duration_sec, media_state.duration_sec),
                updated_at = excluded.updated_at",
            params![file_key, position_sec, duration_sec, playback_speed, now_unix],
        )?;
        Ok(())
    }

    pub fn upsert_speed(&mut self, file_key: &str, speed: f64, now_unix: i64) -> rusqlite::Result<()> {
        let position_sec = self
            .get_media(file_key)
            .ok()
            .flatten()
            .map(|m| m.position_sec)
            .unwrap_or(0.0);
        let duration_sec = self
            .get_media(file_key)
            .ok()
            .flatten()
            .and_then(|m| m.duration_sec);
        self.conn.execute(
            "INSERT INTO media_state (file_key, position_sec, duration_sec, playback_speed, speed_custom, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(file_key) DO UPDATE SET
                playback_speed = excluded.playback_speed,
                speed_custom = 1,
                updated_at = excluded.updated_at",
            params![file_key, position_sec, duration_sec, speed, now_unix],
        )?;
        Ok(())
    }

    pub fn clear_speed_custom(&mut self, file_key: &str, now_unix: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_state SET speed_custom = 0, updated_at = ?1 WHERE file_key = ?2",
            params![now_unix, file_key],
        )?;
        Ok(())
    }

    pub fn get_default_speed(&self) -> f64 {
        self.get_default_speed_audiobook()
    }

    pub fn get_default_speed_audiobook(&self) -> f64 {
        self.parse_speed_setting("default_playback_speed_audiobook")
            .or_else(|| self.parse_speed_setting("default_playback_speed"))
            .unwrap_or(1.5)
    }

    pub fn get_default_speed_music(&self) -> f64 {
        self.parse_speed_setting("default_playback_speed_music").unwrap_or(1.0)
    }

    pub fn set_default_speed_audiobook(&mut self, speed: f64) -> rusqlite::Result<()> {
        self.set_setting("default_playback_speed_audiobook", &format!("{:.6}", speed))
    }

    pub fn set_default_speed_music(&mut self, speed: f64) -> rusqlite::Result<()> {
        self.set_setting("default_playback_speed_music", &format!("{:.6}", speed))
    }

    fn parse_speed_setting(&self, key: &str) -> Option<f64> {
        self.get_setting(key)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    }
}
