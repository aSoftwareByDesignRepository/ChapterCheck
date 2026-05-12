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
        Ok(Self { conn })
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

    /// Returns `(duration_sec, artist, album)` when the row exists; `None` if no row.
    pub fn get_media_display_meta(
        &self,
        file_key: &str,
    ) -> rusqlite::Result<Option<(Option<f64>, Option<String>, Option<String>)>> {
        self.conn
            .query_row(
                "SELECT duration_sec, artist, album FROM media_state WHERE file_key = ?1",
                [file_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
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
            .unwrap_or_else(|| self.get_default_speed());
        self.conn.execute(
            "INSERT INTO media_state (file_key, position_sec, duration_sec, playback_speed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
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
            "INSERT INTO media_state (file_key, position_sec, duration_sec, playback_speed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(file_key) DO UPDATE SET
                playback_speed = excluded.playback_speed,
                updated_at = excluded.updated_at",
            params![file_key, position_sec, duration_sec, speed, now_unix],
        )?;
        Ok(())
    }

    pub fn get_default_speed(&self) -> f64 {
        self.get_setting("default_playback_speed")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(1.0)
    }

    pub fn set_default_speed(&mut self, speed: f64) -> rusqlite::Result<()> {
        self.set_setting("default_playback_speed", &format!("{:.6}", speed))
    }
}
