mod db;
#[cfg(target_os = "linux")]
mod media_controls;
mod mpv;

use db::{LibraryDb, MediaRow};
use mpv::{MpvController, MpvError};
use serde::{Deserialize, Serialize};
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri_plugin_dialog::DialogExt;

const AUDIO_EXT: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "flac", "ogg", "opus", "wav", "wma", "aiff", "aif", "oga",
];

const SESSION_LAST_ROOT: &str = "session.last_root";
const SESSION_LAST_KIND: &str = "session.last_kind";
const SESSION_LAST_SORT: &str = "session.last_sort";
const SESSION_LAST_TRACK: &str = "session.last_track";
const SESSION_LAST_PLAYING: &str = "session.last_playing";
const SESSION_RECENT_JSON: &str = "session.recent_json";
const PREF_RESUME_PLAYING_ON_LAUNCH: &str = "pref.resume_playing_on_launch";
const PREF_SCAN_SUBFOLDERS: &str = "pref.scan_subfolders";
const PREF_UI_LOCALE: &str = "pref.ui_locale";

/// Headphones / MPRIS "Previous" restart the current track when playback is past this point.
const TRACK_RESTART_SECS: f64 = 3.0;

fn notify_transport_changed(app: &AppHandle) {
    let _ = app.emit("abp:transport-changed", ());
}

fn parse_bool_pref(v: Option<String>) -> bool {
    v.map(|s| {
        let t = s.trim();
        t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
    })
    .unwrap_or(false)
}

fn parse_chapter_list_value(data: &Value) -> Vec<ChapterDto> {
    let Some(arr) = data.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .enumerate()
        .filter_map(|(index, ch)| {
            let obj = ch.as_object()?;
            let title_raw = obj
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let time = obj
                .get("time")
                .and_then(|x| x.as_f64())
                .or_else(|| obj.get("time").and_then(|x| x.as_i64().map(|i| i as f64)))
                .or_else(|| obj.get("start").and_then(|x| x.as_f64()))?;
            if !time.is_finite() || time < 0.0 {
                return None;
            }
            let title = if title_raw.is_empty() {
                format!("Chapter {}", index + 1)
            } else {
                title_raw
            };
            Some(ChapterDto {
                index,
                title,
                time_sec: time,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SortKey {
    NameAsc,
    NameDesc,
    ModifiedDesc,
    ModifiedAsc,
    SizeDesc,
    SizeAsc,
    Random,
}

impl Default for SortKey {
    fn default() -> Self {
        SortKey::NameAsc
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RepeatMode {
    #[default]
    Off,
    /// Loop the current track from the start when it ends.
    One,
    /// After the last track, continue from the first.
    All,
}

impl RepeatMode {
    fn as_str(self) -> &'static str {
        match self {
            RepeatMode::Off => "off",
            RepeatMode::One => "one",
            RepeatMode::All => "all",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("off") || t.eq_ignore_ascii_case("none") {
            return Some(Self::Off);
        }
        if t.eq_ignore_ascii_case("one") || t.eq_ignore_ascii_case("track") {
            return Some(Self::One);
        }
        if t.eq_ignore_ascii_case("all") || t.eq_ignore_ascii_case("queue") {
            return Some(Self::All);
        }
        None
    }
}

impl SortKey {
    fn as_kebab(self) -> &'static str {
        match self {
            SortKey::NameAsc => "name-asc",
            SortKey::NameDesc => "name-desc",
            SortKey::ModifiedDesc => "modified-desc",
            SortKey::ModifiedAsc => "modified-asc",
            SortKey::SizeDesc => "size-desc",
            SortKey::SizeAsc => "size-asc",
            SortKey::Random => "random",
        }
    }

    fn from_kebab(s: &str) -> Option<Self> {
        match s {
            "name-asc" => Some(SortKey::NameAsc),
            "name-desc" => Some(SortKey::NameDesc),
            "modified-desc" => Some(SortKey::ModifiedDesc),
            "modified-asc" => Some(SortKey::ModifiedAsc),
            "size-desc" => Some(SortKey::SizeDesc),
            "size-asc" => Some(SortKey::SizeAsc),
            "random" => Some(SortKey::Random),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct PlaylistItemDto {
    pub path: String,
    pub label: String,
    pub duration_sec: Option<f64>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// True when the user has marked this track as listened/finished.
    pub listened: bool,
}

#[derive(Clone, Serialize)]
pub struct PlaylistDto {
    pub root: String,
    pub items: Vec<PlaylistItemDto>,
    pub sort: SortKey,
    /// True when the queue order is **random** (same as `sort == Random`).
    pub shuffled: bool,
}

#[derive(Clone, Serialize)]
pub struct RecentOpenDto {
    pub path: String,
    pub kind: String,
    pub label: String,
}

#[derive(Clone, Serialize)]
pub struct TransportDto {
    pub position_sec: f64,
    pub duration_sec: Option<f64>,
    pub paused: bool,
    pub speed: f64,
    pub eof: bool,
    pub idle: bool,
    pub current_index: Option<usize>,
    pub current_path: Option<String>,
    pub playlist_len: usize,
    pub session_root: Option<String>,
    pub mpv_error: Option<String>,
    /// `"off"` | `"one"` | `"all"`
    pub repeat_mode: String,
}

#[derive(Clone, Serialize)]
pub struct ChapterDto {
    pub index: usize,
    pub title: String,
    pub time_sec: f64,
}

#[derive(Clone, Serialize)]
pub struct AppPrefsDto {
    pub resume_playing_on_launch: bool,
    pub scan_subfolders: bool,
    /// `"en"` or `"de"`.
    pub ui_locale: String,
}

#[derive(Clone, Serialize)]
pub struct SetScanFoldersResult {
    pub prefs: AppPrefsDto,
    pub playlist: Option<PlaylistDto>,
}

struct InnerState {
    db: LibraryDb,
    mpv: MpvController,
    session_root: Option<PathBuf>,
    allowed_files: HashSet<PathBuf>,
    playlist: Vec<PathBuf>,
    sort_key: SortKey,
    current_index: Option<usize>,
    /// `true` when the session was opened via single-file flow (not a scanned folder).
    single_file_session: bool,
    /// When true, opening or rescanning a folder includes audio in nested directories (still under session root).
    scan_subfolders: bool,
    repeat_mode: RepeatMode,
}

pub struct AppState {
    inner: Mutex<InnerState>,
}

impl AppState {
    fn new(db: LibraryDb) -> Self {
        let scan_subfolders = parse_bool_pref(db.get_setting(PREF_SCAN_SUBFOLDERS).ok().flatten());
        Self {
            inner: Mutex::new(InnerState {
                db,
                mpv: MpvController::default(),
                session_root: None,
                allowed_files: HashSet::new(),
                playlist: Vec::new(),
                sort_key: SortKey::default(),
                current_index: None,
                single_file_session: false,
                scan_subfolders,
                repeat_mode: RepeatMode::Off,
            }),
        }
    }

    fn persist_on_close(&self) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.persist_current()?;
        let was_playing = g
            .mpv
            .read_transport_state()
            .map(|r| !r.paused && !r.idle && !r.eof)
            .unwrap_or(false);
        g.persist_session_for_close(was_playing)?;
        g.mpv.shutdown();
        Ok(())
    }

    fn try_restore_last_session(&self) -> Result<Option<PlaylistDto>, String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.restore_last_session()
    }

    fn get_recent_opened(&self) -> Result<Vec<RecentOpenDto>, String> {
        let g = self.inner.lock().map_err(|e| e.to_string())?;
        g.read_recent_opened()
    }

    fn clear_recent_opened(&self) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.clear_recent_opened_history()
    }
}

/// Best-effort artist / album from embedded tags (used when the DB row has no tags yet).
fn probe_audio_tags(path: &Path) -> (Option<String>, Option<String>) {
    use lofty::file::TaggedFileExt;
    use lofty::prelude::Accessor;
    let Ok(tagged) = lofty::read_from_path(path) else {
        return (None, None);
    };
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let Some(t) = tag else {
        return (None, None);
    };
    let artist = t
        .artist()
        .map(|c| c.into_owned())
        .filter(|s| !s.trim().is_empty());
    let album = t
        .album()
        .map(|c| c.into_owned())
        .filter(|s| !s.trim().is_empty());
    (artist, album)
}

impl InnerState {
    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn is_audio_file(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| AUDIO_EXT.iter().any(|a| a.eq_ignore_ascii_case(e)))
            .unwrap_or(false)
    }

    fn canonicalize_allowed(path: &Path) -> Result<PathBuf, String> {
        path.canonicalize()
            .map_err(|e| format!("Cannot access path {}: {e}", path.display()))
    }

    fn is_allowed_playback(&self, path: &Path) -> bool {
        self.allowed_files.contains(path)
    }

    fn path_within_session_root(file: &Path, root: &Path) -> bool {
        file.starts_with(root)
    }

    fn scan_dir(root: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
        let root_canon = Self::canonicalize_allowed(root)?;
        if !root_canon.is_dir() {
            return Err("Path is not a folder".into());
        }
        if !recursive {
            let mut out = Vec::new();
            for ent in fs::read_dir(&root_canon).map_err(|e| format!("Cannot read folder: {e}"))? {
                let ent = ent.map_err(|e| format!("Folder entry: {e}"))?;
                let p = ent.path();
                if !p.is_file() {
                    continue;
                }
                if !Self::is_audio_file(&p) {
                    continue;
                }
                let c = Self::canonicalize_allowed(&p)?;
                if Self::path_within_session_root(&c, &root_canon) {
                    out.push(c);
                }
            }
            return Ok(out);
        }

        let mut out = Vec::new();
        let mut stack = vec![root_canon.clone()];
        while let Some(dir) = stack.pop() {
            let read_dir =
                fs::read_dir(&dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;
            for ent in read_dir {
                let ent = ent.map_err(|e| format!("Folder entry: {e}"))?;
                let p = ent.path();
                if p.is_dir() {
                    if let Ok(dir_canon) = Self::canonicalize_allowed(&p) {
                        if Self::path_within_session_root(&dir_canon, &root_canon) {
                            stack.push(dir_canon);
                        }
                    }
                } else if p.is_file() && Self::is_audio_file(&p) {
                    let c = Self::canonicalize_allowed(&p)?;
                    if Self::path_within_session_root(&c, &root_canon) {
                        out.push(c);
                    }
                }
            }
        }
        Ok(out)
    }

    fn sort_paths(paths: &mut Vec<PathBuf>, key: SortKey) {
        if matches!(key, SortKey::Random) {
            if paths.len() >= 2 {
                paths.shuffle(&mut thread_rng());
            }
            return;
        }

        #[derive(Clone)]
        struct Row {
            path: PathBuf,
            modified: i64,
            size: i64,
            name: String,
        }
        let mut rows: Vec<Row> = paths
            .iter()
            .filter_map(|p| {
                let meta = fs::metadata(p).ok()?;
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some(Row {
                    path: p.clone(),
                    modified,
                    size: meta.len() as i64,
                    name,
                })
            })
            .collect();

        match key {
            SortKey::Random => unreachable!("sort_paths: random order handled above"),
            SortKey::NameAsc => rows.sort_by(|a, b| natord::compare(&a.name, &b.name)),
            SortKey::NameDesc => rows.sort_by(|a, b| natord::compare(&b.name, &a.name)),
            SortKey::ModifiedDesc => rows.sort_by(|a, b| b.modified.cmp(&a.modified)),
            SortKey::ModifiedAsc => rows.sort_by(|a, b| a.modified.cmp(&b.modified)),
            SortKey::SizeDesc => rows.sort_by(|a, b| b.size.cmp(&a.size)),
            SortKey::SizeAsc => rows.sort_by(|a, b| a.size.cmp(&b.size)),
        }

        *paths = rows.into_iter().map(|r| r.path).collect();
    }

    fn build_playlist_dto(&mut self) -> Result<PlaylistDto, String> {
        let now = Self::now_unix();
        let root = self
            .session_root
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .ok_or_else(|| "No folder is open".to_string())?;
        let sort_key = self.sort_key;
        let mut items = Vec::new();
        for p in &self.playlist {
            let key_owned = p.to_string_lossy().into_owned();
            let (mut dur, mut artist, mut album, listened_at) = self
                .db
                .get_media_display_meta(&key_owned)
                .map_err(|e| e.to_string())?
                .unwrap_or((None, None, None, None));
            if dur.is_none() {
                dur = self
                    .db
                    .get_media(&key_owned)
                    .ok()
                    .flatten()
                    .and_then(|m| m.duration_sec);
            }
            if artist.is_none() || album.is_none() {
                let (p_artist, p_album) = probe_audio_tags(p);
                if p_artist.is_some() || p_album.is_some() {
                    let _ = self.db.merge_media_tags(
                        &key_owned,
                        p_artist.as_deref(),
                        p_album.as_deref(),
                        now,
                    );
                    artist = artist.or(p_artist);
                    album = album.or(p_album);
                }
            }
            items.push(PlaylistItemDto {
                path: key_owned.clone(),
                label: p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string_lossy().into_owned()),
                duration_sec: dur,
                artist,
                album,
                listened: listened_at.is_some(),
            });
        }
        Ok(PlaylistDto {
            root,
            items,
            sort: sort_key,
            shuffled: matches!(sort_key, SortKey::Random),
        })
    }

    fn set_session_paths(
        &mut self,
        root: PathBuf,
        mut paths: Vec<PathBuf>,
        sort: SortKey,
        single_file_session: bool,
    ) {
        self.sort_key = sort;
        Self::sort_paths(&mut paths, sort);
        self.session_root = Some(root);
        self.allowed_files = paths.iter().cloned().collect();
        self.playlist = paths;
        self.current_index = None;
        self.single_file_session = single_file_session;
    }

    fn clear_stored_session_keys(&mut self) {
        for k in [
            SESSION_LAST_ROOT,
            SESSION_LAST_KIND,
            SESSION_LAST_SORT,
            SESSION_LAST_TRACK,
            SESSION_LAST_PLAYING,
        ] {
            let _ = self.db.delete_setting(k);
        }
    }

    fn push_recent_session(&mut self) -> Result<(), String> {
        let Some(ref root) = self.session_root else {
            return Ok(());
        };
        let (kind, path): (&'static str, String) = if self.single_file_session {
            let Some(p) = self.playlist.get(0) else {
                return Ok(());
            };
            ("file", p.to_string_lossy().into_owned())
        } else {
            ("folder", root.to_string_lossy().into_owned())
        };
        let mut arr: Vec<Value> = self
            .db
            .get_setting(SESSION_RECENT_JSON)
            .map_err(|e| e.to_string())?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        arr.retain(|v| v.get("path").and_then(|p| p.as_str()) != Some(path.as_str()));
        let now = Self::now_unix();
        let mut out = vec![json!({ "kind": kind, "path": path.clone(), "ts": now })];
        out.extend(arr.into_iter().take(14));
        self.db
            .set_setting(
                SESSION_RECENT_JSON,
                &serde_json::to_string(&out).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn persist_session_meta_checkpoint(&mut self) -> Result<(), String> {
        let Some(ref root) = self.session_root else {
            return Ok(());
        };
        self.db
            .set_setting(SESSION_LAST_ROOT, &root.to_string_lossy())
            .map_err(|e| e.to_string())?;
        self.db
            .set_setting(
                SESSION_LAST_KIND,
                if self.single_file_session { "file" } else { "folder" },
            )
            .map_err(|e| e.to_string())?;
        self.db
            .set_setting(SESSION_LAST_SORT, self.sort_key.as_kebab())
            .map_err(|e| e.to_string())?;
        self.push_recent_session()
    }

    fn persist_session_for_close(&mut self, was_playing: bool) -> Result<(), String> {
        if self.session_root.is_none() {
            return Ok(());
        }
        self.persist_session_meta_checkpoint()?;
        if let Some(p) = self.current_path() {
            self.db
                .set_setting(SESSION_LAST_TRACK, &p.to_string_lossy())
                .map_err(|e| e.to_string())?;
        } else {
            self.db
                .set_setting(SESSION_LAST_TRACK, "")
                .map_err(|e| e.to_string())?;
        }
        self.db
            .set_setting(
                SESSION_LAST_PLAYING,
                if was_playing { "1" } else { "0" },
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn touch_session_track_after_play(&mut self) -> Result<(), String> {
        if self.session_root.is_none() {
            return Ok(());
        }
        if let Some(p) = self.current_path() {
            self.db
                .set_setting(SESSION_LAST_TRACK, &p.to_string_lossy())
                .map_err(|e| e.to_string())?;
        }
        self.db
            .set_setting(SESSION_LAST_SORT, self.sort_key.as_kebab())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn restore_last_session(&mut self) -> Result<Option<PlaylistDto>, String> {
        let root_s = match self
            .db
            .get_setting(SESSION_LAST_ROOT)
            .map_err(|e| e.to_string())?
        {
            Some(s) if !s.trim().is_empty() => s,
            _ => return Ok(None),
        };
        let kind = self
            .db
            .get_setting(SESSION_LAST_KIND)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "folder".into());
        let sort = SortKey::from_kebab(
            &self
                .db
                .get_setting(SESSION_LAST_SORT)
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
        )
        .unwrap_or_default();
        let track_saved = self
            .db
            .get_setting(SESSION_LAST_TRACK)
            .map_err(|e| e.to_string())?
            .filter(|s| !s.trim().is_empty());

        if kind == "file" {
            let fp_s = match &track_saved {
                Some(s) if !s.trim().is_empty() => s.trim(),
                _ => return Ok(None),
            };
            let file_path = match Self::canonicalize_allowed(Path::new(fp_s)) {
                Ok(p) => p,
                Err(_) => {
                    self.clear_stored_session_keys();
                    return Ok(None);
                }
            };
            if !file_path.is_file() || !Self::is_audio_file(&file_path) {
                self.clear_stored_session_keys();
                return Ok(None);
            }
            let parent = match file_path.parent().map(Path::to_path_buf) {
                Some(p) => p,
                None => return Ok(None),
            };
            let parent = match Self::canonicalize_allowed(&parent) {
                Ok(p) => p,
                Err(_) => {
                    self.clear_stored_session_keys();
                    return Ok(None);
                }
            };
            self.sort_key = sort;
            self.set_session_paths(parent, vec![file_path], sort, true);
        } else {
            let root = match Self::canonicalize_allowed(Path::new(root_s.trim())) {
                Ok(p) => p,
                Err(_) => {
                    self.clear_stored_session_keys();
                    return Ok(None);
                }
            };
            if !root.is_dir() {
                self.clear_stored_session_keys();
                return Ok(None);
            }
            let paths = match Self::scan_dir(&root, self.scan_subfolders) {
                Ok(p) => p,
                Err(_) => {
                    self.clear_stored_session_keys();
                    return Ok(None);
                }
            };
            if paths.is_empty() {
                return Ok(None);
            }
            self.sort_key = sort;
            self.set_session_paths(root, paths, sort, false);
        }

        let idx = if let Some(ref ts) = track_saved {
            let ts = ts.trim();
            if ts.is_empty() {
                0usize
            } else if let Ok(c) = Self::canonicalize_allowed(Path::new(ts)) {
                self.playlist
                    .iter()
                    .position(|p| p == &c)
                    .unwrap_or(0)
            } else {
                0usize
            }
        } else {
            0usize
        };
        let idx = idx.min(self.playlist.len().saturating_sub(1));

        let resume_on_launch = parse_bool_pref(
            self.db
                .get_setting(PREF_RESUME_PLAYING_ON_LAUNCH)
                .map_err(|e| e.to_string())?,
        );
        let was_saved_playing = self
            .db
            .get_setting(SESSION_LAST_PLAYING)
            .map_err(|e| e.to_string())?
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let autoplay = resume_on_launch && was_saved_playing;

        self.mpv
            .ensure_running()
            .map_err(|e: MpvError| e.to_string())?;
        if self.playlist.is_empty() {
            return Ok(None);
        }
        self.play_path_at_index_with_autoplay(idx, autoplay)?;
        self.touch_session_track_after_play()?;
        self.build_playlist_dto().map(Some)
    }

    fn read_recent_opened(&self) -> Result<Vec<RecentOpenDto>, String> {
        let raw = self
            .db
            .get_setting(SESSION_RECENT_JSON)
            .map_err(|e| e.to_string())?;
        let arr: Vec<Value> = raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut out = Vec::new();
        for v in arr {
            let kind = v
                .get("kind")
                .and_then(|x| x.as_str())
                .unwrap_or("folder")
                .to_string();
            let path = v
                .get("path")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if path.is_empty() {
                continue;
            }
            let label = Path::new(&path)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            out.push(RecentOpenDto { path, kind, label });
        }
        Ok(out)
    }

    fn clear_recent_opened_history(&mut self) -> Result<(), String> {
        self.db
            .delete_setting(SESSION_RECENT_JSON)
            .map_err(|e| e.to_string())
    }

    fn current_path(&self) -> Option<PathBuf> {
        let idx = self.current_index?;
        self.playlist.get(idx).cloned()
    }

    fn clamp_speed(v: f64) -> f64 {
        if !v.is_finite() {
            return 1.0;
        }
        v.clamp(0.5, 4.0)
    }

    fn resume_start_seconds(position_sec: f64, duration_sec: Option<f64>) -> f64 {
        if !position_sec.is_finite() || position_sec < 0.0 {
            return 0.0;
        }
        if let Some(d) = duration_sec.filter(|d| d.is_finite() && *d > 0.0) {
            let threshold = (d - 2.0).max(0.0);
            if position_sec >= threshold {
                return 0.0;
            }
        }
        position_sec
    }

    fn persist_current(&mut self) -> Result<(), String> {
        let Some(path) = self.current_path() else {
            return Ok(());
        };
        let key = path.to_string_lossy();
        let pos = self.mpv.time_pos_lenient();
        let dur = self.mpv.duration_lenient();
        self.db
            .upsert_position_duration(&key, pos, dur, Self::now_unix())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn play_path_at_index(&mut self, idx: usize) -> Result<(), String> {
        self.play_path_at_index_with_autoplay(idx, true)
    }

    fn play_path_at_index_with_autoplay(&mut self, idx: usize, autoplay: bool) -> Result<(), String> {
        let path = self
            .playlist
            .get(idx)
            .cloned()
            .ok_or_else(|| "Invalid playlist index".to_string())?;
        if !self.is_allowed_playback(&path) {
            return Err("Blocked: file is outside the opened folder".to_string());
        }
        let key = path.to_string_lossy().into_owned();
        let media: Option<MediaRow> = self.db.get_media(&key).map_err(|e| e.to_string())?;
        let default_speed = InnerState::clamp_speed(self.db.get_default_speed());
        let speed = media
            .as_ref()
            .map(|m| InnerState::clamp_speed(m.playback_speed))
            .unwrap_or(default_speed);
        let start = Self::resume_start_seconds(
            media.as_ref().map(|m| m.position_sec).unwrap_or(0.0),
            media.as_ref().and_then(|m| m.duration_sec),
        );

        if autoplay {
            self.mpv
                .load_file(&key, start)
                .map_err(|e: MpvError| e.to_string())?;
        } else {
            self.mpv
                .load_file_start_paused(&key, start)
                .map_err(|e: MpvError| e.to_string())?;
        }
        self.mpv
            .set_speed(speed)
            .map_err(|e: MpvError| e.to_string())?;
        if autoplay {
            self.mpv
                .resume()
                .map_err(|e: MpvError| e.to_string())?;
        } else {
            let _ = self.mpv.pause();
        }
        self.current_index = Some(idx);
        self.touch_session_track_after_play()?;
        Ok(())
    }

    fn recover_mpv_engine(&mut self) -> Result<(), String> {
        self.mpv.reset_session();
        self.mpv
            .ensure_running()
            .map_err(|e: MpvError| e.to_string())?;
        if let Some(idx) = self.current_index {
            if idx < self.playlist.len() {
                self.play_path_at_index_with_autoplay(idx, false)?;
            }
        }
        Ok(())
    }

    /// Re-scan the open folder (respecting `scan_subfolders`), rebuild playlist, reload current file paused.
    fn rescan_folder_playlist(&mut self) -> Result<PlaylistDto, String> {
        if self.single_file_session {
            return self.build_playlist_dto();
        }
        let root = self
            .session_root
            .clone()
            .ok_or_else(|| "No folder is open".to_string())?;
        let prev_path = self.current_path();
        let mut paths = Self::scan_dir(&root, self.scan_subfolders)?;
        if paths.is_empty() {
            return Err("No audio files found in this folder".into());
        }
        let sort = self.sort_key;
        Self::sort_paths(&mut paths, sort);
        self.session_root = Some(root.clone());
        self.allowed_files = paths.iter().cloned().collect();
        self.playlist = paths;
        let idx = prev_path
            .and_then(|prev| self.playlist.iter().position(|p| p == &prev))
            .unwrap_or(0)
            .min(self.playlist.len().saturating_sub(1));
        self.current_index = if self.playlist.is_empty() {
            None
        } else {
            Some(idx)
        };
        self.mpv
            .ensure_running()
            .map_err(|e: MpvError| e.to_string())?;
        self.play_path_at_index_with_autoplay(idx, false)?;
        self.persist_session_meta_checkpoint()?;
        self.build_playlist_dto()
    }

    fn set_track_listened_inner(
        &mut self,
        path: PathBuf,
        listened: bool,
    ) -> Result<PlaylistDto, String> {
        let canon = InnerState::canonicalize_allowed(&path).unwrap_or(path);
        if !self.allowed_files.contains(&canon) {
            return Err("Track is not part of the current session".into());
        }
        let key = canon.to_string_lossy().into_owned();
        let now = Self::now_unix();
        let when = if listened { Some(now) } else { None };
        self.db
            .set_listened_at(&key, when, now)
            .map_err(|e| e.to_string())?;
        self.build_playlist_dto()
    }

    fn mark_session_listened_inner(&mut self, listened: bool) -> Result<PlaylistDto, String> {
        if self.session_root.is_none() {
            return Err("No session is open".into());
        }
        let now = Self::now_unix();
        let when = if listened { Some(now) } else { None };
        let keys: Vec<String> = self
            .playlist
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        for k in &keys {
            self.db
                .set_listened_at(k, when, now)
                .map_err(|e| e.to_string())?;
        }
        self.build_playlist_dto()
    }

    /// Delete one tracked file from disk. Updates queue, advances playback if it was current.
    /// Returns `None` if the session became empty (caller should treat as fully closed).
    fn delete_track_inner(&mut self, path: PathBuf) -> Result<Option<PlaylistDto>, String> {
        let canon = InnerState::canonicalize_allowed(&path).unwrap_or(path);
        if !self.allowed_files.contains(&canon) {
            return Err("Track is not part of the current session".into());
        }
        let key = canon.to_string_lossy().into_owned();
        let was_current = self.current_path().as_ref() == Some(&canon);

        if was_current {
            let _ = self.persist_current();
            self.mpv.reset_session();
        }

        if let Err(e) = fs::remove_file(&canon) {
            if was_current {
                let _ = self.mpv.ensure_running();
                if let Some(idx) = self.current_index {
                    if idx < self.playlist.len() {
                        let _ = self.play_path_at_index_with_autoplay(idx, false);
                    }
                }
            }
            return Err(format!("Cannot delete file: {e}"));
        }
        let _ = self.db.delete_media_row(&key);

        let removed_idx = self.playlist.iter().position(|p| p == &canon);
        self.allowed_files.remove(&canon);
        if let Some(idx_removed) = removed_idx {
            self.playlist.remove(idx_removed);
            if was_current {
                self.current_index = None;
            } else if let Some(cur) = self.current_index {
                if cur > idx_removed {
                    self.current_index = Some(cur - 1);
                }
            }
        }

        if self.playlist.is_empty() {
            self.session_root = None;
            self.allowed_files.clear();
            self.current_index = None;
            self.single_file_session = false;
            self.mpv.reset_session();
            self.clear_stored_session_keys();
            return Ok(None);
        }

        if was_current {
            let new_idx = removed_idx
                .map(|i| i.min(self.playlist.len() - 1))
                .unwrap_or(0);
            self.mpv
                .ensure_running()
                .map_err(|e: MpvError| e.to_string())?;
            self.play_path_at_index_with_autoplay(new_idx, false)?;
        }

        self.persist_session_meta_checkpoint()?;
        self.build_playlist_dto().map(Some)
    }

    /// Permanently delete every tracked file in the session (and remove the folder if it ends up empty).
    fn delete_session_inner(&mut self) -> Result<(), String> {
        let Some(root) = self.session_root.clone() else {
            return Err("No session is open".into());
        };
        let _ = self.persist_current();
        self.mpv.reset_session();

        let files: Vec<PathBuf> = self.playlist.clone();
        let mut last_err: Option<String> = None;
        for p in &files {
            let key = p.to_string_lossy().into_owned();
            match fs::remove_file(p) {
                Ok(()) => {
                    let _ = self.db.delete_media_row(&key);
                }
                Err(e) => {
                    last_err = Some(format!("{}: {e}", p.display()));
                }
            }
        }

        if !self.single_file_session {
            let _ = fs::remove_dir(&root);
        }

        self.session_root = None;
        self.allowed_files.clear();
        self.playlist.clear();
        self.current_index = None;
        self.single_file_session = false;
        self.clear_stored_session_keys();

        if let Some(msg) = last_err {
            return Err(format!("Some files could not be deleted: {msg}"));
        }
        Ok(())
    }

    /// True when mpv has no file loaded (or is not connected).
    fn mpv_is_idle(&mut self) -> bool {
        self.mpv
            .peek_transport()
            .map(|t| t.idle)
            .unwrap_or(true)
    }

    /// Reload the current track when mpv is idle but a playlist index is selected.
    fn ensure_current_track_loaded(&mut self) -> Result<(), String> {
        let Some(idx) = self.current_index else {
            return Ok(());
        };
        if !self.mpv_is_idle() {
            return Ok(());
        }
        if idx >= self.playlist.len() {
            return Ok(());
        }
        self.mpv
            .ensure_running()
            .map_err(|e: MpvError| e.to_string())?;
        self.play_path_at_index_with_autoplay(idx, false)
    }

    /// Start the first queue item when nothing is selected yet.
    fn start_first_track_if_needed(&mut self) -> Result<(), String> {
        if self.current_index.is_some() || self.playlist.is_empty() {
            return Ok(());
        }
        self.mpv
            .ensure_running()
            .map_err(|e: MpvError| e.to_string())?;
        self.play_path_at_index(0)
    }

    /// Resume the current track, restart after EOF, or begin the queue at track 1.
    fn resume_or_start_playback(&mut self) -> Result<(), String> {
        if self.current_index.is_none() {
            return self.start_first_track_if_needed();
        }
        self.ensure_current_track_loaded()?;
        if self.mpv.eof_reached_lenient() {
            self.mpv
                .seek(0.0)
                .map_err(|e: MpvError| e.to_string())?;
        }
        self.mpv
            .set_pause(false)
            .map_err(|e: MpvError| e.to_string())?;
        Ok(())
    }

    /// Play/Pause toggle shared by UI, native menu, and headphone keys.
    fn toggle_playback_inner(&mut self) -> Result<bool, String> {
        if self.current_index.is_none() {
            if self.playlist.is_empty() {
                return Ok(true);
            }
            self.start_first_track_if_needed()?;
            let _ = self.persist_current();
            return Ok(false);
        }
        self.ensure_current_track_loaded()?;
        if self.mpv.eof_reached_lenient() {
            self.mpv
                .seek(0.0)
                .map_err(|e: MpvError| e.to_string())?;
            self.mpv
                .set_pause(false)
                .map_err(|e: MpvError| e.to_string())?;
            let _ = self.persist_current();
            return Ok(false);
        }
        let paused = self
            .mpv
            .toggle_pause()
            .map_err(|e: MpvError| e.to_string())?;
        let _ = self.persist_current();
        Ok(paused)
    }

    /// Hardware "Play" key — same resume/start semantics as [`resume_or_start_playback`].
    fn media_play_inner(&mut self) -> Result<(), String> {
        self.resume_or_start_playback()?;
        let _ = self.persist_current();
        Ok(())
    }

    fn skip_next_inner(&mut self) -> Result<(), String> {
        let len = self.playlist.len();
        if len == 0 {
            return Ok(());
        }
        if self.current_index.is_none() {
            return self.start_first_track_if_needed();
        }
        let idx = self.current_index.unwrap();
        let next = match self.repeat_mode {
            RepeatMode::All => (idx + 1) % len,
            _ => {
                let n = idx.saturating_add(1);
                if n >= len {
                    return Ok(());
                }
                n
            }
        };
        self.persist_current()?;
        self.play_path_at_index(next)
    }

    fn skip_prev_inner(&mut self) -> Result<(), String> {
        let len = self.playlist.len();
        if len == 0 {
            return Ok(());
        }
        if self.current_index.is_none() {
            return self.start_first_track_if_needed();
        }
        let idx = self.current_index.unwrap();
        self.ensure_current_track_loaded()?;
        let pos = self.mpv.time_pos_lenient();
        if pos > TRACK_RESTART_SECS {
            self.mpv
                .seek(0.0)
                .map_err(|e: MpvError| e.to_string())?;
            self.mpv
                .resume()
                .map_err(|e: MpvError| e.to_string())?;
            let _ = self.persist_current();
            return Ok(());
        }
        let prev = match self.repeat_mode {
            RepeatMode::All => (idx + len - 1) % len,
            _ => {
                if idx == 0 {
                    self.mpv
                        .seek(0.0)
                        .map_err(|e: MpvError| e.to_string())?;
                    let _ = self.persist_current();
                    return Ok(());
                }
                idx - 1
            }
        };
        self.persist_current()?;
        self.play_path_at_index(prev)
    }

    fn seek_delta_inner(&mut self, delta: f64) -> Result<(), String> {
        if !delta.is_finite() {
            return Err("Invalid seek delta".to_string());
        }
        if self.current_index.is_none() {
            self.start_first_track_if_needed()?;
        }
        self.ensure_current_track_loaded()?;
        self.mpv
            .seek_relative(delta)
            .map_err(|e: MpvError| e.to_string())?;
        let _ = self.persist_current();
        Ok(())
    }

    fn seek_seconds_inner(&mut self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid seek position".to_string());
        }
        if self.current_index.is_none() {
            self.start_first_track_if_needed()?;
        }
        self.ensure_current_track_loaded()?;
        self.mpv
            .seek(seconds)
            .map_err(|e: MpvError| e.to_string())?;
        let _ = self.persist_current();
        Ok(())
    }

    /// Build the snapshot the OS media controls display. Reads playback state
    /// passively (never spawns mpv) so it is safe from the background sync loop.
    #[cfg(target_os = "linux")]
    fn os_media_snapshot(&mut self) -> media_controls::OsMediaSnapshot {
        let preview_idx = self.current_index.or_else(|| {
            if self.playlist.is_empty() {
                None
            } else {
                Some(0)
            }
        });
        let Some(idx) = preview_idx else {
            return media_controls::OsMediaSnapshot::stopped();
        };
        let path = match self.playlist.get(idx) {
            Some(p) => p.clone(),
            None => return media_controls::OsMediaSnapshot::stopped(),
        };
        let is_active = self.current_index == Some(idx);
        let key = path.to_string_lossy().into_owned();
        let title = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| key.clone());
        let (mut duration_sec, artist, album, _listened) = self
            .db
            .get_media_display_meta(&key)
            .ok()
            .flatten()
            .unwrap_or((None, None, None, None));

        if !is_active {
            return media_controls::OsMediaSnapshot {
                has_track: true,
                stopped: true,
                playing: false,
                position_sec: 0.0,
                duration_sec,
                title,
                artist,
                album,
                track_key: String::new(),
            };
        }

        let read = self.mpv.peek_transport();
        let (position_sec, paused, eof, idle, mpv_duration) = match read {
            Some(r) => (r.position_sec, r.paused, r.eof, r.idle, r.duration_sec),
            None => (0.0, true, false, true, None),
        };
        if duration_sec.is_none() {
            duration_sec = mpv_duration;
        }

        media_controls::OsMediaSnapshot {
            has_track: true,
            stopped: idle,
            playing: !paused && !eof && !idle,
            position_sec: position_sec.max(0.0),
            duration_sec,
            title,
            artist,
            album,
            track_key: key,
        }
    }

    fn app_prefs(&self) -> AppPrefsDto {
        let ui_raw = self.db.get_setting(PREF_UI_LOCALE).ok().flatten();
        let ui_locale = normalize_ui_locale_code(ui_raw.as_deref());
        AppPrefsDto {
            resume_playing_on_launch: parse_bool_pref(
                self.db
                    .get_setting(PREF_RESUME_PLAYING_ON_LAUNCH)
                    .ok()
                    .flatten(),
            ),
            scan_subfolders: self.scan_subfolders,
            ui_locale,
        }
    }
}

fn normalize_ui_locale_code(raw: Option<&str>) -> String {
    match raw.map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("de") => "de".into(),
        _ => "en".into(),
    }
}

fn data_db_path() -> Result<PathBuf, String> {
    let pd = directories::ProjectDirs::from("com", "chaptercheck", "ChapterCheck")
        .ok_or_else(|| "Cannot resolve application data directory".to_string())?;
    let dir = pd.data_dir();
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create data directory: {e}"))?;
    Ok(dir.join("library.sqlite3"))
}

fn setup_err(msg: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::new(std::io::ErrorKind::Other, msg.into()))
}

#[cfg(desktop)]
fn build_native_menu(handle: &AppHandle, ui_locale: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let de = ui_locale == "de";
    let (m_file, m_playback, m_view, m_help) = if de {
        ("Datei", "Wiedergabe", "Ansicht", "Hilfe")
    } else {
        ("File", "Playback", "View", "Help")
    };
    let (open_folder, open_file, preferences, s_pb_toggle, s_pb_prev, s_pb_next, s_pb_back, s_pb_fwd, s_pb_sleep, s_view_player, s_view_queue, s_help_keys, s_help_about) = if de {
        (
            "Ordner öffnen…",
            "Datei öffnen…",
            "Einstellungen…",
            "Wiedergabe / Pause",
            "Vorheriger Titel",
            "Nächster Titel",
            "30 Sekunden zurück",
            "30 Sekunden vor",
            "Schlaf-Timer…",
            "Zur Wiedergabe scrollen",
            "Zur Warteschlange scrollen",
            "Tastenkürzel…",
            "Über ChapterCheck",
        )
    } else {
        (
            "Open Folder…",
            "Open File…",
            "Preferences…",
            "Play / Pause",
            "Previous Track",
            "Next Track",
            "Back 30 Seconds",
            "Forward 30 Seconds",
            "Sleep Timer…",
            "Scroll to Player",
            "Scroll to Queue",
            "Keyboard Shortcuts…",
            "About ChapterCheck",
        )
    };

    let file_open_folder = MenuItem::with_id(
        handle,
        "abp.file.open_folder",
        open_folder,
        true,
        None::<&str>,
    )?;
    let file_open_file = MenuItem::with_id(handle, "abp.file.open_file", open_file, true, None::<&str>)?;
    let file_preferences = MenuItem::with_id(
        handle,
        "abp.file.preferences",
        preferences,
        true,
        None::<&str>,
    )?;
    let pb_toggle = MenuItem::with_id(handle, "abp.playback.toggle", s_pb_toggle, true, None::<&str>)?;
    let pb_prev = MenuItem::with_id(handle, "abp.playback.prev", s_pb_prev, true, None::<&str>)?;
    let pb_next = MenuItem::with_id(handle, "abp.playback.next", s_pb_next, true, None::<&str>)?;
    let pb_back = MenuItem::with_id(handle, "abp.playback.back30", s_pb_back, true, None::<&str>)?;
    let pb_fwd = MenuItem::with_id(handle, "abp.playback.forward30", s_pb_fwd, true, None::<&str>)?;
    let pb_sleep = MenuItem::with_id(handle, "abp.playback.sleep_timer", s_pb_sleep, true, None::<&str>)?;
    let view_player = MenuItem::with_id(handle, "abp.view.player", s_view_player, true, None::<&str>)?;
    let view_queue = MenuItem::with_id(handle, "abp.view.queue", s_view_queue, true, None::<&str>)?;
    let help_keys = MenuItem::with_id(handle, "abp.help.shortcuts", s_help_keys, true, None::<&str>)?;
    let help_about = MenuItem::with_id(handle, "abp.help.about", s_help_about, true, None::<&str>)?;

    Menu::with_items(handle, &[
        &Submenu::with_items(handle, m_file, true, &[
            &file_open_folder,
            &file_open_file,
            &file_preferences,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::close_window(handle, None)?,
        ])?,
        &Submenu::with_items(handle, m_playback, true, &[
            &pb_toggle,
            &pb_prev,
            &pb_next,
            &PredefinedMenuItem::separator(handle)?,
            &pb_back,
            &pb_fwd,
            &PredefinedMenuItem::separator(handle)?,
            &pb_sleep,
        ])?,
        &Submenu::with_items(handle, m_view, true, &[&view_player, &view_queue])?,
        &Submenu::with_items(handle, m_help, true, &[&help_keys, &help_about])?,
    ])
}

#[cfg(desktop)]
#[derive(Clone, serde::Serialize)]
struct UiActionPayload {
    action: &'static str,
}

#[cfg(desktop)]
fn handle_native_menu_event(app: &AppHandle, id: &str) {
    match id {
        "abp.file.open_folder" => {
            let h = app.clone();
            tauri::async_runtime::spawn(async move {
                match pick_open_folder(h.clone()).await {
                    Ok(Some(dto)) => {
                        let _ = h.emit("abp:playlist-update", dto);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = h.emit("abp:user-error", e);
                    }
                }
            });
        }
        "abp.file.open_file" => {
            let h = app.clone();
            tauri::async_runtime::spawn(async move {
                match pick_open_file(h.clone()).await {
                    Ok(Some(dto)) => {
                        let _ = h.emit("abp:playlist-update", dto);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = h.emit("abp:user-error", e);
                    }
                }
            });
        }
        "abp.file.preferences" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "app.preferences",
                },
            );
        }
        "abp.playback.toggle" => match toggle_pause(app.clone()) {
            Ok(_) => notify_transport_changed(&app),
            Err(e) => {
                let _ = app.emit("abp:user-error", e);
            }
        },
        "abp.playback.prev" => match skip_prev(app.clone()) {
            Ok(()) => notify_transport_changed(&app),
            Err(e) => {
                let _ = app.emit("abp:user-error", e);
            }
        },
        "abp.playback.next" => match skip_next(app.clone()) {
            Ok(()) => notify_transport_changed(&app),
            Err(e) => {
                let _ = app.emit("abp:user-error", e);
            }
        },
        "abp.playback.back30" => match seek_delta(app.clone(), -30.0) {
            Ok(()) => notify_transport_changed(&app),
            Err(e) => {
                let _ = app.emit("abp:user-error", e);
            }
        },
        "abp.playback.forward30" => match seek_delta(app.clone(), 30.0) {
            Ok(()) => notify_transport_changed(&app),
            Err(e) => {
                let _ = app.emit("abp:user-error", e);
            }
        },
        "abp.playback.sleep_timer" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "playback.sleep_timer",
                },
            );
        }
        "abp.view.player" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "view.player",
                },
            );
        }
        "abp.view.queue" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "view.queue",
                },
            );
        }
        "abp.help.shortcuts" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "help.shortcuts",
                },
            );
        }
        "abp.help.about" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "help.about",
                },
            );
        }
        _ => {}
    }
}

#[tauri::command]
async fn pick_open_folder(app: AppHandle) -> Result<Option<PlaylistDto>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Open audiobook folder")
        .blocking_pick_folder();
    let Some(fp) = folder else {
        return Ok(None);
    };
    let folder = fp
        .into_path()
        .map_err(|e| format!("Could not use selected folder path: {e}"))?;
    open_folder_path_impl(app, folder).await
}

#[tauri::command]
async fn pick_open_file(app: AppHandle) -> Result<Option<PlaylistDto>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter(
            "Audio",
            &[
                "mp3", "m4a", "m4b", "aac", "flac", "ogg", "opus", "wav", "wma", "aiff", "aif",
                "oga",
            ],
        )
        .set_title("Open audio file")
        .blocking_pick_file();
    let Some(fp) = file else {
        return Ok(None);
    };
    let file_path = fp
        .into_path()
        .map_err(|e| format!("Could not use selected file path: {e}"))?;
    open_file_path_impl(app, file_path).await
}

async fn open_folder_path_impl(app: AppHandle, folder: PathBuf) -> Result<Option<PlaylistDto>, String> {
    let state = app.state::<AppState>();
    let root = InnerState::canonicalize_allowed(&folder)?;
    if !root.is_dir() {
        return Err("Selected path is not a folder".to_string());
    }
    let scan = {
        let g = state.inner.lock().map_err(|e| e.to_string())?;
        g.scan_subfolders
    };
    let paths = InnerState::scan_dir(&root, scan)?;
    let sort = {
        let g = state.inner.lock().map_err(|e| e.to_string())?;
        g.sort_key
    };
    {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.mpv.ensure_running().map_err(|e: MpvError| e.to_string())?;
        let _ = g.persist_current();
        g.set_session_paths(root, paths, sort, false);
        g.persist_session_meta_checkpoint()?;
    }
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.build_playlist_dto()?
    };
    Ok(Some(dto))
}

async fn open_file_path_impl(app: AppHandle, file_path: PathBuf) -> Result<Option<PlaylistDto>, String> {
    let state = app.state::<AppState>();
    let file_path = InnerState::canonicalize_allowed(&file_path)?;
    if !file_path.is_file() {
        return Err("Selected path is not a file".to_string());
    }
    if !InnerState::is_audio_file(&file_path) {
        return Err("Selected file is not a supported audio type".to_string());
    }
    let parent = file_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Cannot determine parent folder".to_string())?;
    let parent = InnerState::canonicalize_allowed(&parent)?;
    let sort = {
        let g = state.inner.lock().map_err(|e| e.to_string())?;
        g.sort_key
    };
    {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.mpv.ensure_running().map_err(|e: MpvError| e.to_string())?;
        let _ = g.persist_current();
        g.set_session_paths(parent, vec![file_path], sort, true);
        g.persist_session_meta_checkpoint()?;
    }
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.build_playlist_dto()?
    };
    Ok(Some(dto))
}

#[tauri::command]
fn resort_playlist(app: AppHandle, sort: SortKey) -> Result<PlaylistDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    if g.session_root.is_none() {
        return Err("Open a folder or file first".to_string());
    }
    let playing_path = g.current_index.and_then(|i| g.playlist.get(i).cloned());
    g.sort_key = sort;
    InnerState::sort_paths(&mut g.playlist, sort);
    if let Some(p) = playing_path {
        g.current_index = g.playlist.iter().position(|x| x == &p);
    }
    g.db
        .set_setting(SESSION_LAST_SORT, sort.as_kebab())
        .map_err(|e| e.to_string())?;
    g.persist_session_meta_checkpoint()?;
    g.build_playlist_dto()
}

#[tauri::command]
fn set_repeat_mode(app: AppHandle, mode: String) -> Result<(), String> {
    let mode = RepeatMode::parse(&mode).ok_or_else(|| "Invalid repeat mode (use off, one, or all)".to_string())?;
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.repeat_mode = mode;
    Ok(())
}

#[tauri::command]
fn play_index(app: AppHandle, index: usize) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.mpv.ensure_running().map_err(|e: MpvError| e.to_string())?;
    let _ = g.persist_current();
    g.play_path_at_index(index)?;
    Ok(())
}

#[tauri::command]
fn toggle_pause(app: AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.toggle_playback_inner()
}

#[tauri::command]
fn set_paused(app: AppHandle, paused: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    if paused {
        // Ignore stray Pause/Stop signals when nothing is loaded — do not spawn mpv.
        if g.current_index.is_some() || !g.mpv_is_idle() {
            g.mpv
                .set_pause(true)
                .map_err(|e: MpvError| e.to_string())?;
            let _ = g.persist_current();
        }
    } else {
        g.resume_or_start_playback()?;
        let _ = g.persist_current();
    }
    Ok(())
}

/// Hardware "Play" key entry point (resume, or start a loaded queue).
#[cfg(target_os = "linux")]
fn media_play(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.media_play_inner()
}

/// Hardware "Play/Pause" toggle key entry point.
#[cfg(target_os = "linux")]
fn media_toggle(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.toggle_playback_inner().map(|_| ())
}

#[tauri::command]
fn seek_seconds(app: AppHandle, seconds: f64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.seek_seconds_inner(seconds)
}

#[tauri::command]
fn seek_delta(app: AppHandle, delta: f64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.seek_delta_inner(delta)
}

#[tauri::command]
fn set_speed(app: AppHandle, speed: f64) -> Result<f64, String> {
    let speed = InnerState::clamp_speed(speed);
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.mpv
        .set_speed(speed)
        .map_err(|e: MpvError| e.to_string())?;
    if let Some(path) = g.current_path() {
        let key = path.to_string_lossy();
        g.db
            .upsert_speed(&key, speed, InnerState::now_unix())
            .map_err(|e| e.to_string())?;
    }
    let _ = g.persist_current();
    Ok(speed)
}

#[tauri::command]
fn set_default_playback_speed(app: AppHandle, speed: f64) -> Result<f64, String> {
    let speed = InnerState::clamp_speed(speed);
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.db
        .set_default_speed(speed)
        .map_err(|e| e.to_string())?;
    g.mpv
        .set_speed(speed)
        .map_err(|e: MpvError| e.to_string())?;
    if let Some(path) = g.current_path() {
        let key = path.to_string_lossy();
        g.db
            .upsert_speed(&key, speed, InnerState::now_unix())
            .map_err(|e| e.to_string())?;
    }
    let _ = g.persist_current();
    Ok(speed)
}

#[tauri::command]
fn save_progress(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.persist_current()
}

#[tauri::command]
fn get_transport(app: AppHandle) -> Result<TransportDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let mut mpv_error = None;
    let (position_sec, duration_sec, paused, speed, eof, idle) = match g.mpv.read_transport_state() {
        Ok(r) => (
            r.position_sec,
            r.duration_sec,
            r.paused,
            r.speed,
            r.eof,
            r.idle,
        ),
        Err(e) => {
            mpv_error = Some(e.to_string());
            (0.0, None, true, 1.0, false, true)
        }
    };
    let current_path = g
        .current_path()
        .map(|p| p.to_string_lossy().into_owned());
    let session_root = g
        .session_root
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    Ok(TransportDto {
        position_sec,
        duration_sec,
        paused,
        speed,
        eof,
        idle,
        current_index: g.current_index,
        current_path,
        playlist_len: g.playlist.len(),
        session_root,
        mpv_error,
        repeat_mode: g.repeat_mode.as_str().to_string(),
    })
}

#[tauri::command]
fn advance_after_eof(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    if !g.mpv.eof_reached_lenient() {
        return Ok(());
    }
    let Some(idx) = g.current_index else {
        return Ok(());
    };
    let len = g.playlist.len();
    if len == 0 {
        return Ok(());
    }
    g.persist_current()?;
    match g.repeat_mode {
        RepeatMode::One => {
            g.mpv
                .seek(0.0)
                .map_err(|e: MpvError| e.to_string())?;
            let _ = g.mpv.resume();
        }
        RepeatMode::All => {
            let next = (idx + 1) % len;
            g.play_path_at_index(next)?;
        }
        RepeatMode::Off => {
            let next = idx.saturating_add(1);
            if next < len {
                g.play_path_at_index(next)?;
            } else {
                let _ = g.mpv.pause();
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn skip_next(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.skip_next_inner()
}

#[tauri::command]
fn skip_prev(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.skip_prev_inner()
}

#[tauri::command]
fn get_recent_opened(app: AppHandle) -> Result<Vec<RecentOpenDto>, String> {
    app.state::<AppState>().get_recent_opened()
}

#[tauri::command]
fn clear_recent_opened(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>().clear_recent_opened()
}

#[tauri::command]
async fn reopen_recent(app: AppHandle, path: String, kind: String) -> Result<Option<PlaylistDto>, String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err("Empty path".to_string());
    }
    let kind = kind.trim().to_lowercase();
    if kind == "file" {
        open_file_path_impl(app, PathBuf::from(path)).await
    } else if kind == "folder" {
        open_folder_path_impl(app, PathBuf::from(path)).await
    } else {
        Err("Unknown kind; expected folder or file".to_string())
    }
}

#[tauri::command]
fn get_app_prefs(app: AppHandle) -> Result<AppPrefsDto, String> {
    let state = app.state::<AppState>();
    let g = state.inner.lock().map_err(|e| e.to_string())?;
    Ok(g.app_prefs())
}

#[tauri::command]
fn set_resume_playing_on_launch(app: AppHandle, enabled: bool) -> Result<AppPrefsDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.db
        .set_setting(
            PREF_RESUME_PLAYING_ON_LAUNCH,
            if enabled { "1" } else { "0" },
        )
        .map_err(|e| e.to_string())?;
    Ok(g.app_prefs())
}

#[tauri::command]
fn set_scan_subfolders(app: AppHandle, enabled: bool) -> Result<SetScanFoldersResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.db
        .set_setting(PREF_SCAN_SUBFOLDERS, if enabled { "1" } else { "0" })
        .map_err(|e| e.to_string())?;
    g.scan_subfolders = enabled;
    let playlist = if g.session_root.is_some() && !g.single_file_session {
        Some(g.rescan_folder_playlist()?)
    } else {
        None
    };
    Ok(SetScanFoldersResult {
        prefs: g.app_prefs(),
        playlist,
    })
}

#[tauri::command]
fn set_ui_locale(app: AppHandle, locale: String) -> Result<AppPrefsDto, String> {
    let code = if locale.trim().eq_ignore_ascii_case("de") {
        "de"
    } else {
        "en"
    };
    {
        let state = app.state::<AppState>();
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.db
            .set_setting(PREF_UI_LOCALE, code)
            .map_err(|e| e.to_string())?;
    }
    #[cfg(desktop)]
    {
        let menu = build_native_menu(&app, code).map_err(|e| e.to_string())?;
        app.set_menu(menu).map_err(|e| e.to_string())?;
    }
    let state = app.state::<AppState>();
    let g = state.inner.lock().map_err(|e| e.to_string())?;
    Ok(g.app_prefs())
}

#[tauri::command]
fn recover_mpv(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.recover_mpv_engine()
}

#[tauri::command]
fn set_track_listened(
    app: AppHandle,
    path: String,
    listened: bool,
) -> Result<PlaylistDto, String> {
    let p = path.trim();
    if p.is_empty() {
        return Err("Empty path".into());
    }
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.set_track_listened_inner(PathBuf::from(p), listened)
}

#[tauri::command]
fn mark_session_listened(app: AppHandle, listened: bool) -> Result<PlaylistDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.mark_session_listened_inner(listened)
}

#[tauri::command]
fn delete_track_file(app: AppHandle, path: String) -> Result<Option<PlaylistDto>, String> {
    let p = path.trim();
    if p.is_empty() {
        return Err("Empty path".into());
    }
    let state = app.state::<AppState>();
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.delete_track_inner(PathBuf::from(p))?
    };
    Ok(dto)
}

#[tauri::command]
fn delete_session_files(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.delete_session_inner()
}

#[tauri::command]
fn get_chapters(app: AppHandle) -> Result<Vec<ChapterDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let raw = g
        .mpv
        .get_property_json("chapter-list")
        .map_err(|e| e.to_string())?;
    Ok(match raw {
        Some(v) => parse_chapter_list_value(&v),
        None => Vec::new(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());

    #[cfg(desktop)]
    {
        builder = builder.on_menu_event(|app, event| {
            handle_native_menu_event(app, event.id.as_ref());
        });
    }

    builder
        .setup(|app: &mut tauri::App| {
            let db_path = data_db_path().map_err(|e| setup_err(e))?;
            let db = LibraryDb::open(&db_path).map_err(|e| setup_err(e.to_string()))?;
            app.manage(AppState::new(db));
            #[cfg(desktop)]
            {
                let h = app.handle().clone();
                let loc = {
                    let st = h.state::<AppState>();
                    let g = st.inner.lock().map_err(|e| setup_err(e.to_string()))?;
                    normalize_ui_locale_code(
                        g.db
                            .get_setting(PREF_UI_LOCALE)
                            .map_err(|e| setup_err(e.to_string()))?
                            .as_deref(),
                    )
                };
                let menu = build_native_menu(&h, &loc).map_err(|e| setup_err(e.to_string()))?;
                app
                    .set_menu(menu)
                    .map_err(|e| setup_err(e.to_string()))?;
            }
            let h = app.handle().clone();
            match h.state::<AppState>().try_restore_last_session() {
                Ok(Some(dto)) => {
                    let _ = h.emit("abp:playlist-update", &dto);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("ChapterCheck: session restore skipped: {e}");
                }
            }
            #[cfg(target_os = "linux")]
            media_controls::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<AppState>();
                let _ = state.persist_on_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            pick_open_folder,
            pick_open_file,
            resort_playlist,
            play_index,
            toggle_pause,
            set_paused,
            seek_seconds,
            seek_delta,
            set_speed,
            set_default_playback_speed,
            get_transport,
            save_progress,
            advance_after_eof,
            set_repeat_mode,
            skip_next,
            skip_prev,
            get_recent_opened,
            clear_recent_opened,
            reopen_recent,
            get_app_prefs,
            set_resume_playing_on_launch,
            set_scan_subfolders,
            set_ui_locale,
            recover_mpv,
            get_chapters,
            set_track_listened,
            mark_session_listened,
            delete_track_file,
            delete_session_files,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
