mod catalog;
mod db;
#[cfg(target_os = "linux")]
mod media_controls;
mod mpv;
mod path_policy;

use path_policy::tracked_file_on_disk;

use catalog::{
    AddLibraryRootInput, AddToLibraryInput, CatalogService, CollectionMetadataInput,
    fetch_metadata_online, MetadataLookupPlan, MetadataSuggestionDto, PREF_ONLINE_METADATA,
    PREF_REPEAT_MODE, SESSION_COLLECTION_ID, SESSION_PLAYBACK_KIND, SESSION_PLAYLIST_ID,
};
use db::{LibraryDb, MediaRow};
use mpv::{MpvController, MpvError};
use serde::{Deserialize, Serialize};
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
const PREF_PLAYLIST_SHUFFLE: &str = "pref.playlist_shuffle";
const PREF_UI_LOCALE: &str = "pref.ui_locale";

/// Headphones / MPRIS "Previous" restart the current track when playback is past this point.
const TRACK_RESTART_SECS: f64 = 3.0;

/// Ensures session persistence runs at most once per process exit.
static CLOSE_PERSISTED: AtomicBool = AtomicBool::new(false);

fn notify_transport_changed(app: &AppHandle) {
    let _ = app.emit("abp:transport-changed", ());
    #[cfg(target_os = "linux")]
    media_controls::nudge();
}

fn parse_bool_pref(v: Option<String>) -> bool {
    v.map(|s| {
        let t = s.trim();
        t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
    })
    .unwrap_or(false)
}

fn resolve_playlist_shuffle(shuffle: Option<bool>, db: &LibraryDb) -> bool {
    shuffle.unwrap_or_else(|| {
        parse_bool_pref(
            db.get_setting(PREF_PLAYLIST_SHUFFLE)
                .ok()
                .flatten(),
        )
    })
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
    /// Library file id when this queue item matches a catalog track.
    pub collection_file_id: Option<i64>,
    /// True when the catalog track is marked missing (moved / not found on rescan).
    pub library_missing: bool,
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
    pub playback_kind: String,
    pub active_collection_id: Option<i64>,
    pub active_collection_kind: Option<String>,
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
    pub playlist_shuffle_on_play: bool,
    pub online_metadata_enabled: bool,
    /// `"en"` or `"de"`.
    pub ui_locale: String,
    pub default_speed_audiobook: f64,
    pub default_speed_music: f64,
}

#[derive(Clone, Serialize)]
pub struct SetScanFoldersResult {
    pub prefs: AppPrefsDto,
    pub playlist: Option<PlaylistDto>,
}

#[derive(Clone, Serialize)]
pub struct EnqueueCollectionResult {
    pub playlist: PlaylistDto,
    pub tracks_added: usize,
    pub collection_title: String,
    /// True when there was no active queue and a new session was created.
    pub session_started: bool,
    /// True when playback began automatically (new session + play-next).
    pub autoplay_started: bool,
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
    active_collection_id: Option<i64>,
    active_playlist_id: Option<i64>,
    /// `"audiobook"` or `"music"` for context-aware UI.
    playback_kind: String,
}

pub struct AppState {
    inner: Mutex<InnerState>,
    scan_in_progress: AtomicBool,
}

fn with_scan_flag<R>(app: &AppHandle, f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let state = app.state::<AppState>();
    state.scan_in_progress.store(true, Ordering::SeqCst);
    let _ = app.emit("abp:scan-status", true);
    let result = f();
    state.scan_in_progress.store(false, Ordering::SeqCst);
    let _ = app.emit("abp:scan-status", false);
    result
}

impl AppState {
    fn new(db: LibraryDb) -> Self {
        let scan_subfolders = parse_bool_pref(db.get_setting(PREF_SCAN_SUBFOLDERS).ok().flatten());
        let repeat_mode = db
            .get_setting(PREF_REPEAT_MODE)
            .ok()
            .flatten()
            .and_then(|s| RepeatMode::parse(&s))
            .unwrap_or(RepeatMode::Off);
        let active_collection_id = db
            .get_setting(SESSION_COLLECTION_ID)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok());
        let playback_kind = db
            .get_setting(SESSION_PLAYBACK_KIND)
            .ok()
            .flatten()
            .unwrap_or_else(|| "audiobook".into());
        let active_playlist_id = db
            .get_setting(SESSION_PLAYLIST_ID)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok());
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
                repeat_mode,
                active_collection_id,
                active_playlist_id,
                playback_kind,
            }),
            scan_in_progress: AtomicBool::new(false),
        }
    }

    fn persist_on_close(&self) -> Result<(), String> {
        if CLOSE_PERSISTED.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        g.persist_current()?;
        let was_playing = g.session_is_actively_playing();
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
            let file_ref = CatalogService::new(&mut self.db)
                .find_collection_file_ref_by_path(p)
                .ok()
                .flatten();
            let on_disk = tracked_file_on_disk(p).is_some();
            let library_missing = match file_ref {
                Some((_, db_missing)) => db_missing || !on_disk,
                None => !on_disk,
            };
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
                collection_file_id: file_ref.map(|(id, _)| id),
                library_missing,
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
        self.active_collection_id = None;
        self.active_playlist_id = None;
        self.playback_kind = "session".to_string();
        let _ = self.db.delete_setting(SESSION_COLLECTION_ID);
        let _ = self.db.delete_setting(SESSION_PLAYLIST_ID);
        let _ = self.db.set_setting(SESSION_PLAYBACK_KIND, "session");
    }

    /// Load a session queue preserving path order (for catalog / playlist playback).
    fn set_session_paths_ordered(
        &mut self,
        root: PathBuf,
        paths: Vec<PathBuf>,
        single_file_session: bool,
        collection_id: Option<i64>,
        playlist_id: Option<i64>,
        playback_kind: &str,
    ) {
        self.session_root = Some(root);
        self.allowed_files = paths.iter().cloned().collect();
        self.playlist = paths;
        self.current_index = None;
        self.single_file_session = single_file_session;
        self.active_collection_id = collection_id;
        self.active_playlist_id = playlist_id;
        self.playback_kind = playback_kind.to_string();
        if let Some(id) = collection_id {
            let _ = self
                .db
                .set_setting(SESSION_COLLECTION_ID, &id.to_string());
        } else {
            let _ = self.db.delete_setting(SESSION_COLLECTION_ID);
        }
        if let Some(id) = playlist_id {
            let _ = self.db.set_setting(SESSION_PLAYLIST_ID, &id.to_string());
        } else {
            let _ = self.db.delete_setting(SESSION_PLAYLIST_ID);
        }
        let _ = self
            .db
            .set_setting(SESSION_PLAYBACK_KIND, playback_kind);
    }

    fn restore_current_index_after_paths_change(&mut self, previous: Option<&PathBuf>) {
        if self.playlist.is_empty() {
            self.current_index = None;
            return;
        }
        let Some(prev) = previous else {
            return;
        };
        if let Some(idx) = self.playlist.iter().position(|p| p == prev) {
            self.current_index = Some(idx);
            return;
        }
        let prev_s = prev.to_string_lossy();
        if let Some(idx) = self
            .playlist
            .iter()
            .position(|p| p.to_string_lossy() == prev_s)
        {
            self.current_index = Some(idx);
            return;
        }
        if let Some(cur) = self.current_index {
            self.current_index = Some(cur.min(self.playlist.len().saturating_sub(1)));
        }
    }

    fn refresh_active_session_from_source(&mut self) -> Result<bool, String> {
        let playing_path = self.current_index.and_then(|i| self.playlist.get(i).cloned());
        if let Some(collection_id) = self.active_collection_id {
            let (paths, root) =
                CatalogService::new(&mut self.db).collection_playback_paths(collection_id)?;
            if paths.is_empty() {
                return Ok(false);
            }
            self.session_root = Some(root);
            self.allowed_files = paths.iter().cloned().collect();
            self.playlist = paths;
            self.restore_current_index_after_paths_change(playing_path.as_ref());
            return Ok(true);
        }
        if let Some(playlist_id) = self.active_playlist_id {
            let paths =
                CatalogService::new(&mut self.db).playlist_playback_paths(playlist_id)?;
            if paths.is_empty() {
                return Ok(false);
            }
            let root = paths[0]
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| paths[0].clone());
            let root = Self::canonicalize_allowed(&root)?;
            self.session_root = Some(root);
            self.allowed_files = paths.iter().cloned().collect();
            self.playlist = paths;
            self.restore_current_index_after_paths_change(playing_path.as_ref());
            return Ok(true);
        }
        Ok(false)
    }

    fn replace_session_path(&mut self, old: &PathBuf, new: PathBuf) -> Result<(), String> {
        let playing = self.current_index.and_then(|i| self.playlist.get(i).cloned());
        for p in &mut self.playlist {
            if p == old {
                *p = new.clone();
            }
        }
        self.allowed_files.remove(old);
        self.allowed_files.insert(new.clone());
        self.restore_current_index_after_paths_change(playing.as_ref());
        Ok(())
    }

    fn remove_path_from_session(&mut self, path_s: &str) -> Result<(), String> {
        let pb = PathBuf::from(path_s);
        let old_current = self.current_index;
        let old_playing = old_current.and_then(|i| self.playlist.get(i).cloned());
        let mut removed_before_current = 0usize;
        for (i, p) in self.playlist.iter().enumerate() {
            if p.to_string_lossy() == path_s {
                if let Some(cur) = old_current {
                    if i < cur {
                        removed_before_current += 1;
                    }
                }
            }
        }
        self.playlist.retain(|p| p.to_string_lossy() != path_s);
        self.allowed_files.remove(&pb);
        if self.playlist.is_empty() {
            self.current_index = None;
            return Ok(());
        }
        if let Some(cur) = old_current {
            if old_playing
                .as_ref()
                .map(|p| p.to_string_lossy() == path_s)
                .unwrap_or(false)
            {
                self.current_index = Some(cur.min(self.playlist.len().saturating_sub(1)));
            } else {
                self.current_index = Some(cur.saturating_sub(removed_before_current));
            }
        }
        Ok(())
    }

    fn sync_session_after_relink(&mut self, old_path: &str, new_path: &str) -> Result<(), String> {
        if !self
            .playlist
            .iter()
            .any(|p| p.to_string_lossy() == old_path)
        {
            return Ok(());
        }
        if self.active_collection_id.is_some() || self.active_playlist_id.is_some() {
            if !self.refresh_active_session_from_source()? {
                self.replace_session_path(&PathBuf::from(old_path), PathBuf::from(new_path))?;
            }
        } else {
            self.replace_session_path(&PathBuf::from(old_path), PathBuf::from(new_path))?;
        }
        Ok(())
    }

    fn sync_session_after_remove(&mut self, removed_path: &str) -> Result<(), String> {
        if !self
            .playlist
            .iter()
            .any(|p| p.to_string_lossy() == removed_path)
        {
            return Ok(());
        }
        if self.active_collection_id.is_some() || self.active_playlist_id.is_some() {
            if !self.refresh_active_session_from_source()? {
                self.remove_path_from_session(removed_path)?;
            }
        } else {
            self.remove_path_from_session(removed_path)?;
        }
        Ok(())
    }

    fn session_playlist_if_open(&mut self) -> Result<Option<PlaylistDto>, String> {
        if self.session_root.is_none() {
            return Ok(None);
        }
        self.persist_session_meta_checkpoint()?;
        Ok(Some(self.build_playlist_dto()?))
    }

    fn remove_queue_item_inner(&mut self, path: PathBuf) -> Result<Option<PlaylistDto>, String> {
        let canon = Self::canonicalize_allowed(&path).unwrap_or(path);
        if !self.allowed_files.contains(&canon) {
            return Err("Track is not part of the current session".into());
        }
        self.remove_path_from_session(&canon.to_string_lossy())?;
        if self.playlist.is_empty() {
            self.session_root = None;
            self.allowed_files.clear();
            self.current_index = None;
            self.single_file_session = false;
            self.clear_stored_session_keys();
            return Ok(None);
        }
        self.session_playlist_if_open()
    }

    fn play_collection_inner(
        &mut self,
        collection_id: i64,
        mode: &str,
        shuffle: bool,
        autoplay: bool,
    ) -> Result<PlaylistDto, String> {
        let (mut paths, root, detail) = {
            let mut cat = CatalogService::new(&mut self.db);
            let (paths, root) = cat.collection_playback_paths(collection_id)?;
            let detail = cat.get_collection_detail(collection_id)?;
            (paths, root, detail)
        };
        if shuffle && paths.len() >= 2 {
            paths.shuffle(&mut thread_rng());
        }
        let start_idx = if mode == "continue" {
            let mut idx = 0usize;
            let mut best_pos = -1.0f64;
            for (i, p) in paths.iter().enumerate() {
                let key = p.to_string_lossy();
                if let Ok(Some(row)) = self.db.get_media(&key) {
                    if row.position_sec > best_pos && row.position_sec > 1.0 {
                        if let Some(dur) = row.duration_sec {
                            if row.position_sec < dur - 30.0 {
                                best_pos = row.position_sec;
                                idx = i;
                            }
                        } else {
                            best_pos = row.position_sec;
                            idx = i;
                        }
                    }
                }
            }
            idx
        } else {
            0
        };
        self.mpv.ensure_running().map_err(|e: MpvError| e.to_string())?;
        let _ = self.persist_current();
        let single = paths.len() == 1;
        self.set_session_paths_ordered(
            root,
            paths,
            single,
            Some(collection_id),
            None,
            &detail.kind,
        );
        self.play_path_at_index_with_autoplay(start_idx, autoplay)?;
        self.touch_session_track_after_play()?;
        self.persist_session_meta_checkpoint()?;
        self.build_playlist_dto()
    }

    fn enqueue_collection_inner(
        &mut self,
        collection_id: i64,
        position: &str,
    ) -> Result<EnqueueCollectionResult, String> {
        let (paths, _root, detail) = {
            let mut cat = CatalogService::new(&mut self.db);
            let (paths, root) = cat.collection_playback_paths(collection_id)?;
            let detail = cat.get_collection_detail(collection_id)?;
            (paths, root, detail)
        };
        let mut paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists()).collect();
        if paths.is_empty() {
            return Err("No playable tracks in this item".into());
        }

        let has_active_queue =
            self.session_root.is_some() && !self.playlist.is_empty();
        if !has_active_queue {
            let single = paths.len() == 1;
            self.set_session_paths_ordered(
                _root,
                paths.clone(),
                single,
                Some(collection_id),
                None,
                &detail.kind,
            );
            let autoplay_started = position == "next";
            if autoplay_started {
                self.mpv
                    .ensure_running()
                    .map_err(|e: MpvError| e.to_string())?;
                self.play_path_at_index_with_autoplay(0, true)?;
                self.touch_session_track_after_play()?;
            }
            self.persist_session_meta_checkpoint()?;
            let playlist = self.build_playlist_dto()?;
            return Ok(EnqueueCollectionResult {
                playlist,
                tracks_added: paths.len(),
                collection_title: detail.title,
                session_started: true,
                autoplay_started,
            });
        }

        let existing: HashSet<PathBuf> = self.playlist.iter().cloned().collect();
        paths.retain(|p| !existing.contains(p));
        if paths.is_empty() {
            return Err("This item is already in your queue".into());
        }

        let insert_at = match position {
            "next" => self
                .current_index
                .map(|i| i.saturating_add(1))
                .unwrap_or(0),
            _ => self.playlist.len(),
        };
        let added = paths.len();
        for (offset, path) in paths.iter().enumerate() {
            self.playlist.insert(insert_at + offset, path.clone());
            self.allowed_files.insert(path.clone());
        }
        if let Some(cur) = self.current_index {
            if insert_at <= cur {
                self.current_index = Some(cur + added);
            }
        }

        self.persist_session_meta_checkpoint()?;
        let playlist = self.build_playlist_dto()?;
        Ok(EnqueueCollectionResult {
            playlist,
            tracks_added: added,
            collection_title: detail.title,
            session_started: false,
            autoplay_started: false,
        })
    }

    fn play_kind_inner(
        &mut self,
        kind: &str,
        filter: Option<&str>,
        search: Option<&str>,
        shuffle: bool,
        autoplay: bool,
    ) -> Result<PlaylistDto, String> {
        let mut paths =
            CatalogService::new(&mut self.db).kind_playback_paths(kind, filter, search)?;
        if shuffle && paths.len() >= 2 {
            paths.shuffle(&mut thread_rng());
        }
        self.mpv.ensure_running().map_err(|e: MpvError| e.to_string())?;
        let _ = self.persist_current();
        let root = paths[0]
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| paths[0].clone());
        let root = InnerState::canonicalize_allowed(&root)?;
        self.set_session_paths_ordered(root, paths, false, None, None, kind);
        self.play_path_at_index_with_autoplay(0, autoplay)?;
        self.touch_session_track_after_play()?;
        self.persist_session_meta_checkpoint()?;
        self.build_playlist_dto()
    }

    fn enqueue_kind_inner(
        &mut self,
        kind: &str,
        filter: Option<&str>,
        search: Option<&str>,
        position: &str,
    ) -> Result<EnqueueCollectionResult, String> {
        let mut paths =
            CatalogService::new(&mut self.db).kind_playback_paths(kind, filter, search)?;
        paths = paths.into_iter().filter(|p| p.exists()).collect();
        if paths.is_empty() {
            return Err("No playable tracks in your library".into());
        }

        let source_title = match kind {
            "music" => "Music",
            "audiobook" => "Audiobooks",
            _ => kind,
        }
        .to_string();

        let has_active_queue = self.session_root.is_some() && !self.playlist.is_empty();
        if !has_active_queue {
            let root = paths[0]
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| paths[0].clone());
            let root = InnerState::canonicalize_allowed(&root)?;
            self.set_session_paths_ordered(root, paths.clone(), false, None, None, kind);
            let autoplay_started = position == "next";
            if autoplay_started {
                self.mpv
                    .ensure_running()
                    .map_err(|e: MpvError| e.to_string())?;
                self.play_path_at_index_with_autoplay(0, true)?;
                self.touch_session_track_after_play()?;
            }
            self.persist_session_meta_checkpoint()?;
            let playlist = self.build_playlist_dto()?;
            return Ok(EnqueueCollectionResult {
                playlist,
                tracks_added: paths.len(),
                collection_title: source_title,
                session_started: true,
                autoplay_started,
            });
        }

        let existing: HashSet<PathBuf> = self.playlist.iter().cloned().collect();
        paths.retain(|p| !existing.contains(p));
        if paths.is_empty() {
            return Err("This item is already in your queue".into());
        }

        let insert_at = match position {
            "next" => self
                .current_index
                .map(|i| i.saturating_add(1))
                .unwrap_or(0),
            _ => self.playlist.len(),
        };
        let added = paths.len();
        for (offset, path) in paths.iter().enumerate() {
            self.playlist.insert(insert_at + offset, path.clone());
            self.allowed_files.insert(path.clone());
        }
        if let Some(cur) = self.current_index {
            if insert_at <= cur {
                self.current_index = Some(cur + added);
            }
        }

        self.persist_session_meta_checkpoint()?;
        let playlist = self.build_playlist_dto()?;
        Ok(EnqueueCollectionResult {
            playlist,
            tracks_added: added,
            collection_title: source_title,
            session_started: false,
            autoplay_started: false,
        })
    }

    fn clear_stored_session_keys(&mut self) {
        for k in [
            SESSION_LAST_ROOT,
            SESSION_LAST_KIND,
            SESSION_LAST_SORT,
            SESSION_LAST_TRACK,
            SESSION_LAST_PLAYING,
            SESSION_COLLECTION_ID,
            SESSION_PLAYBACK_KIND,
            SESSION_PLAYLIST_ID,
        ] {
            let _ = self.db.delete_setting(k);
        }
        self.active_collection_id = None;
        self.active_playlist_id = None;
        self.playback_kind = "audiobook".to_string();
    }

    fn resolve_playback_speed(&mut self, file_key: &str) -> Result<f64, String> {
        if self.db.is_speed_custom(file_key).map_err(|e| e.to_string())? {
            if let Some(media) = self.db.get_media(file_key).map_err(|e| e.to_string())? {
                return Ok(Self::clamp_speed(media.playback_speed));
            }
        }

        if let Some(pl_id) = self.active_playlist_id {
            let speed = CatalogService::new(&mut self.db).get_playlist_default_speed(pl_id)?;
            if let Some(s) = speed {
                return Ok(Self::clamp_speed(s));
            }
        }

        let kind = self.effective_playback_kind();
        let speed = match kind.as_str() {
            "music" => self.db.get_default_speed_music(),
            _ => self.db.get_default_speed_audiobook(),
        };
        Ok(Self::clamp_speed(speed))
    }

    fn resolved_collection_id(&mut self) -> Option<i64> {
        if let Some(cid) = self.active_collection_id {
            return Some(cid);
        }
        let path = self.current_path()?;
        CatalogService::new(&mut self.db)
            .find_collection_for_path(&path)
            .ok()
            .flatten()
    }

    fn collection_kind(&self, collection_id: i64) -> Option<String> {
        self.db
            .connection()
            .query_row(
                "SELECT kind FROM collections WHERE id = ?1",
                [collection_id],
                |r| r.get::<_, String>(0),
            )
            .ok()
    }

    fn effective_playback_kind(&mut self) -> String {
        if let Some(cid) = self.resolved_collection_id() {
            if let Some(kind) = self.collection_kind(cid) {
                return kind;
            }
        }
        if self.playback_kind == "music" || self.playback_kind == "audiobook" {
            return self.playback_kind.clone();
        }
        "audiobook".to_string()
    }

    fn resolve_playable_index(&mut self, start: usize) -> Result<usize, String> {
        for i in start..self.playlist.len() {
            let p = self.playlist[i].clone();
            if tracked_file_on_disk(&p).is_none() {
                let _ = CatalogService::new(&mut self.db).mark_file_unavailable_by_path(&p);
                continue;
            }
            if self.is_allowed_playback(&p) {
                return Ok(i);
            }
        }
        Err("No playable files in queue".into())
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

    fn session_is_actively_playing(&mut self) -> bool {
        self.mpv
            .peek_transport()
            .map(|r| !r.paused && !r.idle && !r.eof)
            .unwrap_or(false)
    }

    fn sync_session_last_playing(&mut self) -> Result<(), String> {
        if self.session_root.is_none() {
            return Ok(());
        }
        let playing = self.session_is_actively_playing();
        self.db
            .set_setting(SESSION_LAST_PLAYING, if playing { "1" } else { "0" })
            .map_err(|e| e.to_string())?;
        Ok(())
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

    fn launch_autoplay_enabled(&self) -> Result<bool, String> {
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
        Ok(resume_on_launch && was_saved_playing)
    }

    fn restore_last_session(&mut self) -> Result<Option<PlaylistDto>, String> {
        if let Some(collection_id) = self.active_collection_id {
            let autoplay = self.launch_autoplay_enabled()?;
            match self.play_collection_inner(collection_id, "continue", false, autoplay) {
                Ok(dto) => return Ok(Some(dto)),
                Err(_) => {
                    self.active_collection_id = None;
                    let _ = self.db.delete_setting(SESSION_COLLECTION_ID);
                }
            }
        }

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

        let autoplay = self.launch_autoplay_enabled()?;

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
        let idx = self.resolve_playable_index(idx)?;
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
        let speed = self.resolve_playback_speed(&key)?;
        let start = Self::resume_start_seconds(
            media.as_ref().map(|m| m.position_sec).unwrap_or(0.0),
            media.as_ref().and_then(|m| m.duration_sec),
        );

        self.mpv
            .load_file_controlled(&key, start, autoplay)
            .map_err(|e: MpvError| e.to_string())?;
        self.mpv
            .set_speed(speed)
            .map_err(|e: MpvError| e.to_string())?;
        self.current_index = Some(idx);
        self.touch_session_track_after_play()?;
        let _ = self.sync_session_last_playing();
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
        CatalogService::new(&mut self.db).path_delete_allowed(&canon)?;
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
        let _ = CatalogService::new(&mut self.db).mark_file_unavailable_by_path(&canon);

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
        let files: Vec<PathBuf> = self.playlist.clone();
        {
            let cat = CatalogService::new(&mut self.db);
            for p in &files {
                cat.path_delete_allowed(p)?;
            }
            if !self.single_file_session {
                cat.path_delete_allowed(&root)?;
            }
        }
        let _ = self.persist_current();
        self.mpv.reset_session();

        let mut last_err: Option<String> = None;
        for p in &files {
            let key = p.to_string_lossy().into_owned();
            match fs::remove_file(p) {
                Ok(()) => {
                    let _ = self.db.delete_media_row(&key);
                    let _ = CatalogService::new(&mut self.db).mark_file_unavailable_by_path(p);
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
            let _ = self.sync_session_last_playing();
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
            let _ = self.sync_session_last_playing();
            return Ok(false);
        }
        let paused = self
            .mpv
            .toggle_pause()
            .map_err(|e: MpvError| e.to_string())?;
        let _ = self.persist_current();
        let _ = self.sync_session_last_playing();
        Ok(paused)
    }

    /// Hardware "Play" key — same resume/start semantics as [`resume_or_start_playback`].
    fn media_play_inner(&mut self) -> Result<(), String> {
        self.resume_or_start_playback()?;
        let _ = self.persist_current();
        let _ = self.sync_session_last_playing();
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
        let mut title = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| key.clone());
        let (mut duration_sec, mut artist, mut album, _listened) = self
            .db
            .get_media_display_meta(&key)
            .ok()
            .flatten()
            .unwrap_or((None, None, None, None));
        let mut cover_url: Option<String> = None;
        if let Some(cid) = self.resolved_collection_id() {
            if let Ok((t, a, al, cover)) =
                CatalogService::new(&mut self.db).collection_mpris_meta(cid, &key)
            {
                title = t;
                if artist.is_none() {
                    artist = a;
                }
                if album.is_none() {
                    album = al;
                }
                if let Some(c) = cover.filter(|p| Path::new(p).exists()) {
                    cover_url = Some(format!("file://{}", c));
                }
            }
        }

        let has_queue = !self.playlist.is_empty();
        let persisted_pos = self
            .db
            .get_media(&key)
            .ok()
            .flatten()
            .map(|m| m.position_sec)
            .unwrap_or(0.0);

        if !is_active {
            return media_controls::OsMediaSnapshot {
                has_track: has_queue,
                stopped: !has_queue,
                playing: false,
                position_sec: persisted_pos.max(0.0),
                duration_sec,
                title,
                artist,
                album,
                track_key: if has_queue { key } else { String::new() },
                cover_url,
            };
        }

        let read = self.mpv.peek_transport();
        let (mpv_pos, paused, eof, idle, mpv_duration) = match read {
            Some(r) => (r.position_sec, r.paused, r.eof, r.idle, r.duration_sec),
            None => (persisted_pos, true, false, true, None),
        };
        if duration_sec.is_none() {
            duration_sec = mpv_duration;
        }
        let position_sec = if idle {
            persisted_pos
        } else {
            mpv_pos
        }
        .max(0.0);
        // Report Paused (not Stopped) when a track is loaded/selected so the OS routes
        // headset Play/Pause to resume rather than treating the session as ended.
        let stopped = !has_queue || (self.current_index.is_none() && idle);

        media_controls::OsMediaSnapshot {
            has_track: has_queue,
            stopped,
            playing: !paused && !eof && !idle,
            position_sec,
            duration_sec,
            title,
            artist,
            album,
            track_key: key,
            cover_url,
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
            playlist_shuffle_on_play: parse_bool_pref(
                self.db
                    .get_setting(PREF_PLAYLIST_SHUFFLE)
                    .ok()
                    .flatten(),
            ),
            online_metadata_enabled: parse_bool_pref(
                self.db
                    .get_setting(PREF_ONLINE_METADATA)
                    .ok()
                    .flatten(),
            ),
            ui_locale,
            default_speed_audiobook: Self::clamp_speed(self.db.get_default_speed_audiobook()),
            default_speed_music: Self::clamp_speed(self.db.get_default_speed_music()),
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
    let (
        link_folder,
        open_folder,
        open_file,
        preferences,
        s_pb_toggle,
        s_pb_prev,
        s_pb_next,
        s_pb_back,
        s_pb_fwd,
        s_pb_sleep,
        s_view_player,
        s_view_queue,
        s_help_keys,
        s_help_about,
    ) = if de {
        (
            "Ordner verknüpfen…",
            "Ordner einmalig öffnen…",
            "Datei einmalig öffnen…",
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
            "Link folder…",
            "Open folder once…",
            "Open file once…",
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

    let file_link_folder = MenuItem::with_id(
        handle,
        "abp.file.link_folder",
        link_folder,
        true,
        None::<&str>,
    )?;
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
            &file_link_folder,
            &PredefinedMenuItem::separator(handle)?,
            &file_open_folder,
            &file_open_file,
            &PredefinedMenuItem::separator(handle)?,
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
#[derive(Clone, serde::Serialize)]
struct UserSessionOpenPayload {
    playlist: PlaylistDto,
    suggest_library_link: bool,
}

#[cfg(desktop)]
fn emit_user_session_open(app: &AppHandle, playlist: PlaylistDto, suggest_library_link: bool) {
    let _ = app.emit(
        "abp:user-session-open",
        UserSessionOpenPayload {
            playlist,
            suggest_library_link,
        },
    );
}

#[cfg(desktop)]
fn handle_native_menu_event(app: &AppHandle, id: &str) {
    match id {
        "abp.file.link_folder" => {
            let _ = app.emit(
                "abp:ui-action",
                UiActionPayload {
                    action: "library.link_folder",
                },
            );
        }
        "abp.file.open_folder" => {
            let h = app.clone();
            tauri::async_runtime::spawn(async move {
                match pick_open_folder(h.clone()).await {
                    Ok(Some(dto)) => emit_user_session_open(&h, dto, true),
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
                    Ok(Some(dto)) => emit_user_session_open(&h, dto, false),
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
async fn pick_library_folder(app: AppHandle) -> Result<Option<String>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Link audio folder")
        .blocking_pick_folder();
    let Some(fp) = folder else {
        return Ok(None);
    };
    let folder = fp
        .into_path()
        .map_err(|e| format!("Could not use selected folder path: {e}"))?;
    Ok(Some(folder.to_string_lossy().to_string()))
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
    let catalog_match = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        CatalogService::new(&mut g.db).find_collection_for_folder(&root)?
    };
    if let Some(collection_id) = catalog_match {
        let dto = {
            let mut g = state.inner.lock().map_err(|e| e.to_string())?;
            g.play_collection_inner(collection_id, "continue", false, true)?
        };
        let _ = app.emit("abp:playlist-update", &dto);
        return Ok(Some(dto));
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

/// Paths passed when the desktop or file manager opens a file/folder with this app.
fn startup_open_paths() -> Vec<PathBuf> {
    std::env::args()
        .skip(1)
        .filter_map(|arg| decode_open_path_arg(&arg))
        .collect()
}

fn decode_open_path_arg(arg: &str) -> Option<PathBuf> {
    let raw = arg.trim();
    if raw.is_empty() || raw.starts_with('-') {
        return None;
    }
    let path_s = if let Some(rest) = raw.strip_prefix("file://") {
        percent_decode_path(rest)
    } else {
        raw.to_string()
    };
    if path_s.is_empty() {
        return None;
    }
    Some(PathBuf::from(path_s))
}

fn percent_decode_path(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn open_path_from_os(app: AppHandle, path: PathBuf) -> Result<(), String> {
    let opened = if path.is_dir() {
        open_folder_path_impl(app.clone(), path).await?
    } else if path.is_file() {
        open_file_path_impl(app.clone(), path).await?
    } else {
        return Err("Path does not exist".into());
    };
    if let Some(dto) = opened {
        let _ = app.emit("abp:playlist-update", &dto);
    }
    Ok(())
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
    let catalog_match = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        CatalogService::new(&mut g.db).find_collection_for_path(&file_path)?
    };
    if let Some(collection_id) = catalog_match {
        let dto = {
            let mut g = state.inner.lock().map_err(|e| e.to_string())?;
            g.play_collection_inner(collection_id, "continue", false, true)?
        };
        let _ = app.emit("abp:playlist-update", &dto);
        return Ok(Some(dto));
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
    g.db
        .set_setting(PREF_REPEAT_MODE, mode.as_str())
        .map_err(|e| e.to_string())?;
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
    let paused = g.toggle_playback_inner()?;
    notify_transport_changed(&app);
    Ok(paused)
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
    let _ = g.sync_session_last_playing();
    notify_transport_changed(&app);
    Ok(())
}

/// Hardware "Play" key entry point (resume, or start a loaded queue).
#[cfg(target_os = "linux")]
fn media_play(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.media_play_inner()?;
    notify_transport_changed(&app);
    Ok(())
}

/// Hardware "Play/Pause" toggle key entry point.
#[cfg(target_os = "linux")]
fn media_toggle(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.toggle_playback_inner().map(|_| ())?;
    notify_transport_changed(&app);
    Ok(())
}

#[tauri::command]
fn get_os_media_status() -> media_controls::OsMediaStatusDto {
    #[cfg(target_os = "linux")]
    {
        return media_controls::status();
    }
    #[cfg(not(target_os = "linux"))]
    {
        media_controls::OsMediaStatusDto {
            available: false,
            player_name: String::new(),
        }
    }
}

#[tauri::command]
fn seek_seconds(app: AppHandle, seconds: f64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.seek_seconds_inner(seconds)?;
    notify_transport_changed(&app);
    Ok(())
}

#[tauri::command]
fn seek_delta(app: AppHandle, delta: f64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.seek_delta_inner(delta)?;
    notify_transport_changed(&app);
    Ok(())
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
    let kind = g.effective_playback_kind();
    if kind == "music" {
        g.db
            .set_default_speed_music(speed)
            .map_err(|e| e.to_string())?;
    } else {
        g.db
            .set_default_speed_audiobook(speed)
            .map_err(|e| e.to_string())?;
    }
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
fn set_playback_speed_defaults(
    app: AppHandle,
    audiobook: f64,
    music: f64,
) -> Result<AppPrefsDto, String> {
    let audiobook = InnerState::clamp_speed(audiobook);
    let music = InnerState::clamp_speed(music);
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.db
        .set_default_speed_audiobook(audiobook)
        .map_err(|e| e.to_string())?;
    g.db
        .set_default_speed_music(music)
        .map_err(|e| e.to_string())?;
    Ok(g.app_prefs())
}

#[tauri::command]
fn set_playlist_default_speed(
    app: AppHandle,
    playlist_id: i64,
    speed: Option<f64>,
) -> Result<catalog::PlaylistDetailDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let speed = speed.map(InnerState::clamp_speed);
    CatalogService::new(&mut g.db).set_playlist_default_speed(playlist_id, speed)?;
    CatalogService::new(&mut g.db).get_playlist_detail(playlist_id)
}

#[tauri::command]
fn reset_track_speed_to_default(app: AppHandle) -> Result<f64, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let Some(path) = g.current_path() else {
        return Err("Nothing is playing".into());
    };
    let key = path.to_string_lossy().into_owned();
    g.db
        .clear_speed_custom(&key, InnerState::now_unix())
        .map_err(|e| e.to_string())?;
    let speed = g.resolve_playback_speed(&key)?;
    g.mpv
        .set_speed(speed)
        .map_err(|e: MpvError| e.to_string())?;
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
        playback_kind: g.effective_playback_kind(),
        active_collection_id: g.active_collection_id,
        active_collection_kind: g
            .resolved_collection_id()
            .and_then(|id| g.collection_kind(id)),
    })
}

#[tauri::command]
fn get_current_playlist(app: AppHandle) -> Result<Option<PlaylistDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    if g.playlist.is_empty() {
        return Ok(None);
    }
    g.build_playlist_dto().map(Some)
}

#[tauri::command]
fn list_library_roots(app: AppHandle) -> Result<Vec<catalog::LibraryRootDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_roots()
}

#[tauri::command]
fn add_library_root(app: AppHandle, input: AddLibraryRootInput) -> Result<catalog::LibraryRootDto, String> {
    with_scan_flag(&app, || {
        let state = app.state::<AppState>();
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        CatalogService::new(&mut g.db).add_root(input)
    })
}

#[tauri::command]
fn remove_library_root(app: AppHandle, root_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).remove_root(root_id)
}

#[tauri::command]
fn scan_library_root(app: AppHandle, root_id: i64) -> Result<catalog::ScanStatusDto, String> {
    with_scan_flag(&app, || {
        let state = app.state::<AppState>();
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        CatalogService::new(&mut g.db).scan_root(root_id)
    })
}

#[tauri::command]
fn refresh_library_roots(app: AppHandle) -> Result<(), String> {
    with_scan_flag(&app, || {
        let state = app.state::<AppState>();
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        CatalogService::new(&mut g.db).refresh_roots_availability()
    })
}

#[tauri::command]
fn list_collections(
    app: AppHandle,
    kind: Option<String>,
    filter: Option<String>,
    search: Option<String>,
    series: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<catalog::CollectionListPageDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_collections(
        kind.as_deref(),
        filter.as_deref(),
        search.as_deref(),
        series.as_deref(),
        limit.unwrap_or(200),
        offset.unwrap_or(0),
    )
}

#[tauri::command]
fn list_series_names(app: AppHandle, kind: Option<String>) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_series_names(kind.as_deref())
}

#[tauri::command]
fn get_collection_detail(app: AppHandle, collection_id: i64) -> Result<catalog::CollectionDetailDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).get_collection_detail(collection_id)
}

#[tauri::command]
fn find_collection_file_id(app: AppHandle, path: String) -> Result<Option<i64>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).find_collection_file_id_by_path(Path::new(&path))
}

#[tauri::command]
fn find_relax_playlist(app: AppHandle) -> Result<Option<i64>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).find_relax_playlist_id()
}

#[tauri::command]
fn get_home_summary(app: AppHandle) -> Result<catalog::HomeSummaryDto, String> {
    let state = app.state::<AppState>();
    let scanning = state.scan_in_progress.load(Ordering::SeqCst);
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).get_home_summary(scanning)
}

#[tauri::command]
fn play_collection(
    app: AppHandle,
    collection_id: i64,
    mode: String,
    shuffle: Option<bool>,
) -> Result<PlaylistDto, String> {
    let state = app.state::<AppState>();
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.play_collection_inner(collection_id, &mode, shuffle.unwrap_or(false), true)?
    };
    let _ = app.emit("abp:playlist-update", &dto);
    Ok(dto)
}

#[tauri::command]
fn enqueue_collection(
    app: AppHandle,
    collection_id: i64,
    position: Option<String>,
) -> Result<EnqueueCollectionResult, String> {
    let state = app.state::<AppState>();
    let result = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.enqueue_collection_inner(collection_id, position.as_deref().unwrap_or("end"))?
    };
    let _ = app.emit("abp:playlist-update", &result.playlist);
    Ok(result)
}

#[tauri::command]
fn play_kind(
    app: AppHandle,
    kind: String,
    filter: Option<String>,
    search: Option<String>,
    shuffle: Option<bool>,
) -> Result<PlaylistDto, String> {
    catalog::validate_playback_kind(&kind)?;
    let state = app.state::<AppState>();
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.play_kind_inner(
            &kind,
            filter.as_deref(),
            search.as_deref(),
            shuffle.unwrap_or(false),
            true,
        )?
    };
    let _ = app.emit("abp:playlist-update", &dto);
    Ok(dto)
}

#[tauri::command]
fn enqueue_kind(
    app: AppHandle,
    kind: String,
    filter: Option<String>,
    search: Option<String>,
    position: Option<String>,
) -> Result<EnqueueCollectionResult, String> {
    catalog::validate_playback_kind(&kind)?;
    let state = app.state::<AppState>();
    let result = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.enqueue_kind_inner(
            &kind,
            filter.as_deref(),
            search.as_deref(),
            position.as_deref().unwrap_or("end"),
        )?
    };
    let _ = app.emit("abp:playlist-update", &result.playlist);
    Ok(result)
}

#[tauri::command]
fn update_collection_metadata(
    app: AppHandle,
    collection_id: i64,
    metadata: CollectionMetadataInput,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).update_collection_metadata(collection_id, metadata)
}

fn apply_collection_kind_playback_effects(
    g: &mut InnerState,
    collection_id: i64,
    kind: &str,
) -> Result<(), String> {
    let playing_collection = g.resolved_collection_id() == Some(collection_id);
    if playing_collection {
        g.playback_kind = kind.to_string();
        let _ = g.db.set_setting(SESSION_PLAYBACK_KIND, kind);
    }
    let paths = CatalogService::new(&mut g.db).collection_file_paths(collection_id)?;
    let now = InnerState::now_unix();
    for path in &paths {
        let _ = g.db.clear_speed_custom(path, now);
    }
    if playing_collection {
        if let Some(current) = g.current_path() {
            let key = current.to_string_lossy().into_owned();
            let speed = g.resolve_playback_speed(&key)?;
            if g.mpv.ensure_running().is_ok() {
                let _ = g.mpv.set_speed(speed);
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn set_collection_kind(app: AppHandle, collection_id: i64, kind: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).set_collection_kind(collection_id, &kind)?;
    apply_collection_kind_playback_effects(&mut g, collection_id, &kind)?;
    Ok(())
}

#[tauri::command]
fn set_collections_kind(
    app: AppHandle,
    collection_ids: Vec<i64>,
    kind: String,
) -> Result<catalog::SetCollectionsKindResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let result = CatalogService::new(&mut g.db).set_collections_kind(&collection_ids, &kind)?;
    for &id in &collection_ids {
        if result
            .failures
            .iter()
            .any(|failure| failure.collection_id == id)
        {
            continue;
        }
        let _ = apply_collection_kind_playback_effects(&mut g, id, &kind);
    }
    Ok(result)
}

#[tauri::command]
fn list_collection_ids(
    app: AppHandle,
    kind: Option<String>,
    filter: Option<String>,
    search: Option<String>,
) -> Result<Vec<i64>, String> {
    if let Some(ref k) = kind {
        catalog::validate_playback_kind(k)?;
    }
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_collection_ids(
        kind.as_deref(),
        filter.as_deref(),
        search.as_deref(),
    )
}

#[tauri::command]
fn find_collection_id_for_path(app: AppHandle, path: String) -> Result<Option<i64>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).find_collection_for_path(Path::new(&path))
}

#[tauri::command]
fn mark_collection_listened(app: AppHandle, collection_id: i64, listened: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).mark_collection_listened(collection_id, listened)
}

#[tauri::command]
fn list_playlists(app: AppHandle) -> Result<Vec<catalog::PlaylistSummaryDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_playlists()
}

#[tauri::command]
fn create_playlist(app: AppHandle, name: String, pin: Option<bool>) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).create_playlist(&name, pin.unwrap_or(false))
}

#[tauri::command]
fn add_to_playlist(app: AppHandle, playlist_id: i64, collection_file_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).add_to_playlist(playlist_id, collection_file_id)
}

#[tauri::command]
fn list_album_groups(
    app: AppHandle,
    search: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<catalog::AlbumGroupDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_album_groups(
        search.as_deref(),
        limit.unwrap_or(500),
        offset.unwrap_or(0),
    )
}

#[tauri::command]
fn add_album_to_playlist(
    app: AppHandle,
    playlist_id: i64,
    artist: String,
    album: String,
) -> Result<catalog::AddToPlaylistBulkResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).add_album_to_playlist(playlist_id, &artist, &album)
}

#[tauri::command]
fn add_collection_to_playlist(
    app: AppHandle,
    playlist_id: i64,
    collection_id: i64,
) -> Result<catalog::AddToPlaylistBulkResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).add_collection_to_playlist(playlist_id, collection_id)
}

#[tauri::command]
fn create_playlist_from_album(
    app: AppHandle,
    artist: String,
    album: String,
) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).create_playlist_from_album(&artist, &album)
}

#[tauri::command]
fn create_playlist_from_collection(app: AppHandle, collection_id: i64) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).create_playlist_from_collection(collection_id)
}

#[tauri::command]
fn list_metadata_groups(
    app: AppHandle,
    group_kind: String,
    search: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<catalog::MetadataGroupDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).list_metadata_groups(
        &group_kind,
        search.as_deref(),
        limit.unwrap_or(500),
        offset.unwrap_or(0),
    )
}

#[tauri::command]
fn add_metadata_group_to_playlist(
    app: AppHandle,
    playlist_id: i64,
    group_kind: String,
    group_key: String,
) -> Result<catalog::AddToPlaylistBulkResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).add_metadata_group_to_playlist(
        playlist_id,
        &group_kind,
        &group_key,
    )
}

#[tauri::command]
fn create_playlist_from_metadata_group(
    app: AppHandle,
    group_kind: String,
    group_key: String,
) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).create_playlist_from_metadata_group(&group_kind, &group_key)
}

#[tauri::command]
async fn pick_import_folder_to_playlist(
    app: AppHandle,
    playlist_id: i64,
) -> Result<Option<catalog::ImportFolderToPlaylistResult>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Import folder into playlist")
        .blocking_pick_folder();
    let Some(fp) = folder else {
        return Ok(None);
    };
    let folder = fp
        .into_path()
        .map_err(|e| format!("Could not use selected folder path: {e}"))?;
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let result = CatalogService::new(&mut g.db).import_folder_to_playlist(playlist_id, &folder)?;
    Ok(Some(result))
}

#[tauri::command]
fn rename_playlist(app: AppHandle, playlist_id: i64, name: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).rename_playlist(playlist_id, &name)
}

#[tauri::command]
fn delete_playlist(app: AppHandle, playlist_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).delete_playlist(playlist_id)
}

#[tauri::command]
fn get_playlist_detail(
    app: AppHandle,
    playlist_id: i64,
) -> Result<catalog::PlaylistDetailDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).get_playlist_detail(playlist_id)
}

#[tauri::command]
fn remove_from_playlist(app: AppHandle, item_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).remove_from_playlist(item_id)
}

#[tauri::command]
fn reorder_playlist_items(
    app: AppHandle,
    playlist_id: i64,
    item_ids: Vec<i64>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).reorder_playlist_items(playlist_id, item_ids)
}

#[tauri::command]
fn reorder_collection_files(
    app: AppHandle,
    collection_id: i64,
    file_ids: Vec<i64>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).reorder_collection_files(collection_id, file_ids)
}

#[tauri::command]
fn update_file_display_title(
    app: AppHandle,
    file_id: i64,
    display_title: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).update_file_display_title(file_id, &display_title)
}

#[derive(Clone, Serialize)]
pub struct LibraryFileChangeResult {
    pub playlist: Option<PlaylistDto>,
}

#[tauri::command]
fn relink_collection_file(
    app: AppHandle,
    file_id: i64,
    new_path: String,
) -> Result<LibraryFileChangeResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let result = CatalogService::new(&mut g.db).relink_collection_file(file_id, &new_path)?;
    g.sync_session_after_relink(&result.old_path, &result.new_path)?;
    let playlist = g.session_playlist_if_open()?;
    if let Some(ref dto) = playlist {
        let _ = app.emit("abp:playlist-update", dto);
    }
    Ok(LibraryFileChangeResult { playlist })
}

#[tauri::command]
fn remove_collection_file_from_library(
    app: AppHandle,
    file_id: i64,
) -> Result<catalog::RemoveCollectionFileResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let result = CatalogService::new(&mut g.db).remove_collection_file_from_library(file_id)?;
    g.sync_session_after_remove(&result.removed_path)?;
    let playlist = g.session_playlist_if_open()?;
    if let Some(ref dto) = playlist {
        let _ = app.emit("abp:playlist-update", dto);
    }
    Ok(result)
}

#[tauri::command]
fn remove_collection_from_library(
    app: AppHandle,
    collection_id: i64,
) -> Result<catalog::RemoveCollectionResult, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    let result = CatalogService::new(&mut g.db)
        .remove_collection_from_library(collection_id)?;
    for path in &result.removed_paths {
        g.sync_session_after_remove(path)?;
    }
    let playlist = g.session_playlist_if_open()?;
    if let Some(ref dto) = playlist {
        let _ = app.emit("abp:playlist-update", dto);
    }
    Ok(result)
}

#[tauri::command]
fn remove_queue_item(app: AppHandle, path: String) -> Result<Option<PlaylistDto>, String> {
    let state = app.state::<AppState>();
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.remove_queue_item_inner(PathBuf::from(path))?
    };
    if let Some(ref d) = dto {
        let _ = app.emit("abp:playlist-update", d);
    }
    Ok(dto)
}

#[tauri::command]
async fn pick_relink_audio_file(app: AppHandle) -> Result<Option<String>, String> {
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
        .set_title("Choose replacement audio file")
        .blocking_pick_file();
    let Some(fp) = file else {
        return Ok(None);
    };
    let path = fp
        .into_path()
        .map_err(|e| format!("Could not use selected path: {e}"))?;
    Ok(Some(path.to_string_lossy().to_string()))
}

#[tauri::command]
fn fix_collection_track_order(app: AppHandle, collection_id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).fix_collection_track_order(collection_id)
}

#[tauri::command]
fn set_playlist_pinned(app: AppHandle, playlist_id: i64, pinned: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).set_playlist_pinned(playlist_id, pinned)
}

#[tauri::command]
fn get_scan_status(app: AppHandle) -> Result<Vec<catalog::ScanStatusDto>, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).scan_status_all()
}

#[tauri::command]
fn add_to_library(app: AppHandle, input: AddToLibraryInput) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).add_to_library(input)
}

#[tauri::command]
async fn lookup_metadata_online(
    app: AppHandle,
    collection_id: i64,
) -> Result<Vec<MetadataSuggestionDto>, String> {
    let plan = {
        let state = app.state::<AppState>();
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        let enabled = parse_bool_pref(
            g.db
                .get_setting(PREF_ONLINE_METADATA)
                .ok()
                .flatten(),
        );
        CatalogService::new(&mut g.db).plan_metadata_lookup(collection_id, enabled)?
    };

    match plan {
        MetadataLookupPlan::Disabled => {
            Err("Online metadata lookup is disabled in preferences".into())
        }
        MetadataLookupPlan::EmptyTitle => Ok(Vec::new()),
        MetadataLookupPlan::Cached(suggestions) => Ok(suggestions),
        MetadataLookupPlan::Fetch(req) => {
            let cache_key = req.cache_key.clone();
            let suggestions = tauri::async_runtime::spawn_blocking(move || fetch_metadata_online(&req))
                .await
                .map_err(|e| e.to_string())??;

            if !suggestions.is_empty() {
                let state = app.state::<AppState>();
                let mut g = state.inner.lock().map_err(|e| e.to_string())?;
                CatalogService::new(&mut g.db)
                    .store_metadata_lookup_cache(&cache_key, &suggestions)?;
            }
            Ok(suggestions)
        }
    }
}

#[tauri::command]
fn set_online_metadata_enabled(app: AppHandle, enabled: bool) -> Result<AppPrefsDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.db
        .set_setting(
            PREF_ONLINE_METADATA,
            if enabled { "1" } else { "0" },
        )
        .map_err(|e| e.to_string())?;
    Ok(g.app_prefs())
}

#[tauri::command]
fn update_library_root(
    app: AppHandle,
    root_id: i64,
    new_path: String,
) -> Result<catalog::LibraryRootDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    CatalogService::new(&mut g.db).update_root_path(root_id, &new_path)?;
    CatalogService::new(&mut g.db)
        .list_roots()?
        .into_iter()
        .find(|r| r.id == root_id)
        .ok_or_else(|| "Root not found after update".to_string())
}

#[tauri::command]
async fn export_db(app: AppHandle) -> Result<Option<String>, String> {
    let dest = app
        .dialog()
        .file()
        .set_title("Back up library database")
        .add_filter("SQLite database", &["sqlite3", "db", "sqlite"])
        .set_file_name("chaptercheck-library-backup.sqlite3")
        .blocking_save_file();
    let Some(fp) = dest else {
        return Ok(None);
    };
    let dest_path = fp
        .into_path()
        .map_err(|e| format!("Could not use selected path: {e}"))?;
    let src = data_db_path()?;
    fs::copy(&src, &dest_path).map_err(|e| format!("Backup failed: {e}"))?;
    Ok(Some(dest_path.to_string_lossy().to_string()))
}

#[tauri::command]
fn play_playlist(
    app: AppHandle,
    playlist_id: i64,
    shuffle: Option<bool>,
) -> Result<PlaylistDto, String> {
    let state = app.state::<AppState>();
    let (paths, kind) = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        let mut cat = CatalogService::new(&mut g.db);
        let paths = cat.playlist_playback_paths(playlist_id)?;
        let kind = "music".to_string();
        (paths, kind)
    };
    if paths.is_empty() {
        return Err("This playlist has no playable tracks".into());
    }
    let shuffle_play = {
        let state = app.state::<AppState>();
        let g = state.inner.lock().map_err(|e| e.to_string())?;
        resolve_playlist_shuffle(shuffle, &g.db)
    };
    let mut paths = paths;
    if shuffle_play && paths.len() >= 2 {
        paths.shuffle(&mut thread_rng());
    }
    let dto = {
        let mut g = state.inner.lock().map_err(|e| e.to_string())?;
        g.mpv.ensure_running().map_err(|e: MpvError| e.to_string())?;
        let _ = g.persist_current();
        let root = paths[0]
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| paths[0].clone());
        let root = InnerState::canonicalize_allowed(&root)?;
        g.set_session_paths_ordered(root, paths, false, None, Some(playlist_id), &kind);
        g.play_path_at_index_with_autoplay(0, true)?;
        g.touch_session_track_after_play()?;
        g.persist_session_meta_checkpoint()?;
        g.build_playlist_dto()?
    };
    let _ = app.emit("abp:playlist-update", &dto);
    Ok(dto)
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
                match g.resolve_playable_index(next) {
                    Ok(playable) => {
                        g.play_path_at_index(playable)?;
                    }
                    Err(_) => {
                        let _ = g.mpv.pause();
                    }
                }
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
    g.skip_next_inner()?;
    notify_transport_changed(&app);
    Ok(())
}

#[tauri::command]
fn skip_prev(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.skip_prev_inner()?;
    notify_transport_changed(&app);
    Ok(())
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
    if !enabled {
        g.db
            .set_setting(SESSION_LAST_PLAYING, "0")
            .map_err(|e| e.to_string())?;
    }
    Ok(g.app_prefs())
}

#[tauri::command]
fn set_playlist_shuffle_on_play(app: AppHandle, enabled: bool) -> Result<AppPrefsDto, String> {
    let state = app.state::<AppState>();
    let mut g = state.inner.lock().map_err(|e| e.to_string())?;
    g.db
        .set_setting(PREF_PLAYLIST_SHUFFLE, if enabled { "1" } else { "0" })
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
            mpv::cleanup_orphaned_processes();
            let db_path = data_db_path().map_err(|e| setup_err(e))?;
            let db = LibraryDb::open(&db_path).map_err(|e| setup_err(e.to_string()))?;
            app.manage(AppState::new(db));
            {
                let state = app.state::<AppState>();
                let mut g = state.inner.lock().map_err(|e| setup_err(e.to_string()))?;
                let _ = CatalogService::new(&mut g.db).refresh_roots_availability();
            }
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
            let cli_paths = startup_open_paths();
            if let Some(path) = cli_paths.into_iter().find(|p| p.exists()) {
                let open_app = h.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = open_path_from_os(open_app, path).await {
                        eprintln!("ChapterCheck: open from file manager failed: {e}");
                    }
                });
            } else {
                match h.state::<AppState>().try_restore_last_session() {
                    Ok(Some(dto)) => {
                        let _ = h.emit("abp:playlist-update", &dto);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("ChapterCheck: session restore skipped: {e}");
                    }
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
            pick_library_folder,
            pick_open_file,
            resort_playlist,
            play_index,
            toggle_pause,
            set_paused,
            seek_seconds,
            seek_delta,
            set_speed,
            set_default_playback_speed,
            set_playback_speed_defaults,
            set_playlist_default_speed,
            reset_track_speed_to_default,
            get_transport,
            get_current_playlist,
            get_os_media_status,
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
            set_playlist_shuffle_on_play,
            set_scan_subfolders,
            set_ui_locale,
            recover_mpv,
            get_chapters,
            set_track_listened,
            mark_session_listened,
            delete_track_file,
            delete_session_files,
            list_library_roots,
            add_library_root,
            remove_library_root,
            scan_library_root,
            refresh_library_roots,
            list_collections,
            list_series_names,
            get_collection_detail,
            find_collection_file_id,
            find_collection_id_for_path,
            find_relax_playlist,
            get_home_summary,
            play_collection,
            enqueue_collection,
            play_kind,
            enqueue_kind,
            update_collection_metadata,
            set_collection_kind,
            set_collections_kind,
            list_collection_ids,
            mark_collection_listened,
            list_playlists,
            create_playlist,
            add_to_playlist,
            list_album_groups,
            add_album_to_playlist,
            add_collection_to_playlist,
            create_playlist_from_album,
            create_playlist_from_collection,
            list_metadata_groups,
            add_metadata_group_to_playlist,
            create_playlist_from_metadata_group,
            pick_import_folder_to_playlist,
            rename_playlist,
            delete_playlist,
            get_playlist_detail,
            remove_from_playlist,
            reorder_playlist_items,
            reorder_collection_files,
            update_file_display_title,
            relink_collection_file,
            remove_collection_file_from_library,
            remove_collection_from_library,
            remove_queue_item,
            pick_relink_audio_file,
            fix_collection_track_order,
            set_playlist_pinned,
            get_scan_status,
            add_to_library,
            lookup_metadata_online,
            set_online_metadata_enabled,
            update_library_root,
            export_db,
            play_playlist,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let _ = state.persist_on_close();
                }
            }
        });
}

#[cfg(test)]
mod playback_tests {
    use super::*;
    use rusqlite::params;
    use std::fs;
    use std::path::PathBuf;

    fn test_inner(db: LibraryDb) -> InnerState {
        InnerState {
            db,
            mpv: MpvController::default(),
            session_root: None,
            allowed_files: HashSet::new(),
            playlist: Vec::new(),
            sort_key: SortKey::default(),
            current_index: None,
            single_file_session: false,
            scan_subfolders: false,
            repeat_mode: RepeatMode::Off,
            active_collection_id: None,
            active_playlist_id: None,
            playback_kind: "audiobook".into(),
        }
    }

    fn seed_enqueue_fixture() -> (LibraryDb, i64, PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("cc_enqueue_test_{stamp}"));
        fs::create_dir_all(&base).unwrap();
        let track1 = base.join("track1.m4b");
        let track2 = base.join("track2.m4b");
        fs::write(&track1, b"x").unwrap();
        fs::write(&track2, b"x").unwrap();
        let root_path = base.to_string_lossy().to_string();
        let track1_s = track1.to_string_lossy().to_string();
        let track2_s = track2.to_string_lossy().to_string();

        let mut db = LibraryDb::open_in_memory().unwrap();
        let now = InnerState::now_unix();
        db.connection_mut()
            .execute(
                "INSERT INTO library_roots (path, label, content_kind, scan_rule, scan_subfolders, is_available, created_at, updated_at)
                 VALUES (?1, 'Test', 'audiobook', 'subfolder-is-item', 1, 1, ?2, ?2)",
                params![root_path.as_str(), now],
            )
            .unwrap();
        let root_id = db.connection().last_insert_rowid();
        db.connection_mut()
            .execute(
                "INSERT INTO collections (root_id, kind, title, sort_title, layout_kind, created_at, updated_at)
                 VALUES (?1, 'audiobook', 'Book', 'Book', 'flat_multi', ?2, ?2)",
                params![root_id, now],
            )
            .unwrap();
        let collection_id = db.connection().last_insert_rowid();
        for (order, path) in [(0, &track1_s), (1, &track2_s)] {
            db.connection_mut()
                .execute(
                    "INSERT INTO collection_files (collection_id, path, display_title, label, track_order, created_at, updated_at)
                     VALUES (?1, ?2, 'Track', 'Track', ?3, ?4, ?4)",
                    params![collection_id, path.as_str(), order, now],
                )
                .unwrap();
        }
        (db, collection_id, base)
    }

    #[test]
    fn resolve_playlist_shuffle_pref() {
        let db = LibraryDb::open_in_memory().unwrap();
        assert!(!resolve_playlist_shuffle(None, &db));
        assert!(!resolve_playlist_shuffle(Some(false), &db));
        assert!(resolve_playlist_shuffle(Some(true), &db));

        let mut db = LibraryDb::open_in_memory().unwrap();
        db.set_setting(PREF_PLAYLIST_SHUFFLE, "true").unwrap();
        assert!(resolve_playlist_shuffle(None, &db));
        assert!(!resolve_playlist_shuffle(Some(false), &db));
    }

    #[test]
    fn enqueue_empty_queue_end_starts_session_without_autoplay() {
        let (db, collection_id, tmp) = seed_enqueue_fixture();
        let mut state = test_inner(db);
        let result = state
            .enqueue_collection_inner(collection_id, "end")
            .expect("enqueue");
        assert!(result.session_started);
        assert!(!result.autoplay_started);
        assert_eq!(result.tracks_added, 2);
        assert_eq!(state.playlist.len(), 2);
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn enqueue_duplicate_collection_errors() {
        let (db, collection_id, tmp) = seed_enqueue_fixture();
        let mut state = test_inner(db);
        state.enqueue_collection_inner(collection_id, "end").unwrap();
        match state.enqueue_collection_inner(collection_id, "end") {
            Err(msg) => assert!(msg.contains("already in your queue")),
            Ok(_) => panic!("expected duplicate enqueue to fail"),
        }
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn enqueue_empty_playlist_with_session_root_starts_fresh_session() {
        let (db, collection_id, tmp) = seed_enqueue_fixture();
        let mut state = test_inner(db);
        state.session_root = Some(tmp.join("track1.m4b"));
        state.playlist = vec![];
        let result = state
            .enqueue_collection_inner(collection_id, "end")
            .expect("enqueue");
        assert!(result.session_started);
        assert!(!result.autoplay_started);
        assert_eq!(state.playlist.len(), 2);
        let _ = fs::remove_dir_all(tmp);
    }
}
