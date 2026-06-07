//! Persistent library catalog: roots, collections, playlists, scanner.

use crate::db::LibraryDb;
use crate::path_policy::{
    canonicalize_existing, canonicalize_under_root, is_under_any_root, tracked_file_on_disk,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const AUDIO_EXT: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "flac", "ogg", "opus", "wav", "wma", "aiff", "aif", "oga",
];

pub const PREF_ONLINE_METADATA: &str = "pref.online_metadata_enabled";
pub const PREF_REPEAT_MODE: &str = "session.repeat_mode";
pub const SESSION_COLLECTION_ID: &str = "session.collection_id";
pub const SESSION_PLAYBACK_KIND: &str = "session.playback_kind";
pub const SESSION_PLAYLIST_ID: &str = "session.playlist_id";

/// Minimum % before a title appears in "In progress" / "In Arbeit" lists (avoids 0% rows).
const IN_PROGRESS_LIST_MIN_PCT: f64 = 1.0;

fn show_in_progress_list(in_progress: bool, listened: bool, progress_pct: f64) -> bool {
    in_progress && !listened && progress_pct >= IN_PROGRESS_LIST_MIN_PCT
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentKind {
    Audiobook,
    Music,
    Mixed,
}

impl ContentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audiobook => "audiobook",
            Self::Music => "music",
            Self::Mixed => "mixed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "audiobook" | "book" => Some(Self::Audiobook),
            "music" => Some(Self::Music),
            "mixed" => Some(Self::Mixed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanRule {
    SubfolderIsItem,
    FileIsItem,
    TagArtistAlbum,
    AutoClassify,
}

impl ScanRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SubfolderIsItem => "subfolder-is-item",
            Self::FileIsItem => "file-is-item",
            Self::TagArtistAlbum => "tag-artist-album",
            Self::AutoClassify => "auto-classify",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "subfolder-is-item" => Some(Self::SubfolderIsItem),
            "file-is-item" => Some(Self::FileIsItem),
            "tag-artist-album" => Some(Self::TagArtistAlbum),
            "auto-classify" => Some(Self::AutoClassify),
            _ => None,
        }
    }

    pub fn default_for(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Audiobook => Self::SubfolderIsItem,
            ContentKind::Music => Self::TagArtistAlbum,
            ContentKind::Mixed => Self::AutoClassify,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    SingleFile,
    FlatMulti,
    CdNested,
}

impl LayoutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleFile => "single_file",
            Self::FlatMulti => "flat_multi",
            Self::CdNested => "cd_nested",
        }
    }
}

#[derive(Clone, Serialize)]
pub struct LibraryRootDto {
    pub id: i64,
    pub path: String,
    pub label: String,
    pub content_kind: String,
    pub scan_rule: String,
    pub scan_subfolders: bool,
    pub is_available: bool,
    pub last_scan_at: Option<i64>,
    pub last_scan_status: Option<String>,
    pub collection_count: i64,
}

#[derive(Clone, Serialize)]
pub struct CollectionSummaryDto {
    pub id: i64,
    pub root_id: i64,
    pub kind: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub layout_kind: String,
    pub cover_path: Option<String>,
    pub progress_pct: f64,
    pub listened: bool,
    pub in_progress: bool,
    pub unavailable: bool,
    pub root_unavailable: bool,
    pub playable_file_count: i64,
    pub missing_file_count: i64,
    pub location_hint: String,
    pub track_count: i64,
    pub last_played_at: Option<i64>,
    pub series: Option<String>,
    pub series_index: Option<i32>,
}

#[derive(Clone, Serialize)]
pub struct CollectionFileDto {
    pub id: i64,
    pub path: String,
    pub display_title: String,
    pub label: String,
    pub track_order: i32,
    pub disc_index: i32,
    pub track_index: i32,
    pub duration_sec: Option<f64>,
    pub position_sec: f64,
    pub listened: bool,
    pub unavailable: bool,
}

#[derive(Clone, Serialize)]
pub struct CollectionDetailDto {
    pub id: i64,
    pub root_id: i64,
    pub kind: String,
    pub title: String,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<i32>,
    pub layout_kind: String,
    pub cover_path: Option<String>,
    pub progress_pct: f64,
    pub listened: bool,
    pub unavailable: bool,
    pub root_unavailable: bool,
    pub missing_file_count: i64,
    pub playable_file_count: i64,
    pub location_hint: String,
    pub is_manual: bool,
    pub files: Vec<CollectionFileDto>,
}

#[derive(Clone, Serialize)]
pub struct HomeSummaryDto {
    pub continue_item: Option<CollectionSummaryDto>,
    pub in_progress: Vec<CollectionSummaryDto>,
    pub music_shelf: Vec<CollectionSummaryDto>,
    pub has_library: bool,
    pub scan_in_progress: bool,
}

#[derive(Clone, Serialize)]
pub struct PlaylistSummaryDto {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub is_pinned: bool,
    pub track_count: i64,
    pub unavailable_count: i64,
}

#[derive(Clone, Serialize)]
pub struct PlaylistDetailDto {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub is_pinned: bool,
    pub default_playback_speed: Option<f64>,
    pub items: Vec<PlaylistItemDto>,
}

#[derive(Clone, Serialize)]
pub struct PlaylistItemDto {
    pub id: i64,
    pub collection_file_id: i64,
    pub track_order: i32,
    pub display_title: String,
    pub collection_title: String,
    pub unavailable: bool,
}

#[derive(Clone, Serialize)]
pub struct ScanStatusDto {
    pub root_id: i64,
    pub scanning: bool,
    pub last_scan_at: Option<i64>,
    pub last_scan_status: Option<String>,
    pub collections_found: i64,
}

#[derive(Clone, Serialize)]
pub struct ImportFolderToPlaylistResult {
    pub folder_path: String,
    pub tracks_added: i64,
    pub tracks_skipped: i64,
    pub tracks_total: i64,
    pub library_linked: bool,
}

#[derive(Clone, Serialize)]
pub struct AddToPlaylistBulkResult {
    pub tracks_added: i64,
    pub tracks_skipped: i64,
}

#[derive(Clone, Serialize)]
pub struct AlbumGroupDto {
    pub artist: String,
    pub album: String,
    pub track_count: i64,
}

#[derive(Clone, Serialize)]
pub struct MetadataGroupDto {
    pub group_kind: String,
    pub group_key: String,
    pub label: String,
    pub subtitle: Option<String>,
    pub track_count: i64,
}

#[derive(Clone, Serialize)]
pub struct RemoveCollectionFileResult {
    pub collection_removed: bool,
    pub removed_path: String,
}

#[derive(Clone, Serialize)]
pub struct RemoveCollectionResult {
    pub removed_paths: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct RelinkCollectionFileResult {
    pub old_path: String,
    pub new_path: String,
    pub collection_id: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CollectionMetadataInput {
    pub title: Option<String>,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<i32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AddLibraryRootInput {
    pub path: String,
    pub label: Option<String>,
    pub content_kind: String,
    pub scan_rule: Option<String>,
    pub scan_subfolders: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AddToLibraryInput {
    pub path: String,
    pub content_kind: String,
    pub grouping: String,
    pub metadata: CollectionMetadataInput,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MetadataSuggestionDto {
    pub title: Option<String>,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub source: String,
}

#[derive(Clone)]
pub struct MetadataLookupRequest {
    pub kind: String,
    pub title: String,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cache_key: String,
}

pub enum MetadataLookupPlan {
    Disabled,
    EmptyTitle,
    Cached(Vec<MetadataSuggestionDto>),
    Fetch(MetadataLookupRequest),
}

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

fn natord_sort_paths(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| {
        natord::compare(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
    });
}

fn parse_disc_index(name: &str) -> Option<i32> {
    let lower = name.to_lowercase();
    for prefix in ["cd", "disc", "disk"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<i32>() {
                return Some(n);
            }
        }
    }
    None
}

fn parse_track_index(filename: &str) -> i32 {
    let stem = Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let digits: String = stem
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '_')
        .filter(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(0)
}

fn partial_file_hash(path: &Path) -> Option<String> {
    const CHUNK: usize = 64 * 1024;
    let mut f = fs::File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    let len = meta.len() as usize;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK.min(len.max(1))];
    if len == 0 {
        return Some(format!("{:x}", hasher.finalize()));
    }
    let n = f.read(&mut buf).ok()?;
    hasher.update(&buf[..n]);
    if len > CHUNK {
        let mut tail_buf = vec![0u8; CHUNK];
        if let Ok(mut f2) = fs::File::open(path) {
            if f2.seek(std::io::SeekFrom::End(-(CHUNK as i64))).is_ok() {
                if let Ok(m) = f2.read(&mut tail_buf) {
                    hasher.update(&tail_buf[..m]);
                }
            }
        }
    }
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn file_inode(path: &Path) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| m.ino() as i64)
}

#[cfg(not(unix))]
fn file_inode(_path: &Path) -> Option<i64> {
    None
}

struct ScannedFile {
    path: PathBuf,
    display_title: String,
    label: String,
    disc_index: i32,
    track_index: i32,
}

fn probe_tags(path: &Path) -> (Option<String>, Option<String>, Option<String>, Option<u32>, Option<u32>) {
    use lofty::file::TaggedFileExt;
    use lofty::prelude::Accessor;
    let Ok(tagged) = lofty::read_from_path(path) else {
        return (None, None, None, None, None);
    };
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let Some(t) = tag else {
        return (None, None, None, None, None);
    };
    let title = t.title().map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
    let artist = t.artist().map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
    let album = t.album().map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
    let track = t.track();
    let disc = t.disk();
    (title, artist, album, track, disc)
}

fn probe_duration(path: &Path) -> Option<f64> {
    use lofty::file::AudioFile;
    let tagged = lofty::read_from_path(path).ok()?;
    let d = tagged.properties().duration();
    if d.as_secs_f64() > 0.0 {
        Some(d.as_secs_f64())
    } else {
        None
    }
}

fn is_m4b(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("m4b"))
        .unwrap_or(false)
}

/// Guess audiobook vs music from file count, duration, size, and tags.
fn classify_collection(files: &[ScannedFile], layout: LayoutKind) -> ContentKind {
    if files.is_empty() {
        return ContentKind::Audiobook;
    }

    if files.iter().any(|f| is_m4b(&f.path)) {
        return ContentKind::Audiobook;
    }

    let file_count = files.len();
    let mut total_duration = 0.0_f64;
    let mut max_duration = 0.0_f64;
    let mut total_size = 0_u64;
    let mut dur_samples = 0_u32;

    for sf in files {
        if let Ok(meta) = fs::metadata(&sf.path) {
            total_size += meta.len();
        }
        if let Some(d) = probe_duration(&sf.path) {
            total_duration += d;
            max_duration = max_duration.max(d);
            dur_samples += 1;
        }
    }

    let avg_duration = if dur_samples > 0 {
        total_duration / dur_samples as f64
    } else {
        0.0
    };

    const MIN: f64 = 60.0;
    const LONG_TRACK: f64 = 45.0 * MIN;
    const SHORT_TRACK: f64 = 12.0 * MIN;
    const ALBUM_TRACK: f64 = 8.0 * MIN;

    if file_count == 1 {
        if max_duration >= LONG_TRACK || total_size >= 80 * 1024 * 1024 {
            return ContentKind::Audiobook;
        }
        if max_duration > 0.0 && max_duration < SHORT_TRACK {
            return ContentKind::Music;
        }
        return ContentKind::Audiobook;
    }

    if layout == LayoutKind::CdNested {
        return ContentKind::Audiobook;
    }

    let tagged_album = files.iter().any(|f| {
        let (_, artist, album, _, _) = probe_tags(&f.path);
        artist.is_some() && album.is_some()
    });

    if file_count >= 3 && avg_duration > 0.0 && avg_duration < ALBUM_TRACK {
        return ContentKind::Music;
    }

    if total_duration >= 3.0 * 3600.0 && file_count <= 40 {
        return ContentKind::Audiobook;
    }

    if tagged_album && avg_duration > 0.0 && avg_duration < SHORT_TRACK {
        return ContentKind::Music;
    }

    if file_count >= 5 && avg_duration > 0.0 && avg_duration < 10.0 * MIN {
        return ContentKind::Music;
    }

    ContentKind::Audiobook
}

fn gather_audiobook_files(book_dir: &Path) -> (LayoutKind, Vec<ScannedFile>) {
    let mut direct_audio = Vec::new();
    let mut disc_dirs: Vec<(i32, String, PathBuf)> = Vec::new();

    if let Ok(entries) = fs::read_dir(book_dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_file() && is_audio_file(&p) {
                direct_audio.push(p);
            } else if p.is_dir() {
                let dir_name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if let Some(disc) = parse_disc_index(&dir_name) {
                    disc_dirs.push((disc, p.file_name().unwrap().to_string_lossy().to_string(), p));
                } else {
                    // nested non-CD folder — recurse flat
                    if let Ok(sub) = fs::read_dir(&p) {
                        for se in sub.flatten() {
                            let sp = se.path();
                            if sp.is_file() && is_audio_file(&sp) {
                                direct_audio.push(sp);
                            }
                        }
                    }
                }
            }
        }
    }

    let layout = if direct_audio.len() == 1 && disc_dirs.is_empty() {
        LayoutKind::SingleFile
    } else if !disc_dirs.is_empty() {
        LayoutKind::CdNested
    } else {
        LayoutKind::FlatMulti
    };

    let mut out = Vec::new();
    if !disc_dirs.is_empty() {
        disc_dirs.sort_by_key(|(d, _, _)| *d);
        for (disc_idx, disc_name, disc_path) in disc_dirs {
            let mut files: Vec<PathBuf> = fs::read_dir(&disc_path)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|e| {
                    let p = e.ok()?.path();
                    if p.is_file() && is_audio_file(&p) {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect();
            natord_sort_paths(&mut files);
            for (ti, path) in files.into_iter().enumerate() {
                let fname = path.file_name().unwrap().to_string_lossy().to_string();
                let (tag_title, _, _, track, disc) = probe_tags(&path);
                let disc_index = disc.map(|d| d as i32).unwrap_or(disc_idx);
                let track_index = track.map(|t| t as i32).unwrap_or(parse_track_index(&fname) as i32);
                let display = tag_title.unwrap_or_else(|| format!("{disc_name} · Part {}", ti + 1));
                out.push(ScannedFile {
                    path,
                    display_title: display.clone(),
                    label: display,
                    disc_index,
                    track_index,
                });
            }
        }
    } else {
        natord_sort_paths(&mut direct_audio);
        for (i, path) in direct_audio.into_iter().enumerate() {
            let fname = path.file_name().unwrap().to_string_lossy().to_string();
            let (tag_title, _, _, track, disc) = probe_tags(&path);
            let display = tag_title.unwrap_or_else(|| format!("Part {}", i + 1));
            out.push(ScannedFile {
                path,
                display_title: display.clone(),
                label: display,
                disc_index: disc.map(|d| d as i32).unwrap_or(0),
                track_index: track.map(|t| t as i32).unwrap_or(parse_track_index(&fname) as i32),
            });
        }
    }

    out.sort_by(|a, b| {
        a.disc_index
            .cmp(&b.disc_index)
            .then(a.track_index.cmp(&b.track_index))
            .then(a.path.cmp(&b.path))
    });
    (layout, out)
}

fn common_parent_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    if paths.is_empty() {
        return None;
    }
    if paths.len() == 1 {
        return paths[0].parent().map(|p| p.to_path_buf());
    }
    let mut common = paths[0].parent()?.to_path_buf();
    for p in &paths[1..] {
        while !p.starts_with(&common) {
            common = common.parent()?.to_path_buf();
        }
    }
    Some(common)
}

pub struct CatalogService<'a> {
    db: &'a mut LibraryDb,
}

impl<'a> CatalogService<'a> {
    pub fn new(db: &'a mut LibraryDb) -> Self {
        Self { db }
    }

    fn conn(&self) -> &rusqlite::Connection {
        self.db.connection()
    }

    fn conn_mut(&mut self) -> &mut rusqlite::Connection {
        self.db.connection_mut()
    }

    pub fn list_roots(&self) -> Result<Vec<LibraryRootDto>, String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.path, r.label, r.content_kind, r.scan_rule, r.scan_subfolders,
                        r.is_available, r.last_scan_at, r.last_scan_status,
                        (SELECT COUNT(*) FROM collections c WHERE c.root_id = r.id)
                 FROM library_roots r ORDER BY r.label",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(LibraryRootDto {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    label: r.get(2)?,
                    content_kind: r.get(3)?,
                    scan_rule: r.get(4)?,
                    scan_subfolders: r.get::<_, i32>(5)? != 0,
                    is_available: r.get::<_, i32>(6)? != 0,
                    last_scan_at: r.get(7)?,
                    last_scan_status: r.get(8)?,
                    collection_count: r.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    fn find_root_id_by_canonical_path(&self, path: &Path) -> Result<Option<i64>, String> {
        let path_str = path.to_string_lossy().to_string();
        let exact: Option<i64> = self
            .conn()
            .query_row(
                "SELECT id FROM library_roots WHERE path = ?1",
                [&path_str],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if exact.is_some() {
            return Ok(exact);
        }
        for root in self.list_roots()? {
            if let Ok(canon) = canonicalize_existing(Path::new(&root.path)) {
                if canon == path {
                    return Ok(Some(root.id));
                }
            }
        }
        Ok(None)
    }

    fn root_dto_by_id(&self, id: i64) -> Result<LibraryRootDto, String> {
        self.list_roots()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| "Library root not found".into())
    }

    pub fn add_root(&mut self, input: AddLibraryRootInput) -> Result<LibraryRootDto, String> {
        let kind = ContentKind::parse(&input.content_kind).ok_or("Invalid content kind")?;
        let rule = input
            .scan_rule
            .as_deref()
            .and_then(ScanRule::parse)
            .unwrap_or_else(|| ScanRule::default_for(kind));
        let path = canonicalize_existing(Path::new(input.path.trim()))
            .map_err(|e| e.to_string())?;
        if !path.is_dir() {
            return Err("Library root must be a folder".into());
        }
        let path_str = path.to_string_lossy().to_string();
        let now = now_unix();

        if let Some(existing_id) = self.find_root_id_by_canonical_path(&path)? {
            self.conn_mut()
                .execute(
                    "UPDATE library_roots SET path = ?1, is_available = 1, updated_at = ?2 WHERE id = ?3",
                    params![path_str.as_str(), now, existing_id],
                )
                .map_err(|e| e.to_string())?;
            self.scan_root(existing_id)?;
            return self.root_dto_by_id(existing_id);
        }

        let label = input
            .label
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path_str.clone())
            });
        let scan_sub = input
            .scan_subfolders
            .unwrap_or(kind == ContentKind::Audiobook || kind == ContentKind::Mixed);
        let available = 1;
        self.conn_mut()
            .execute(
                "INSERT INTO library_roots (path, label, content_kind, scan_rule, scan_subfolders,
                 is_available, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
                params![
                    path_str.as_str(),
                    label,
                    kind.as_str(),
                    rule.as_str(),
                    scan_sub as i32,
                    available,
                    now,
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed: library_roots.path") {
                    "This folder is already in your library.".into()
                } else {
                    e.to_string()
                }
            })?;
        let id = self.conn().last_insert_rowid();
        self.scan_root(id)?;
        self.root_dto_by_id(id)
    }

    pub fn remove_root(&mut self, root_id: i64) -> Result<(), String> {
        self.conn_mut()
            .execute("DELETE FROM library_roots WHERE id = ?1", [root_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_root_path(&mut self, root_id: i64, new_path: &str) -> Result<(), String> {
        let path = canonicalize_existing(Path::new(new_path.trim())).map_err(|e| e.to_string())?;
        if !path.is_dir() {
            return Err("Path must be a folder".into());
        }
        if let Some(other_id) = self.find_root_id_by_canonical_path(&path)? {
            if other_id != root_id {
                return Err("This folder is already in your library.".into());
            }
        }
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE library_roots SET path = ?1, is_available = 1, updated_at = ?2 WHERE id = ?3",
                params![path.to_string_lossy().as_ref(), now, root_id],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed: library_roots.path") {
                    "This folder is already in your library.".into()
                } else {
                    e.to_string()
                }
            })?;
        self.scan_root(root_id)?;
        Ok(())
    }

    pub fn refresh_roots_availability(&mut self) -> Result<(), String> {
        let roots = self.list_roots()?;
        let now = now_unix();
        for r in roots {
            let available = Path::new(&r.path).exists();
            self.conn_mut()
                .execute(
                    "UPDATE library_roots SET is_available = ?1, updated_at = ?2 WHERE id = ?3",
                    params![available as i32, now, r.id],
                )
                .map_err(|e| e.to_string())?;
            if available && !r.is_available {
                let _ = self.scan_root(r.id);
            }
        }
        Ok(())
    }

    pub fn scan_root(&mut self, root_id: i64) -> Result<ScanStatusDto, String> {
        let row: (String, String, String, i32) = self
            .conn()
            .query_row(
                "SELECT path, content_kind, scan_rule, scan_subfolders FROM library_roots WHERE id = ?1",
                [root_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|e| e.to_string())?;

        let (path_s, kind_s, rule_s, _scan_sub) = row;
        let root_path = PathBuf::from(&path_s);
        if !root_path.exists() {
            let now = now_unix();
            self.conn_mut()
                .execute(
                    "UPDATE library_roots SET is_available = 0, last_scan_at = ?1, last_scan_status = 'away' WHERE id = ?2",
                    params![now, root_id],
                )
                .map_err(|e| e.to_string())?;
            return Ok(ScanStatusDto {
                root_id,
                scanning: false,
                last_scan_at: Some(now),
                last_scan_status: Some("away".into()),
                collections_found: 0,
            });
        }

        let kind = ContentKind::parse(&kind_s).unwrap_or(ContentKind::Mixed);
        let rule = ScanRule::parse(&rule_s).unwrap_or_else(|| ScanRule::default_for(kind));
        let root_canon = canonicalize_existing(&root_path).map_err(|e| e.to_string())?;
        let now = now_unix();
        let scan_started = now;
        let mut found = 0i64;

        match (kind, rule) {
            (ContentKind::Audiobook, ScanRule::SubfolderIsItem) => {
                if let Ok(entries) = fs::read_dir(&root_canon) {
                    for ent in entries.flatten() {
                        let p = ent.path();
                        if p.is_file() && is_audio_file(&p) {
                            let canon = canonicalize_under_root(&p, &root_canon)
                                .map_err(|e| e.to_string())?;
                            let title = canon
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Audiobook".into());
                            let files = vec![ScannedFile {
                                path: canon,
                                display_title: title.clone(),
                                label: title.clone(),
                                disc_index: 0,
                                track_index: 1,
                            }];
                            found += self.upsert_collection(
                                root_id,
                                kind,
                                &title,
                                LayoutKind::SingleFile,
                                &files,
                                None,
                                Some(scan_started),
                            )? as i64;
                        } else if p.is_dir() {
                            let book_dir = canonicalize_under_root(&p, &root_canon)
                                .map_err(|e| e.to_string())?;
                            let title = book_dir
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Audiobook".into());
                            let (layout, files) = gather_audiobook_files(&book_dir);
                            if !files.is_empty() {
                                found += self.upsert_collection(
                                    root_id,
                                    kind,
                                    &title,
                                    layout,
                                    &files,
                                    None,
                                    Some(scan_started),
                                )? as i64;
                            }
                        }
                    }
                }
            }
            (ContentKind::Audiobook, ScanRule::FileIsItem) => {
                let loose = self.collect_audio_files(&root_canon, &root_canon)?;
                for file in loose {
                    let title = file
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Audiobook".into());
                    let files = vec![ScannedFile {
                        path: file,
                        display_title: title.clone(),
                        label: title.clone(),
                        disc_index: 0,
                        track_index: 1,
                    }];
                    found += self.upsert_collection(
                        root_id,
                        kind,
                        &title,
                        LayoutKind::SingleFile,
                        &files,
                        None,
                        Some(scan_started),
                    )? as i64;
                }
            }
            (ContentKind::Music, ScanRule::TagArtistAlbum) | (_, ScanRule::TagArtistAlbum) => {
                let mut by_album: HashMap<(String, String), Vec<PathBuf>> = HashMap::new();
                for file in self.collect_audio_files(&root_canon, &root_canon)? {
                    let (_, artist, album, _, _) = probe_tags(&file);
                    let artist = artist.unwrap_or_else(|| "Unknown Artist".into());
                    let album = album.unwrap_or_else(|| {
                        file.parent()
                            .and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Unknown Album".into())
                    });
                    by_album.entry((artist, album)).or_default().push(file);
                }
                for ((artist, album), mut paths) in by_album {
                    natord_sort_paths(&mut paths);
                    let title = format!("{album}");
                    let files: Vec<ScannedFile> = paths
                        .into_iter()
                        .enumerate()
                        .map(|(i, path)| {
                            let (tag_title, _, _, _, _) = probe_tags(&path);
                            let display = tag_title.unwrap_or_else(|| format!("Track {}", i + 1));
                            ScannedFile {
                                path,
                                display_title: display.clone(),
                                label: display,
                                disc_index: 0,
                                track_index: i as i32 + 1,
                            }
                        })
                        .collect();
                    let meta = CollectionMetadataInput {
                        title: Some(title.clone()),
                        author: None,
                        narrator: None,
                        artist: Some(artist),
                        album: Some(album),
                        series: None,
                        series_index: None,
                    };
                    found += self.upsert_collection(
                        root_id,
                        ContentKind::Music,
                        &title,
                        LayoutKind::FlatMulti,
                        &files,
                        Some(meta),
                        Some(scan_started),
                    )? as i64;
                }
            }
            (ContentKind::Music, ScanRule::SubfolderIsItem) => {
                if let Ok(entries) = fs::read_dir(&root_canon) {
                    for ent in entries.flatten() {
                        let p = ent.path();
                        if !p.is_dir() {
                            continue;
                        }
                        let album_dir = canonicalize_under_root(&p, &root_canon)
                            .map_err(|e| e.to_string())?;
                        let title = album_dir
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Album".into());
                        let mut paths = self.collect_audio_files(&root_canon, &album_dir)?;
                        if paths.is_empty() {
                            continue;
                        }
                        natord_sort_paths(&mut paths);
                        let artist = paths
                            .first()
                            .and_then(|p| probe_tags(p).1)
                            .unwrap_or_else(|| "Unknown Artist".into());
                        let files: Vec<ScannedFile> = paths
                            .into_iter()
                            .enumerate()
                            .map(|(i, path)| {
                                let (tag_title, _, _, _, _) = probe_tags(&path);
                                let display = tag_title.unwrap_or_else(|| format!("Track {}", i + 1));
                                ScannedFile {
                                    path,
                                    display_title: display.clone(),
                                    label: display,
                                    disc_index: 0,
                                    track_index: i as i32 + 1,
                                }
                            })
                            .collect();
                        let meta = CollectionMetadataInput {
                            title: Some(title.clone()),
                            author: None,
                            narrator: None,
                            artist: Some(artist),
                            album: Some(title.clone()),
                            series: None,
                            series_index: None,
                        };
                        found += self.upsert_collection(
                            root_id,
                            ContentKind::Music,
                            &title,
                            LayoutKind::FlatMulti,
                            &files,
                            Some(meta),
                            Some(scan_started),
                        )? as i64;
                    }
                }
            }
            (ContentKind::Music, ScanRule::FileIsItem) => {
                let loose = self.collect_audio_files(&root_canon, &root_canon)?;
                for file in loose {
                    let (tag_title, artist, album, _, _) = probe_tags(&file);
                    let title = tag_title.unwrap_or_else(|| {
                        file.file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Track".into())
                    });
                    let artist = artist.unwrap_or_else(|| "Unknown Artist".into());
                    let album = album.unwrap_or_else(|| title.clone());
                    let files = vec![ScannedFile {
                        path: file,
                        display_title: title.clone(),
                        label: title.clone(),
                        disc_index: 0,
                        track_index: 1,
                    }];
                    let meta = CollectionMetadataInput {
                        title: Some(title.clone()),
                        author: None,
                        narrator: None,
                        artist: Some(artist),
                        album: Some(album),
                        series: None,
                        series_index: None,
                    };
                    found += self.upsert_collection(
                        root_id,
                        ContentKind::Music,
                        &title,
                        LayoutKind::SingleFile,
                        &files,
                        Some(meta),
                        Some(scan_started),
                    )? as i64;
                }
            }
            (_, ScanRule::AutoClassify) | (ContentKind::Mixed, _) => {
                found += self.scan_auto_classify_root(root_id, &root_canon, scan_started)? as i64;
            }
        }

        self.reconcile_stale_files_after_scan(root_id, scan_started)?;
        self.mark_missing_files_unavailable_after_scan(root_id, scan_started)?;
        self.reconcile_false_unavailable_in_root(root_id)?;
        self.reconcile_superseded_unavailable_files_in_root(root_id)?;
        self.prune_empty_collections_in_root(root_id)?;

        self.conn_mut()
            .execute(
                "UPDATE library_roots SET is_available = 1, last_scan_at = ?1, last_scan_status = 'ok', updated_at = ?1 WHERE id = ?2",
                params![now, root_id],
            )
            .map_err(|e| e.to_string())?;

        Ok(ScanStatusDto {
            root_id,
            scanning: false,
            last_scan_at: Some(now),
            last_scan_status: Some("ok".into()),
            collections_found: found,
        })
    }

    fn collect_audio_files(&self, root: &Path, start: &Path) -> Result<Vec<PathBuf>, String> {
        let mut stack = vec![start.to_path_buf()];
        let mut out = Vec::new();
        while let Some(dir) = stack.pop() {
            let entries =
                fs::read_dir(&dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))?;
            for ent in entries {
                let ent = ent.map_err(|e| e.to_string())?;
                let p = ent.path();
                if p.is_dir() {
                    let canon = canonicalize_under_root(&p, root).map_err(|e| e.to_string())?;
                    stack.push(canon);
                } else if p.is_file() && is_audio_file(&p) {
                    let canon = canonicalize_under_root(&p, root).map_err(|e| e.to_string())?;
                    out.push(canon);
                }
            }
        }
        Ok(out)
    }

    fn mark_collection_manual(&mut self, collection_id: i64) -> Result<(), String> {
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE collections SET is_manual = 1, updated_at = ?1 WHERE id = ?2",
                params![now, collection_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn find_collection_by_root_title(
        &self,
        root_id: i64,
        title: &str,
    ) -> Result<Option<(i64, String, bool)>, String> {
        self.conn()
            .query_row(
                "SELECT id, kind, is_manual FROM collections
                 WHERE root_id = ?1 AND title = ?2
                 ORDER BY is_manual DESC, id ASC
                 LIMIT 1",
                params![root_id, title],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i32>(2)? != 0)),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    fn apply_auto_kind(
        &mut self,
        root_id: i64,
        title: &str,
        classified: ContentKind,
    ) -> Result<ContentKind, String> {
        let Some((id, stored_kind, is_manual)) = self.find_collection_by_root_title(root_id, title)? else {
            return Ok(classified);
        };
        if is_manual {
            return Ok(ContentKind::parse(&stored_kind).unwrap_or(classified));
        }
        let stored = ContentKind::parse(&stored_kind).unwrap_or(classified);
        if stored != classified {
            let now = now_unix();
            let updated = self
                .conn_mut()
                .execute(
                    "UPDATE collections SET kind = ?1, updated_at = ?2 WHERE id = ?3",
                    params![classified.as_str(), now, id],
                )
                .map_err(|e| e.to_string())?;
            if updated == 0 {
                return Ok(stored);
            }
        }
        Ok(classified)
    }

    fn scan_auto_classify_root(
        &mut self,
        root_id: i64,
        root_canon: &Path,
        scan_started: i64,
    ) -> Result<usize, String> {
        let mut found = 0usize;
        if let Ok(entries) = fs::read_dir(root_canon) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_file() && is_audio_file(&p) {
                    let canon = canonicalize_under_root(&p, root_canon).map_err(|e| e.to_string())?;
                    let title = canon
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Audiobook".into());
                    let files = vec![ScannedFile {
                        path: canon,
                        display_title: title.clone(),
                        label: title.clone(),
                        disc_index: 0,
                        track_index: 1,
                    }];
                    let classified = classify_collection(&files, LayoutKind::SingleFile);
                    let kind = self.apply_auto_kind(root_id, &title, classified)?;
                    found += self.upsert_collection(
                        root_id,
                        kind,
                        &title,
                        LayoutKind::SingleFile,
                        &files,
                        None,
                        Some(scan_started),
                    )?;
                } else if p.is_dir() {
                    let book_dir = canonicalize_under_root(&p, root_canon).map_err(|e| e.to_string())?;
                    let title = book_dir
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Item".into());
                    let (layout, files) = gather_audiobook_files(&book_dir);
                    if files.is_empty() {
                        continue;
                    }
                    let classified = classify_collection(&files, layout);
                    let kind = self.apply_auto_kind(root_id, &title, classified)?;
                    let meta = if kind == ContentKind::Music {
                        let artist = files
                            .first()
                            .and_then(|f| probe_tags(&f.path).1)
                            .unwrap_or_else(|| "Unknown Artist".into());
                        Some(CollectionMetadataInput {
                            title: Some(title.clone()),
                            author: None,
                            narrator: None,
                            artist: Some(artist),
                            album: Some(title.clone()),
                            series: None,
                            series_index: None,
                        })
                    } else {
                        None
                    };
                    found += self.upsert_collection(
                        root_id,
                        kind,
                        &title,
                        layout,
                        &files,
                        meta,
                        Some(scan_started),
                    )?;
                }
            }
        }
        Ok(found)
    }

    fn collection_available_count(&self, collection_id: i64) -> Result<i64, String> {
        self.conn()
            .query_row(
                "SELECT COUNT(*) FROM collection_files WHERE collection_id = ?1 AND unavailable = 0",
                [collection_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }

    fn collection_fingerprints(
        &self,
        collection_id: i64,
        available_only: bool,
    ) -> Result<HashSet<(String, i64)>, String> {
        let sql = if available_only {
            "SELECT partial_hash, file_size FROM collection_files
             WHERE collection_id = ?1 AND unavailable = 0
             AND partial_hash IS NOT NULL AND file_size > 0"
        } else {
            "SELECT partial_hash, file_size FROM collection_files
             WHERE collection_id = ?1 AND partial_hash IS NOT NULL AND file_size > 0"
        };
        let mut stmt = self.conn().prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([collection_id], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    /// True when the target duplicate is an empty shell or the same audio by fingerprint (e.g. after relink/move).
    fn can_absorb_kind_conflict(&self, source_id: i64, conflict_id: i64) -> Result<bool, String> {
        let source_avail = self.collection_available_count(source_id)?;
        let conflict_avail = self.collection_available_count(conflict_id)?;

        if conflict_avail == 0 {
            return Ok(true);
        }

        let source_avail_fps = self.collection_fingerprints(source_id, true)?;
        let conflict_avail_fps = self.collection_fingerprints(conflict_id, true)?;

        if !source_avail_fps.is_empty() && source_avail_fps == conflict_avail_fps {
            return Ok(true);
        }

        let source_all = self.collection_fingerprints(source_id, false)?;
        let conflict_all = self.collection_fingerprints(conflict_id, false)?;
        if !conflict_all.is_empty() && conflict_all.is_subset(&source_all) {
            return Ok(true);
        }

        if source_avail == 0 && !conflict_avail_fps.is_empty() {
            return Ok(true);
        }

        Ok(false)
    }

    fn copy_collection_metadata(&mut self, from_id: i64, to_id: i64) -> Result<(), String> {
        self.conn_mut()
            .execute(
                "UPDATE collections SET
                    author = COALESCE((SELECT author FROM collections WHERE id = ?1), author),
                    narrator = COALESCE((SELECT narrator FROM collections WHERE id = ?1), narrator),
                    artist = COALESCE((SELECT artist FROM collections WHERE id = ?1), artist),
                    album = COALESCE((SELECT album FROM collections WHERE id = ?1), album),
                    series = COALESCE((SELECT series FROM collections WHERE id = ?1), series),
                    series_index = COALESCE((SELECT series_index FROM collections WHERE id = ?1), series_index),
                    cover_path = COALESCE((SELECT cover_path FROM collections WHERE id = ?1), cover_path),
                    listened_at = COALESCE((SELECT listened_at FROM collections WHERE id = ?1), listened_at)
                 WHERE id = ?2",
                params![from_id, to_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn absorb_kind_conflict(&mut self, source_id: i64, conflict_id: i64) -> Result<(), String> {
        if source_id == conflict_id {
            return Ok(());
        }
        self.copy_collection_metadata(conflict_id, source_id)?;
        self.conn_mut()
            .execute(
                "UPDATE collection_files SET collection_id = ?1 WHERE collection_id = ?2",
                params![source_id, conflict_id],
            )
            .map_err(|e| e.to_string())?;
        self.conn_mut()
            .execute("DELETE FROM collections WHERE id = ?1", [conflict_id])
            .map_err(|e| e.to_string())?;
        self.refresh_collection_paths_availability(source_id)?;
        Ok(())
    }

    pub fn collection_file_paths(&self, collection_id: i64) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn()
            .prepare("SELECT path FROM collection_files WHERE collection_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([collection_id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn set_collection_kind(&mut self, collection_id: i64, kind_str: &str) -> Result<(), String> {
        let new_kind = ContentKind::parse(kind_str).ok_or("Invalid content kind")?;
        if new_kind == ContentKind::Mixed {
            return Err("Choose audiobook or music".into());
        }
        let (root_id, title, old_kind): (i64, String, String) = self
            .conn()
            .query_row(
                "SELECT root_id, title, kind FROM collections WHERE id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        if old_kind == new_kind.as_str() {
            let now = now_unix();
            self.conn_mut()
                .execute(
                    "UPDATE collections SET is_manual = 1, updated_at = ?1 WHERE id = ?2",
                    params![now, collection_id],
                )
                .map_err(|e| e.to_string())?;
            self.refresh_collection_paths_availability(collection_id)?;
            return Ok(());
        }
        let conflict: Option<i64> = self
            .conn()
            .query_row(
                "SELECT id FROM collections WHERE root_id = ?1 AND title = ?2 AND kind = ?3 AND id != ?4",
                params![root_id, title, new_kind.as_str(), collection_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(conflict_id) = conflict {
            if self.can_absorb_kind_conflict(collection_id, conflict_id)? {
                self.absorb_kind_conflict(collection_id, conflict_id)?;
            } else {
                return Err(
                    "Another item with this title already exists under that type with different audio files."
                        .into(),
                );
            }
        }
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE collections SET kind = ?1, is_manual = 1, updated_at = ?2 WHERE id = ?3",
                params![new_kind.as_str(), now, collection_id],
            )
            .map_err(|e| e.to_string())?;
        let root_id: i64 = self
            .conn()
            .query_row(
                "SELECT root_id FROM collections WHERE id = ?1",
                [collection_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        self.refresh_collection_paths_availability(collection_id)?;
        self.reconcile_false_unavailable_in_root(root_id)?;
        self.reconcile_superseded_unavailable_files_in_root(root_id)?;
        self.prune_empty_collections_in_root(root_id)?;
        Ok(())
    }

    /// Prefer a manually classified collection (e.g. after type change) over an auto-detected duplicate.
    fn find_collection_id_for_upsert(
        &self,
        root_id: i64,
        title: &str,
        kind: ContentKind,
    ) -> Result<Option<i64>, String> {
        let manual: Option<i64> = self
            .conn()
            .query_row(
                "SELECT id FROM collections WHERE root_id = ?1 AND title = ?2 AND is_manual = 1
                 ORDER BY CASE WHEN kind = ?3 THEN 0 ELSE 1 END, id ASC
                 LIMIT 1",
                params![root_id, title, kind.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if manual.is_some() {
            return Ok(manual);
        }
        self.conn()
            .query_row(
                "SELECT id FROM collections WHERE root_id = ?1 AND title = ?2 AND kind = ?3",
                params![root_id, title, kind.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    fn reconcile_stale_files_after_scan(
        &mut self,
        root_id: i64,
        scan_started: i64,
    ) -> Result<(), String> {
        let now = now_unix();
        let stale: Vec<(i64, String, Option<String>, i64)> = {
            let mut stmt = self
                .conn()
                .prepare(
                    "SELECT cf.id, cf.path, cf.partial_hash, cf.file_size
                     FROM collection_files cf
                     INNER JOIN collections c ON c.id = cf.collection_id
                     WHERE c.root_id = ?1 AND cf.updated_at < ?2",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![root_id, scan_started], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .map_err(|e| e.to_string())?;
            rows.filter_map(|row| row.ok()).collect()
        };

        for (file_id, path_s, hash, size) in stale {
            let path = PathBuf::from(&path_s);
            if let Some(canon) = tracked_file_on_disk(&path) {
                let canon_s = canon.to_string_lossy().to_string();
                let meta = fs::metadata(&canon).ok();
                let file_size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(size);
                let mtime = meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let inode = file_inode(&canon);
                let partial_hash = partial_file_hash(&canon).or(hash);
                self.conn_mut()
                    .execute(
                        "UPDATE collection_files SET path = ?1, file_size = ?2, file_mtime = ?3, inode = ?4,
                         partial_hash = ?5, unavailable = 0, updated_at = ?6 WHERE id = ?7",
                        params![
                            canon_s,
                            file_size,
                            mtime,
                            inode,
                            partial_hash,
                            now,
                            file_id
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                if canon_s != path_s {
                    let _ = self.db.update_media_path(&path_s, &canon_s);
                }
                continue;
            }

            if let (Some(h), true) = (hash.as_deref(), size > 0) {
                let sibling_path: Option<String> = self
                    .conn()
                    .query_row(
                        "SELECT cf.path FROM collection_files cf
                         INNER JOIN collections c ON c.id = cf.collection_id
                         WHERE c.root_id = ?1 AND cf.partial_hash = ?2 AND cf.file_size = ?3
                         AND cf.updated_at >= ?4 AND cf.id != ?5
                         LIMIT 1",
                        params![root_id, h, size, scan_started, file_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?;
                if let Some(new_path) = sibling_path {
                    let _ = self.db.update_media_path(&path_s, &new_path);
                    self.conn_mut()
                        .execute("DELETE FROM collection_files WHERE id = ?1", [file_id])
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    fn mark_missing_files_unavailable_after_scan(
        &mut self,
        root_id: i64,
        scan_started: i64,
    ) -> Result<(), String> {
        let now = now_unix();
        let stale: Vec<(i64, String)> = {
            let mut stmt = self
                .conn()
                .prepare(
                    "SELECT cf.id, cf.path
                     FROM collection_files cf
                     INNER JOIN collections c ON c.id = cf.collection_id
                     WHERE c.root_id = ?1 AND cf.updated_at < ?2 AND cf.unavailable = 0",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![root_id, scan_started], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|row| row.ok()).collect()
        };
        for (file_id, path_s) in stale {
            if tracked_file_on_disk(Path::new(&path_s)).is_some() {
                continue;
            }
            self.conn_mut()
                .execute(
                    "UPDATE collection_files SET unavailable = 1, updated_at = ?1 WHERE id = ?2",
                    params![now, file_id],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn reconcile_false_unavailable_in_root(&mut self, root_id: i64) -> Result<(), String> {
        let rows: Vec<(i64, String)> = {
            let mut stmt = self
                .conn()
                .prepare(
                    "SELECT cf.id, cf.path
                     FROM collection_files cf
                     INNER JOIN collections c ON c.id = cf.collection_id
                     WHERE c.root_id = ?1 AND cf.unavailable = 1",
                )
                .map_err(|e| e.to_string())?;
            let mapped = stmt
                .query_map([root_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?;
            mapped.filter_map(|row| row.ok()).collect()
        };
        let now = now_unix();
        for (file_id, path_s) in rows {
            let path = Path::new(&path_s);
            let Some(canon) = tracked_file_on_disk(path) else {
                continue;
            };
            let canon_s = canon.to_string_lossy().to_string();
            self.conn_mut()
                .execute(
                    "UPDATE collection_files SET path = ?1, unavailable = 0, updated_at = ?2 WHERE id = ?3",
                    params![canon_s, now, file_id],
                )
                .map_err(|e| e.to_string())?;
            if canon_s != path_s {
                let _ = self.db.update_media_path(&path_s, &canon_s);
            }
        }
        Ok(())
    }

    /// Drop unavailable file rows superseded by an available copy in the same library root (e.g. after rescan or type change).
    fn reconcile_superseded_unavailable_files_in_root(&mut self, root_id: i64) -> Result<(), String> {
        let stale_by_path: Vec<i64> = {
            let mut stmt = self
                .conn()
                .prepare(
                    "SELECT cf.id
                     FROM collection_files cf
                     INNER JOIN collections c ON c.id = cf.collection_id
                     WHERE c.root_id = ?1 AND cf.unavailable = 1
                     AND EXISTS (
                         SELECT 1 FROM collection_files cf2
                         INNER JOIN collections c2 ON c2.id = cf2.collection_id
                         WHERE c2.root_id = ?1 AND cf2.unavailable = 0
                         AND cf2.path = cf.path AND cf2.id != cf.id
                     )",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([root_id], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|row| row.ok()).collect()
        };
        for file_id in stale_by_path {
            self.conn_mut()
                .execute("DELETE FROM collection_files WHERE id = ?1", [file_id])
                .map_err(|e| e.to_string())?;
        }

        let stale_by_hash: Vec<i64> = {
            let mut stmt = self
                .conn()
                .prepare(
                    "SELECT cf.id
                     FROM collection_files cf
                     INNER JOIN collections c ON c.id = cf.collection_id
                     WHERE c.root_id = ?1 AND cf.unavailable = 1
                     AND cf.partial_hash IS NOT NULL AND cf.file_size > 0
                     AND EXISTS (
                         SELECT 1 FROM collection_files cf2
                         INNER JOIN collections c2 ON c2.id = cf2.collection_id
                         WHERE c2.root_id = ?1 AND cf2.unavailable = 0
                         AND cf2.partial_hash = cf.partial_hash AND cf2.file_size = cf.file_size
                         AND cf2.id != cf.id
                     )",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([root_id], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|row| row.ok()).collect()
        };
        for file_id in stale_by_hash {
            self.conn_mut()
                .execute("DELETE FROM collection_files WHERE id = ?1", [file_id])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn prune_empty_collections_in_root(&mut self, root_id: i64) -> Result<(), String> {
        self.conn_mut()
            .execute(
                "DELETE FROM collections
                 WHERE root_id = ?1
                 AND id NOT IN (SELECT DISTINCT collection_id FROM collection_files)",
                [root_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn prune_empty_collections(&mut self) -> Result<(), String> {
        self.conn_mut()
            .execute(
                "DELETE FROM collections
                 WHERE id NOT IN (SELECT DISTINCT collection_id FROM collection_files)",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn reconcile_library_health(&mut self) -> Result<(), String> {
        let root_ids: Vec<i64> = {
            let mut stmt = self
                .conn()
                .prepare("SELECT id FROM library_roots")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|row| row.ok()).collect()
        };
        for root_id in root_ids {
            self.reconcile_false_unavailable_in_root(root_id)?;
            self.reconcile_superseded_unavailable_files_in_root(root_id)?;
            self.prune_empty_collections_in_root(root_id)?;
        }
        self.prune_empty_collections()?;
        Ok(())
    }

    fn refresh_collection_paths_availability(&mut self, collection_id: i64) -> Result<(), String> {
        let now = now_unix();
        let (root_id, root_path): (i64, String) = self
            .conn()
            .query_row(
                "SELECT c.root_id, lr.path FROM collections c
                 JOIN library_roots lr ON lr.id = c.root_id WHERE c.id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if Path::new(&root_path).exists() {
            self.conn_mut()
                .execute(
                    "UPDATE library_roots SET is_available = 1, updated_at = ?1 WHERE id = ?2",
                    params![now, root_id],
                )
                .map_err(|e| e.to_string())?;
        }
        let rows: Vec<(i64, String)> = {
            let mut stmt = self
                .conn()
                .prepare("SELECT id, path FROM collection_files WHERE collection_id = ?1")
                .map_err(|e| e.to_string())?;
            let mapped = stmt
                .query_map([collection_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?;
            mapped.filter_map(|row| row.ok()).collect()
        };
        for (file_id, path_s) in rows {
            let path = Path::new(&path_s);
            let Some(canon) = tracked_file_on_disk(path) else {
                continue;
            };
            let canon_s = canon.to_string_lossy().to_string();
            let meta = fs::metadata(&canon).ok();
            let file_size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let inode = file_inode(&canon);
            let partial_hash = partial_file_hash(&canon);
            self.conn_mut()
                .execute(
                    "UPDATE collection_files SET path = ?1, file_size = ?2, file_mtime = ?3, inode = ?4,
                     partial_hash = ?5, unavailable = 0, updated_at = ?6 WHERE id = ?7",
                    params![
                        canon_s,
                        file_size,
                        mtime,
                        inode,
                        partial_hash,
                        now,
                        file_id
                    ],
                )
                .map_err(|e| e.to_string())?;
            if canon_s != path_s {
                let _ = self.db.update_media_path(&path_s, &canon_s);
            }
        }
        Ok(())
    }

    fn upsert_collection(
        &mut self,
        root_id: i64,
        kind: ContentKind,
        title: &str,
        layout: LayoutKind,
        files: &[ScannedFile],
        meta: Option<CollectionMetadataInput>,
        scan_started: Option<i64>,
    ) -> Result<usize, String> {
        if files.is_empty() {
            return Ok(0);
        }
        let now = scan_started
            .map(|s| now_unix().max(s))
            .unwrap_or_else(now_unix);
        let sort_title = title.to_lowercase();
        let existing = self.find_collection_id_for_upsert(root_id, title, kind)?;
        let conn = self.conn_mut();

        let collection_id = if let Some(id) = existing {
            let is_manual: i32 = conn
                .query_row(
                    "SELECT is_manual FROM collections WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if is_manual != 0 {
                conn.execute(
                    "UPDATE collections SET updated_at = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(|e| e.to_string())?;
            } else {
                conn.execute(
                    "UPDATE collections SET layout_kind = ?1, updated_at = ?2 WHERE id = ?3",
                    params![layout.as_str(), now, id],
                )
                .map_err(|e| e.to_string())?;
            }
            id
        } else {
            let author = meta.as_ref().and_then(|m| m.author.clone());
            let narrator = meta.as_ref().and_then(|m| m.narrator.clone());
            let artist = meta.as_ref().and_then(|m| m.artist.clone());
            let album = meta.as_ref().and_then(|m| m.album.clone());
            conn.execute(
                "INSERT INTO collections (root_id, kind, title, sort_title, layout_kind, author, narrator, artist, album, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
                params![
                    root_id,
                    kind.as_str(),
                    title,
                    sort_title,
                    layout.as_str(),
                    author,
                    narrator,
                    artist,
                    album,
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        };

        for (order, sf) in files.iter().enumerate() {
            self.upsert_collection_file(collection_id, order as i32, sf, now)?;
        }
        let _ = self.try_extract_cover(collection_id, files);
        Ok(1)
    }

    fn upsert_collection_file(
        &mut self,
        collection_id: i64,
        track_order: i32,
        sf: &ScannedFile,
        now: i64,
    ) -> Result<(), String> {
        let path = &sf.path;
        let exists = path.exists();
        let meta = fs::metadata(path).ok();
        let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let inode = file_inode(path);
        let hash = partial_file_hash(path);
        let path_s = path.to_string_lossy().to_string();

        let (root_id, collection_manual): (i64, bool) = self
            .conn()
            .query_row(
                "SELECT root_id, is_manual FROM collections WHERE id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get::<_, i32>(1)? != 0)),
            )
            .map_err(|e| e.to_string())?;

        let file_id = {
            let conn = self.conn_mut();
            let existing_id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM collection_files WHERE path = ?1",
                    [&path_s],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;

            let relink_id: Option<i64> = if existing_id.is_none() {
                if let (Some(ref h), true) = (&hash, size > 0) {
                    conn.query_row(
                        "SELECT cf.id FROM collection_files cf
                         INNER JOIN collections c ON c.id = cf.collection_id
                         WHERE c.root_id = ?1 AND cf.partial_hash = ?2 AND cf.file_size = ?3
                         AND cf.path != ?4
                         LIMIT 1",
                        params![root_id, h, size, path_s],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(fid) = existing_id.or(relink_id) {
                let old_path: Option<String> = if relink_id.is_some() {
                    conn.query_row(
                        "SELECT path FROM collection_files WHERE id = ?1",
                        [fid],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| e.to_string())?
                } else {
                    None
                };
                let title_manual: i32 = conn
                    .query_row(
                        "SELECT title_manual FROM collection_files WHERE id = ?1",
                        [fid],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let preserve_titles = collection_manual || title_manual != 0;
                let (display_title, label, order, disc_index, track_index) = if preserve_titles {
                    conn.query_row(
                        "SELECT display_title, label, track_order, disc_index, track_index
                         FROM collection_files WHERE id = ?1",
                        [fid],
                        |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, String>(1)?,
                                r.get(2)?,
                                r.get(3)?,
                                r.get(4)?,
                            ))
                        },
                    )
                    .map_err(|e| e.to_string())?
                } else {
                    (
                        sf.display_title.clone(),
                        sf.label.clone(),
                        track_order,
                        sf.disc_index,
                        sf.track_index,
                    )
                };
                conn.execute(
                    "UPDATE collection_files SET collection_id = ?1, path = ?2, display_title = ?3, label = ?4,
                     track_order = ?5, disc_index = ?6, track_index = ?7, file_size = ?8, file_mtime = ?9,
                     inode = ?10, partial_hash = ?11, unavailable = ?12, updated_at = ?13 WHERE id = ?14",
                    params![
                        collection_id,
                        path_s,
                        display_title,
                        label,
                        order,
                        disc_index,
                        track_index,
                        size,
                        mtime,
                        inode,
                        hash,
                        (!exists) as i32,
                        now,
                        fid,
                    ],
                )
                .map_err(|e| e.to_string())?;
                if let Some(old) = old_path {
                    if old != path_s {
                        let _ = self.db.update_media_path(&old, &path_s);
                    }
                }
                fid
            } else {
                conn.execute(
                    "INSERT INTO collection_files (collection_id, path, display_title, label, track_order,
                     disc_index, track_index, file_size, file_mtime, inode, partial_hash, unavailable,
                     title_manual, created_at, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0,?13,?13)",
                    params![
                        collection_id,
                        path_s,
                        sf.display_title,
                        sf.label,
                        track_order,
                        sf.disc_index,
                        sf.track_index,
                        size,
                        mtime,
                        inode,
                        hash,
                        (!exists) as i32,
                        now,
                    ],
                )
                .map_err(|e| e.to_string())?;
                conn.last_insert_rowid()
            }
        };
        self.db
            .link_file_identity(&path_s, file_id)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn location_hint_for_root(&self, root_id: i64, root_unavailable: bool, tracks_missing: bool) -> String {
        if root_unavailable {
            return "away".into();
        }
        if tracks_missing {
            return "missing".into();
        }
        self.conn()
            .query_row(
                "SELECT label FROM library_roots WHERE id = ?1",
                [root_id],
                |r| r.get::<_, String>(0),
            )
            .map(|label| {
                if label.to_lowercase().contains("ssd") || label.to_lowercase().contains("external") {
                    "external".into()
                } else {
                    "local".into()
                }
            })
            .unwrap_or_else(|_| "local".into())
    }

    fn collection_progress(&self, collection_id: i64) -> Result<(f64, bool, bool, i64), String> {
        let conn = self.conn();
        let listened: Option<i64> = conn
            .query_row(
                "SELECT listened_at FROM collections WHERE id = ?1",
                [collection_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        if listened.is_some() {
            return Ok((100.0, true, false, 0));
        }

        let mut stmt = conn
            .prepare(
                "SELECT cf.path, cf.unavailable, ms.position_sec, ms.duration_sec, ms.listened_at
                 FROM collection_files cf
                 LEFT JOIN media_state ms ON ms.file_key = cf.path
                 WHERE cf.collection_id = ?1 ORDER BY cf.track_order",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, i32, f64, Option<f64>, Option<i64>)> = stmt
            .query_map([collection_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                    r.get(3)?,
                    r.get(4)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();

        if rows.is_empty() {
            return Ok((0.0, false, false, 0));
        }

        let mut total_dur = 0.0f64;
        let mut total_pos = 0.0f64;
        let mut any_progress = false;
        let mut all_listened = true;

        let mut stmt_updated = conn
            .prepare(
                "SELECT MAX(ms.updated_at) FROM collection_files cf
                 LEFT JOIN media_state ms ON ms.file_key = cf.path
                 WHERE cf.collection_id = ?1 AND cf.unavailable = 0 AND ms.position_sec > 1.0",
            )
            .map_err(|e| e.to_string())?;
        let last_played = stmt_updated
            .query_row([collection_id], |r| r.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
            .unwrap_or(0);

        for (_path, unavail, pos, dur, listened_at) in &rows {
            if *unavail != 0 {
                all_listened = false;
                continue;
            }
            if let Some(d) = dur {
                if *d > 0.0 {
                    total_dur += d;
                    total_pos += pos.min(*d);
                }
            }
            if *pos > 1.0 {
                any_progress = true;
            }
            if listened_at.is_none() {
                all_listened = false;
            }
        }

        let pct = if total_dur > 0.0 {
            (total_pos / total_dur * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let finished = all_listened && rows.iter().any(|r| r.1 == 0);
        Ok((pct, finished, any_progress && !finished, last_played))
    }

    pub fn list_collections(
        &mut self,
        kind: Option<&str>,
        filter: Option<&str>,
        search: Option<&str>,
        series: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CollectionSummaryDto>, String> {
        self.reconcile_library_health()?;
        let mut sql = String::from(
            "SELECT c.id, c.root_id, c.kind, c.title, c.author, c.artist, c.album, c.layout_kind,
                    c.cover_path, c.listened_at, r.is_available, c.series, c.series_index
             FROM collections c
             JOIN library_roots r ON r.id = c.root_id WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(k) = kind {
            sql.push_str(" AND c.kind = ?");
            params_vec.push(Box::new(k.to_string()));
        }
        if let Some(s) = search {
            let q = format!("%{}%", s.trim().to_lowercase());
            sql.push_str(" AND (LOWER(c.title) LIKE ? OR LOWER(COALESCE(c.author,'')) LIKE ? OR LOWER(COALESCE(c.artist,'')) LIKE ? OR LOWER(COALESCE(c.series,'')) LIKE ?)");
            params_vec.push(Box::new(q.clone()));
            params_vec.push(Box::new(q.clone()));
            params_vec.push(Box::new(q.clone()));
            params_vec.push(Box::new(q));
        }
        if let Some(ser) = series.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND LOWER(c.series) = LOWER(?)");
            params_vec.push(Box::new(ser.trim().to_string()));
        }
        let order = if series.is_some() {
            " ORDER BY c.series_index, c.sort_title"
        } else {
            " ORDER BY c.sort_title"
        };
        sql.push_str(order);
        let fetch_limit = if filter.is_some() {
            (limit * 8).clamp(limit, 4000)
        } else {
            limit
        };
        sql.push_str(" LIMIT ? OFFSET ?");
        params_vec.push(Box::new(fetch_limit));
        params_vec.push(Box::new(offset));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let mut out = Vec::new();
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                    r.get::<_, i32>(10)? != 0,
                    r.get::<_, Option<String>>(11)?,
                    r.get::<_, Option<i32>>(12)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        for row in rows.flatten() {
            let (
                id,
                root_id,
                kind_s,
                title,
                author,
                artist,
                _album,
                layout,
                cover,
                _listened_at,
                root_avail,
                series_name,
                series_index,
            ) = row;
            let (progress_pct, listened, in_progress, last_played) = self.collection_progress(id)?;
            let playable_file_count: i64 = self
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM collection_files WHERE collection_id = ?1 AND unavailable = 0",
                    [id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let track_count: i64 = self
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM collection_files WHERE collection_id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let missing_file_count = track_count - playable_file_count;
            let root_unavailable = !root_avail;
            let tracks_missing = root_avail && playable_file_count == 0 && track_count > 0;
            let unavailable = root_unavailable || playable_file_count == 0;
            let subtitle = author.or(artist);
            if let Some(f) = filter {
                match f {
                    "in-progress" if !show_in_progress_list(in_progress, listened, progress_pct) => {
                        continue
                    }
                    "finished" if !listened => continue,
                    "away" if !unavailable => continue,
                    "series" if series_name.is_none() => continue,
                    "all" | _ => {}
                }
            }
            out.push(CollectionSummaryDto {
                id,
                root_id,
                kind: kind_s,
                title,
                subtitle,
                layout_kind: layout,
                cover_path: cover,
                progress_pct,
                listened,
                in_progress,
                unavailable,
                root_unavailable,
                playable_file_count,
                missing_file_count,
                location_hint: self.location_hint_for_root(root_id, root_unavailable, tracks_missing),
                track_count,
                last_played_at: if last_played > 0 { Some(last_played) } else { None },
                series: series_name,
                series_index,
            });
            if out.len() as i64 >= limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn get_collection_detail(&mut self, collection_id: i64) -> Result<CollectionDetailDto, String> {
        let root_id: i64 = self
            .conn()
            .query_row(
                "SELECT root_id FROM collections WHERE id = ?1",
                [collection_id],
                |r| r.get(0),
            )
            .map_err(|_| "Collection not found".to_string())?;
        self.reconcile_false_unavailable_in_root(root_id)?;
        self.reconcile_superseded_unavailable_files_in_root(root_id)?;
        self.prune_empty_collections_in_root(root_id)?;
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT c.id, c.root_id, c.kind, c.title, c.author, c.narrator, c.artist, c.album,
                        c.series, c.series_index, c.layout_kind, c.cover_path, c.is_manual, r.is_available
                 FROM collections c JOIN library_roots r ON r.id = c.root_id WHERE c.id = ?1",
                [collection_id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, Option<i32>>(9)?,
                        r.get::<_, String>(10)?,
                        r.get::<_, Option<String>>(11)?,
                        r.get::<_, i32>(12)? != 0,
                        r.get::<_, i32>(13)? != 0,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;

        let (
            id,
            root_id,
            kind,
            title,
            author,
            narrator,
            artist,
            album,
            series,
            series_index,
            layout,
            cover,
            is_manual,
            root_avail,
        ) = row;
        let (progress_pct, listened, _, _) = self.collection_progress(id)?;
        let unavailable = !root_avail;

        let mut stmt = conn
            .prepare(
                "SELECT cf.id, cf.path, cf.display_title, cf.label, cf.track_order, cf.disc_index,
                        cf.track_index, cf.unavailable, ms.position_sec, ms.duration_sec, ms.listened_at
                 FROM collection_files cf
                 LEFT JOIN media_state ms ON ms.file_key = cf.path
                 WHERE cf.collection_id = ?1 ORDER BY cf.track_order",
            )
            .map_err(|e| e.to_string())?;
        let files: Vec<CollectionFileDto> = stmt
            .query_map([collection_id], |r| {
                let listened = r.get::<_, Option<i64>>(10)?.is_some();
                Ok(CollectionFileDto {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    display_title: r.get(2)?,
                    label: r.get(3)?,
                    track_order: r.get(4)?,
                    disc_index: r.get(5)?,
                    track_index: r.get(6)?,
                    unavailable: r.get::<_, i32>(7)? != 0,
                    duration_sec: r.get(8)?,
                    position_sec: r.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
                    listened,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();

        let missing_file_count = files.iter().filter(|f| f.unavailable).count() as i64;
        let playable_file_count = files.len() as i64 - missing_file_count;
        let all_files_missing = !files.is_empty() && playable_file_count == 0;
        Ok(CollectionDetailDto {
            id,
            root_id,
            kind,
            title,
            author,
            narrator,
            artist,
            album,
            series,
            series_index,
            layout_kind: layout,
            cover_path: cover,
            progress_pct,
            listened,
            unavailable: unavailable || all_files_missing,
            root_unavailable: unavailable,
            missing_file_count,
            playable_file_count,
            location_hint: self.location_hint_for_root(root_id, unavailable, all_files_missing),
            is_manual,
            files,
        })
    }

    pub fn get_home_summary(&mut self) -> Result<HomeSummaryDto, String> {
        let roots = self.list_roots()?;
        let has_library = !roots.is_empty();
        let audiobooks = self.list_collections(Some("audiobook"), None, None, None, 200, 0)?;
        let music = self.list_collections(Some("music"), None, None, None, 20, 0)?;

        let continue_item = audiobooks
            .iter()
            .filter(|c| !c.unavailable && c.in_progress)
            .max_by(|a, b| {
                let at = a.last_played_at.unwrap_or(0);
                let bt = b.last_played_at.unwrap_or(0);
                if at != bt {
                    return at.cmp(&bt);
                }
                a.progress_pct
                    .partial_cmp(&b.progress_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();

        let mut in_progress: Vec<CollectionSummaryDto> = audiobooks
            .iter()
            .filter(|c| {
                show_in_progress_list(c.in_progress, c.listened, c.progress_pct) && !c.unavailable
            })
            .filter(|c| continue_item.as_ref().map(|ci| ci.id != c.id).unwrap_or(true))
            .cloned()
            .collect();
        in_progress.sort_by(|a, b| {
            let at = a.last_played_at.unwrap_or(0);
            let bt = b.last_played_at.unwrap_or(0);
            bt.cmp(&at)
        });
        in_progress.truncate(6);

        Ok(HomeSummaryDto {
            continue_item,
            in_progress,
            music_shelf: music.into_iter().take(4).collect(),
            has_library,
            scan_in_progress: false,
        })
    }

    pub fn list_series_names(&self, kind: Option<&str>) -> Result<Vec<String>, String> {
        let mut sql = String::from(
            "SELECT DISTINCT c.series FROM collections c
             WHERE c.series IS NOT NULL AND TRIM(c.series) != ''",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(k) = kind {
            sql.push_str(" AND c.kind = ?");
            params.push(Box::new(k.to_string()));
        }
        sql.push_str(" ORDER BY LOWER(c.series)");
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// Validated playback paths: root must be available; each file must exist and stay under root.
    pub fn collection_playback_paths(
        &mut self,
        collection_id: i64,
    ) -> Result<(Vec<PathBuf>, PathBuf), String> {
        let (root_id, root_path_s, root_available): (i64, String, i32) = self
            .conn()
            .query_row(
                "SELECT c.root_id, lr.path, lr.is_available FROM collections c
                 JOIN library_roots lr ON lr.id = c.root_id WHERE c.id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "Collection not found".to_string())?;

        if root_available == 0 {
            return Err(
                "This item is on a folder that is not connected right now. Plug in the drive and refresh."
                    .into(),
            );
        }

        let root_canon =
            canonicalize_existing(Path::new(&root_path_s)).map_err(|e| e.to_string())?;

        let raw: Vec<PathBuf> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT path FROM collection_files WHERE collection_id = ?1 AND unavailable = 0 ORDER BY track_order",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([collection_id], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|x| x.ok())
                .map(PathBuf::from)
                .collect()
        };

        let mut playable = Vec::new();
        for path in raw {
            let Some(on_disk) = tracked_file_on_disk(&path) else {
                let _ = self.mark_file_unavailable_by_path(&path);
                continue;
            };
            if on_disk.to_string_lossy() != path.to_string_lossy() {
                let now = now_unix();
                let path_s = on_disk.to_string_lossy().to_string();
                let old_s = path.to_string_lossy().to_string();
                let _ = self.conn_mut().execute(
                    "UPDATE collection_files SET path = ?1, unavailable = 0, updated_at = ?2 WHERE path = ?3",
                    params![path_s, now, old_s],
                );
                let _ = self.db.update_media_path(&old_s, &path_s);
            } else {
                let now = now_unix();
                let path_s = path.to_string_lossy().to_string();
                let _ = self.conn_mut().execute(
                    "UPDATE collection_files SET unavailable = 0, updated_at = ?1 WHERE path = ?2",
                    params![now, path_s],
                );
            }
            let canon = canonicalize_under_root(&on_disk, &root_canon).map_err(|e| e.to_string())?;
            playable.push(canon);
        }

        if playable.is_empty() {
            let _ = root_id;
            return Err("No playable files in this collection".into());
        }
        Ok((playable, root_canon))
    }

    pub fn find_collection_file_ref_by_stored_path(
        &self,
        path_s: &str,
    ) -> Result<Option<(i64, bool)>, String> {
        self.conn()
            .query_row(
                "SELECT id, unavailable FROM collection_files WHERE path = ?1",
                [path_s],
                |r| Ok((r.get(0)?, r.get::<_, i32>(1)? != 0)),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn find_collection_file_ref_by_path(
        &self,
        path: &Path,
    ) -> Result<Option<(i64, bool)>, String> {
        let path_s = path.to_string_lossy().to_string();
        if let Some(found) = self.find_collection_file_ref_by_stored_path(&path_s)? {
            return Ok(Some(found));
        }
        if let Ok(canon) = canonicalize_existing(path) {
            let canon_s = canon.to_string_lossy().to_string();
            if canon_s != path_s {
                if let Some(found) = self.find_collection_file_ref_by_stored_path(&canon_s)? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    pub fn find_collection_file_id_by_path(&self, path: &Path) -> Result<Option<i64>, String> {
        Ok(self
            .find_collection_file_ref_by_path(path)?
            .map(|(id, _)| id))
    }

    pub fn remove_collection_file_from_library(
        &mut self,
        file_id: i64,
    ) -> Result<RemoveCollectionFileResult, String> {
        let (path, collection_id): (String, i64) = self
            .conn()
            .query_row(
                "SELECT path, collection_id FROM collection_files WHERE id = ?1",
                [file_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "Track not found".to_string())?;

        let now = now_unix();
        let _ = self
            .conn_mut()
            .execute("DELETE FROM media_state WHERE file_key = ?1", [&path]);
        self.conn_mut()
            .execute("DELETE FROM collection_files WHERE id = ?1", [file_id])
            .map_err(|e| e.to_string())?;

        let remaining: i64 = self
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM collection_files WHERE collection_id = ?1",
                [collection_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;

        let collection_removed = remaining == 0;
        if collection_removed {
            self.conn_mut()
                .execute("DELETE FROM collections WHERE id = ?1", [collection_id])
                .map_err(|e| e.to_string())?;
        } else {
            self.conn_mut()
                .execute(
                    "UPDATE collections SET updated_at = ?1 WHERE id = ?2",
                    params![now, collection_id],
                )
                .map_err(|e| e.to_string())?;
        }

        Ok(RemoveCollectionFileResult {
            collection_removed,
            removed_path: path,
        })
    }

    pub fn remove_collection_from_library(
        &mut self,
        collection_id: i64,
    ) -> Result<RemoveCollectionResult, String> {
        let root_id: i64 = self
            .conn()
            .query_row(
                "SELECT root_id FROM collections WHERE id = ?1",
                [collection_id],
                |r| r.get(0),
            )
            .map_err(|_| "Collection not found".to_string())?;

        let paths: Vec<String> = {
            let mut stmt = self
                .conn()
                .prepare("SELECT path FROM collection_files WHERE collection_id = ?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([collection_id], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|row| row.ok()).collect()
        };

        for path in &paths {
            let _ = self
                .conn_mut()
                .execute("DELETE FROM media_state WHERE file_key = ?1", [path]);
        }
        self.conn_mut()
            .execute("DELETE FROM collections WHERE id = ?1", [collection_id])
            .map_err(|e| e.to_string())?;
        self.prune_empty_collections_in_root(root_id)?;

        Ok(RemoveCollectionResult { removed_paths: paths })
    }

    pub fn find_relax_playlist_id(&self) -> Result<Option<i64>, String> {
        self.conn()
            .query_row(
                "SELECT id FROM user_playlists
                 WHERE (is_pinned = 1 OR LOWER(name) LIKE '%relax%')
                 AND (SELECT COUNT(*) FROM user_playlist_items i WHERE i.playlist_id = user_playlists.id) > 0
                 ORDER BY is_pinned DESC, name LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn find_collection_for_path(&self, path: &Path) -> Result<Option<i64>, String> {
        let path_s = path.to_string_lossy().to_string();
        if let Some(id) = self.find_collection_id_for_stored_path(&path_s)? {
            return Ok(Some(id));
        }
        if let Ok(canon) = canonicalize_existing(path) {
            let canon_s = canon.to_string_lossy().to_string();
            if canon_s != path_s {
                if let Some(id) = self.find_collection_id_for_stored_path(&canon_s)? {
                    return Ok(Some(id));
                }
            }
        }
        Ok(None)
    }

    fn find_collection_id_for_stored_path(&self, path_s: &str) -> Result<Option<i64>, String> {
        self.conn()
            .query_row(
                "SELECT cf.collection_id FROM collection_files cf
                 INNER JOIN collections c ON c.id = cf.collection_id
                 WHERE cf.path = ?1 AND cf.unavailable = 0
                 ORDER BY c.is_manual DESC,
                          CASE c.kind WHEN 'music' THEN 0 WHEN 'audiobook' THEN 1 ELSE 2 END,
                          cf.id ASC
                 LIMIT 1",
                [path_s],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Returns a collection id when every available file in that collection lives under `folder`.
    pub fn find_collection_for_folder(&self, folder: &Path) -> Result<Option<i64>, String> {
        let canon = canonicalize_existing(folder).map_err(|e| e.to_string())?;
        let prefix = format!("{}/", canon.to_string_lossy().trim_end_matches('/'));
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT cf.collection_id,
                        COUNT(*) AS total,
                        SUM(CASE WHEN cf.path LIKE ?1 || '%' THEN 1 ELSE 0 END) AS under_prefix
                 FROM collection_files cf
                 WHERE cf.unavailable = 0
                 GROUP BY cf.collection_id
                 HAVING total = under_prefix AND total > 0",
            )
            .map_err(|e| e.to_string())?;
        let matches: Vec<i64> = stmt
            .query_map([&prefix], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();
        if matches.len() == 1 {
            return Ok(Some(matches[0]));
        }
        if matches.len() > 1 {
            let folder_name = canon
                .file_name()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            for cid in matches {
                let title: String = self
                    .conn()
                    .query_row(
                        "SELECT LOWER(title) FROM collections WHERE id = ?1",
                        [cid],
                        |r| r.get(0),
                    )
                    .unwrap_or_default();
                if title == folder_name {
                    return Ok(Some(cid));
                }
            }
        }
        Ok(None)
    }

    pub fn update_collection_metadata(
        &mut self,
        collection_id: i64,
        meta: CollectionMetadataInput,
    ) -> Result<(), String> {
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE collections SET
                    title = COALESCE(?1, title),
                    sort_title = COALESCE(LOWER(?1), sort_title),
                    author = COALESCE(?2, author),
                    narrator = COALESCE(?3, narrator),
                    artist = COALESCE(?4, artist),
                    album = COALESCE(?5, album),
                    series = COALESCE(?6, series),
                    series_index = COALESCE(?7, series_index),
                    is_manual = 1,
                    updated_at = ?8
                 WHERE id = ?9",
                params![
                    meta.title,
                    meta.author,
                    meta.narrator,
                    meta.artist,
                    meta.album,
                    meta.series,
                    meta.series_index,
                    now,
                    collection_id,
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn mark_collection_listened(&mut self, collection_id: i64, listened: bool) -> Result<(), String> {
        let now = now_unix();
        let when: Option<i64> = if listened { Some(now) } else { None };
        self.conn_mut()
            .execute(
                "UPDATE collections SET listened_at = ?1, updated_at = ?2 WHERE id = ?3",
                params![when, now, collection_id],
            )
            .map_err(|e| e.to_string())?;
        let paths: Vec<String> = {
            let conn = self.conn();
            conn.prepare("SELECT path FROM collection_files WHERE collection_id = ?1")
                .map_err(|e| e.to_string())?
                .query_map([collection_id], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .filter_map(|x| x.ok())
                .collect()
        };
        for p in paths {
            let when = if listened { Some(now) } else { None };
            let _ = self.db.set_listened_at(&p, when, now);
        }
        Ok(())
    }

    pub fn mark_file_unavailable_by_path(&mut self, path: &Path) -> Result<(), String> {
        let path_s = path.to_string_lossy().to_string();
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE collection_files SET unavailable = 1, updated_at = ?1 WHERE path = ?2",
                params![now, path_s],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_playlists(&self) -> Result<Vec<PlaylistSummaryDto>, String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.name, p.kind, p.is_pinned,
                        (SELECT COUNT(*) FROM user_playlist_items i WHERE i.playlist_id = p.id),
                        (SELECT COUNT(*) FROM user_playlist_items i
                         JOIN collection_files cf ON cf.id = i.collection_file_id
                         WHERE i.playlist_id = p.id AND cf.unavailable = 1)
                 FROM user_playlists p ORDER BY p.is_pinned DESC, p.name",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PlaylistSummaryDto {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    is_pinned: r.get::<_, i32>(3)? != 0,
                    track_count: r.get(4)?,
                    unavailable_count: r.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn create_playlist(&mut self, name: &str, pin: bool) -> Result<i64, String> {
        let now = now_unix();
        self.conn_mut()
            .execute(
                "INSERT INTO user_playlists (name, kind, is_pinned, created_at, updated_at) VALUES (?1,'music',?2,?3,?3)",
                params![name.trim(), pin as i32, now],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.conn().last_insert_rowid())
    }

    pub fn add_to_playlist(&mut self, playlist_id: i64, collection_file_id: i64) -> Result<(), String> {
        self.try_add_to_playlist(playlist_id, collection_file_id)?;
        Ok(())
    }

    fn try_add_to_playlist(&mut self, playlist_id: i64, collection_file_id: i64) -> Result<bool, String> {
        let exists: bool = self
            .conn()
            .query_row(
                "SELECT 1 FROM collection_files WHERE id = ?1",
                [collection_file_id],
                |_| Ok(()),
            )
            .is_ok();
        if !exists {
            return Err("Track not found in library".into());
        }
        let order: i32 = self
            .conn()
            .query_row(
                "SELECT COALESCE(MAX(track_order), -1) + 1 FROM user_playlist_items WHERE playlist_id = ?1",
                [playlist_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let now = now_unix();
        let inserted = self
            .conn_mut()
            .execute(
                "INSERT OR IGNORE INTO user_playlist_items (playlist_id, collection_file_id, track_order, added_at)
                 VALUES (?1,?2,?3,?4)",
                params![playlist_id, collection_file_id, order, now],
            )
            .map_err(|e| e.to_string())?;
        Ok(inserted > 0)
    }

    fn find_root_covering_path(&self, path: &Path) -> Result<Option<i64>, String> {
        let canon = canonicalize_existing(path).map_err(|e| e.to_string())?;
        let mut best: Option<(i64, usize)> = None;
        for root in self.list_roots()? {
            if let Ok(rc) = canonicalize_existing(Path::new(&root.path)) {
                if canon.starts_with(&rc) {
                    let len = rc.as_os_str().len();
                    if best.map(|(_, l)| len > l).unwrap_or(true) {
                        best = Some((root.id, len));
                    }
                }
            }
        }
        Ok(best.map(|(id, _)| id))
    }

    fn collection_file_ids_under_folder(&self, folder: &Path) -> Result<Vec<i64>, String> {
        let canon = canonicalize_existing(folder).map_err(|e| e.to_string())?;
        let prefix = format!("{}/", canon.to_string_lossy().trim_end_matches('/'));
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT cf.id FROM collection_files cf
                 JOIN collections c ON c.id = cf.collection_id
                 WHERE cf.unavailable = 0 AND cf.path LIKE ?1 || '%'
                 ORDER BY c.sort_title, cf.track_order, cf.path",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&prefix], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }

    fn ensure_folder_catalogued(&mut self, folder: &Path) -> Result<bool, String> {
        if !self.collection_file_ids_under_folder(folder)?.is_empty() {
            return Ok(false);
        }
        if let Some(root_id) = self.find_root_covering_path(folder)? {
            self.scan_root(root_id)?;
            if !self.collection_file_ids_under_folder(folder)?.is_empty() {
                return Ok(false);
            }
        }
        self.add_to_library(AddToLibraryInput {
            path: folder.to_string_lossy().to_string(),
            content_kind: "music".to_string(),
            grouping: "file-is-item".to_string(),
            metadata: CollectionMetadataInput {
                title: None,
                author: None,
                narrator: None,
                artist: None,
                album: None,
                series: None,
                series_index: None,
            },
        })?;
        Ok(true)
    }

    pub fn import_folder_to_playlist(
        &mut self,
        playlist_id: i64,
        folder: &Path,
    ) -> Result<ImportFolderToPlaylistResult, String> {
        let canon = canonicalize_existing(folder).map_err(|e| e.to_string())?;
        if !canon.is_dir() {
            return Err("Selected path is not a folder".into());
        }
        let playlist_exists: bool = self
            .conn()
            .query_row(
                "SELECT 1 FROM user_playlists WHERE id = ?1",
                [playlist_id],
                |_| Ok(()),
            )
            .is_ok();
        if !playlist_exists {
            return Err("Playlist not found".into());
        }
        let library_linked = self.ensure_folder_catalogued(&canon)?;
        let file_ids = self.collection_file_ids_under_folder(&canon)?;
        if file_ids.is_empty() {
            return Err("No audio files found in this folder".into());
        }
        let mut tracks_added = 0i64;
        let mut tracks_skipped = 0i64;
        for fid in file_ids {
            if self.try_add_to_playlist(playlist_id, fid)? {
                tracks_added += 1;
            } else {
                tracks_skipped += 1;
            }
        }
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE user_playlists SET updated_at = ?1 WHERE id = ?2",
                params![now, playlist_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(ImportFolderToPlaylistResult {
            folder_path: canon.to_string_lossy().to_string(),
            tracks_added,
            tracks_skipped,
            tracks_total: tracks_added + tracks_skipped,
            library_linked,
        })
    }

    fn album_artist_expr() -> &'static str {
        "COALESCE(NULLIF(TRIM(c.artist), ''), 'Unknown Artist')"
    }

    fn album_name_expr() -> &'static str {
        "COALESCE(NULLIF(TRIM(c.album), ''), NULLIF(TRIM(c.title), ''), 'Unknown Album')"
    }

    fn playlist_name_for_album(artist: &str, album: &str) -> String {
        if artist == "Unknown Artist" {
            album.to_string()
        } else {
            format!("{artist} – {album}")
        }
    }

    fn author_expr() -> &'static str {
        "COALESCE(NULLIF(TRIM(c.author), ''), 'Unknown Author')"
    }

    fn narrator_expr() -> &'static str {
        "COALESCE(NULLIF(TRIM(c.narrator), ''), 'Unknown Narrator')"
    }

    fn series_expr() -> &'static str {
        "COALESCE(NULLIF(TRIM(c.series), ''), 'Unknown Series')"
    }

    fn metadata_group_key(kind: &str, value: &str) -> String {
        format!("{kind}\0{value}")
    }

    fn parse_metadata_group_key(group_key: &str) -> Result<(&str, &str), String> {
        let (kind, value) = group_key
            .split_once('\0')
            .ok_or_else(|| "Invalid metadata group key".to_string())?;
        if value.trim().is_empty() {
            return Err("Invalid metadata group key".into());
        }
        Ok((kind, value))
    }

    fn playlist_name_for_metadata_group(
        &self,
        group_kind: &str,
        group_key: &str,
    ) -> Result<String, String> {
        let (kind, value) = Self::parse_metadata_group_key(group_key)?;
        if kind != group_kind {
            return Err("Metadata group kind mismatch".into());
        }
        Ok(match kind {
            "album" => {
                let (artist, album) = value
                    .split_once('\0')
                    .ok_or_else(|| "Invalid album group key".to_string())?;
                Self::playlist_name_for_album(artist, album)
            }
            "artist" => value.to_string(),
            "audiobook" => {
                let collection_id: i64 = value
                    .parse()
                    .map_err(|_| "Invalid audiobook group key".to_string())?;
                self.conn()
                    .query_row(
                        "SELECT title FROM collections WHERE id = ?1 AND kind = 'audiobook'",
                        [collection_id],
                        |r| r.get(0),
                    )
                    .map_err(|_| "Audiobook not found".to_string())?
            }
            "author" => value.to_string(),
            "narrator" => value.to_string(),
            "series" => value.to_string(),
            _ => return Err(format!("Unknown metadata group kind: {kind}")),
        })
    }

    fn metadata_group_file_ids(&self, group_kind: &str, group_key: &str) -> Result<Vec<i64>, String> {
        let (kind, value) = Self::parse_metadata_group_key(group_key)?;
        if kind != group_kind {
            return Err("Metadata group kind mismatch".into());
        }
        let artist_expr = Self::album_artist_expr();
        let album_expr = Self::album_name_expr();
        let author_expr = Self::author_expr();
        let narrator_expr = Self::narrator_expr();
        let series_expr = Self::series_expr();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match kind {
            "album" => {
                let (artist, album) = value
                    .split_once('\0')
                    .ok_or_else(|| "Invalid album group key".to_string())?;
                (
                    format!(
                        "SELECT cf.id FROM collection_files cf
                         JOIN collections c ON c.id = cf.collection_id
                         JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                         WHERE c.kind = 'music' AND cf.unavailable = 0
                           AND LOWER({artist_expr}) = LOWER(?1)
                           AND LOWER({album_expr}) = LOWER(?2)
                         ORDER BY LOWER({artist_expr}), LOWER({album_expr}), c.sort_title, cf.disc_index, cf.track_order, cf.path"
                    ),
                    vec![Box::new(artist.to_string()), Box::new(album.to_string())],
                )
            }
            "artist" => (
                format!(
                    "SELECT cf.id FROM collection_files cf
                     JOIN collections c ON c.id = cf.collection_id
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'music' AND cf.unavailable = 0
                       AND LOWER({artist_expr}) = LOWER(?1)
                     ORDER BY LOWER({artist_expr}), LOWER({album_expr}), c.sort_title, cf.disc_index, cf.track_order, cf.path"
                ),
                vec![Box::new(value.to_string())],
            ),
            "audiobook" => {
                let collection_id: i64 = value
                    .parse()
                    .map_err(|_| "Invalid audiobook group key".to_string())?;
                (
                    "SELECT cf.id FROM collection_files cf
                     JOIN collections c ON c.id = cf.collection_id
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE cf.collection_id = ?1 AND c.kind = 'audiobook' AND cf.unavailable = 0
                     ORDER BY cf.disc_index, cf.track_order, cf.path"
                        .to_string(),
                    vec![Box::new(collection_id)],
                )
            }
            "author" => (
                format!(
                    "SELECT cf.id FROM collection_files cf
                     JOIN collections c ON c.id = cf.collection_id
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook' AND cf.unavailable = 0
                       AND LOWER({author_expr}) = LOWER(?1)
                     ORDER BY LOWER({author_expr}), c.sort_title, cf.disc_index, cf.track_order, cf.path"
                ),
                vec![Box::new(value.to_string())],
            ),
            "narrator" => (
                format!(
                    "SELECT cf.id FROM collection_files cf
                     JOIN collections c ON c.id = cf.collection_id
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook' AND cf.unavailable = 0
                       AND LOWER({narrator_expr}) = LOWER(?1)
                     ORDER BY LOWER({narrator_expr}), c.sort_title, cf.disc_index, cf.track_order, cf.path"
                ),
                vec![Box::new(value.to_string())],
            ),
            "series" => (
                format!(
                    "SELECT cf.id FROM collection_files cf
                     JOIN collections c ON c.id = cf.collection_id
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook' AND cf.unavailable = 0
                       AND LOWER({series_expr}) = LOWER(?1)
                     ORDER BY LOWER({series_expr}), c.series_index, c.sort_title, cf.disc_index, cf.track_order, cf.path"
                ),
                vec![Box::new(value.to_string())],
            ),
            _ => return Err(format!("Unknown metadata group kind: {kind}")),
        };
        let mut stmt = self.conn().prepare(&sql).map_err(|e| e.to_string())?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn list_metadata_groups(
        &self,
        group_kind: &str,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MetadataGroupDto>, String> {
        let artist_expr = Self::album_artist_expr();
        let album_expr = Self::album_name_expr();
        let author_expr = Self::author_expr();
        let narrator_expr = Self::narrator_expr();
        let series_expr = Self::series_expr();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let sql = match group_kind {
            "album" => {
                let mut sql = format!(
                    "SELECT {artist_expr} AS label, {album_expr} AS subtitle, COUNT(cf.id) AS track_count
                     FROM collections c
                     JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'music'"
                );
                if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
                    let q = format!("%{}%", s.trim().to_lowercase());
                    sql.push_str(&format!(
                        " AND (LOWER({artist_expr}) LIKE ? OR LOWER({album_expr}) LIKE ?)"
                    ));
                    params_vec.push(Box::new(q.clone()));
                    params_vec.push(Box::new(q));
                }
                sql.push_str(&format!(
                    " GROUP BY LOWER({artist_expr}), LOWER({album_expr})
                      ORDER BY LOWER({artist_expr}), LOWER({album_expr})
                      LIMIT ? OFFSET ?"
                ));
                sql
            }
            "artist" => {
                let mut sql = format!(
                    "SELECT {artist_expr} AS label, NULL AS subtitle, COUNT(cf.id) AS track_count
                     FROM collections c
                     JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'music'"
                );
                if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
                    sql.push_str(&format!(" AND LOWER({artist_expr}) LIKE ?"));
                    params_vec.push(Box::new(format!("%{}%", s.trim().to_lowercase())));
                }
                sql.push_str(&format!(
                    " GROUP BY LOWER({artist_expr})
                      ORDER BY LOWER({artist_expr})
                      LIMIT ? OFFSET ?"
                ));
                sql
            }
            "audiobook" => {
                let mut sql = format!(
                    "SELECT c.title AS label, {author_expr} AS subtitle, COUNT(cf.id) AS track_count, c.id AS collection_id
                     FROM collections c
                     JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook'"
                );
                if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
                    let q = format!("%{}%", s.trim().to_lowercase());
                    sql.push_str(&format!(
                        " AND (LOWER(c.title) LIKE ? OR LOWER({author_expr}) LIKE ? OR LOWER({narrator_expr}) LIKE ?)"
                    ));
                    params_vec.push(Box::new(q.clone()));
                    params_vec.push(Box::new(q.clone()));
                    params_vec.push(Box::new(q));
                }
                sql.push_str(
                    " GROUP BY c.id
                      ORDER BY c.sort_title
                      LIMIT ? OFFSET ?",
                );
                sql
            }
            "author" => {
                let mut sql = format!(
                    "SELECT {author_expr} AS label, NULL AS subtitle, COUNT(cf.id) AS track_count
                     FROM collections c
                     JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook'"
                );
                if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
                    sql.push_str(&format!(" AND LOWER({author_expr}) LIKE ?"));
                    params_vec.push(Box::new(format!("%{}%", s.trim().to_lowercase())));
                }
                sql.push_str(&format!(
                    " GROUP BY LOWER({author_expr})
                      HAVING label != 'Unknown Author'
                      ORDER BY LOWER({author_expr})
                      LIMIT ? OFFSET ?"
                ));
                sql
            }
            "narrator" => {
                let mut sql = format!(
                    "SELECT {narrator_expr} AS label, NULL AS subtitle, COUNT(cf.id) AS track_count
                     FROM collections c
                     JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook'"
                );
                if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
                    sql.push_str(&format!(" AND LOWER({narrator_expr}) LIKE ?"));
                    params_vec.push(Box::new(format!("%{}%", s.trim().to_lowercase())));
                }
                sql.push_str(&format!(
                    " GROUP BY LOWER({narrator_expr})
                      HAVING label != 'Unknown Narrator'
                      ORDER BY LOWER({narrator_expr})
                      LIMIT ? OFFSET ?"
                ));
                sql
            }
            "series" => {
                let mut sql = format!(
                    "SELECT {series_expr} AS label, NULL AS subtitle, COUNT(cf.id) AS track_count
                     FROM collections c
                     JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE c.kind = 'audiobook' AND c.series IS NOT NULL AND TRIM(c.series) != ''"
                );
                if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
                    sql.push_str(&format!(" AND LOWER({series_expr}) LIKE ?"));
                    params_vec.push(Box::new(format!("%{}%", s.trim().to_lowercase())));
                }
                sql.push_str(&format!(
                    " GROUP BY LOWER({series_expr})
                      ORDER BY LOWER({series_expr})
                      LIMIT ? OFFSET ?"
                ));
                sql
            }
            _ => return Err(format!("Unknown metadata group kind: {group_kind}")),
        };

        params_vec.push(Box::new(limit));
        params_vec.push(Box::new(offset));
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        if group_kind == "audiobook" {
            let rows = stmt
                .query_map(param_refs.as_slice(), |r| {
                    let label: String = r.get(0)?;
                    let subtitle: Option<String> = r.get(1)?;
                    let track_count: i64 = r.get(2)?;
                    let collection_id: i64 = r.get(3)?;
                    let group_key = Self::metadata_group_key("audiobook", &collection_id.to_string());
                    Ok(MetadataGroupDto {
                        group_kind: "audiobook".to_string(),
                        group_key,
                        label,
                        subtitle: subtitle.filter(|s| s != "Unknown Author"),
                        track_count,
                    })
                })
                .map_err(|e| e.to_string())?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string());
        }

        if group_kind == "album" {
            let rows = stmt
                .query_map(param_refs.as_slice(), |r| {
                    let artist: String = r.get(0)?;
                    let album: String = r.get(1)?;
                    let track_count: i64 = r.get(2)?;
                    let group_key = Self::metadata_group_key(
                        "album",
                        &format!("{artist}\0{album}"),
                    );
                    let label = if artist == "Unknown Artist" {
                        album.clone()
                    } else {
                        format!("{artist} – {album}")
                    };
                    Ok(MetadataGroupDto {
                        group_kind: "album".to_string(),
                        group_key,
                        label,
                        subtitle: None,
                        track_count,
                    })
                })
                .map_err(|e| e.to_string())?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string());
        }

        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                let label: String = r.get(0)?;
                let subtitle: Option<String> = r.get(1)?;
                let track_count: i64 = r.get(2)?;
                let group_key = Self::metadata_group_key(group_kind, &label);
                Ok(MetadataGroupDto {
                    group_kind: group_kind.to_string(),
                    group_key,
                    label: label.clone(),
                    subtitle,
                    track_count,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn add_metadata_group_to_playlist(
        &mut self,
        playlist_id: i64,
        group_kind: &str,
        group_key: &str,
    ) -> Result<AddToPlaylistBulkResult, String> {
        let file_ids = self.metadata_group_file_ids(group_kind, group_key)?;
        if file_ids.is_empty() {
            return Err("No tracks found for this selection".into());
        }
        self.add_file_ids_to_playlist(playlist_id, file_ids)
    }

    pub fn create_playlist_from_metadata_group(
        &mut self,
        group_kind: &str,
        group_key: &str,
    ) -> Result<i64, String> {
        let file_ids = self.metadata_group_file_ids(group_kind, group_key)?;
        if file_ids.is_empty() {
            return Err("No tracks found for this selection".into());
        }
        let name = self.ensure_unique_playlist_name(
            &self.playlist_name_for_metadata_group(group_kind, group_key)?,
        )?;
        let playlist_id = self.create_playlist(&name, false)?;
        self.add_file_ids_to_playlist(playlist_id, file_ids)?;
        Ok(playlist_id)
    }

    fn ensure_unique_playlist_name(&self, base: &str) -> Result<String, String> {
        let trimmed = base.trim();
        if trimmed.is_empty() {
            return Err("Playlist name cannot be empty".into());
        }
        let exists: bool = self
            .conn()
            .query_row(
                "SELECT 1 FROM user_playlists WHERE name = ?1",
                [trimmed],
                |_| Ok(()),
            )
            .is_ok();
        if !exists {
            return Ok(trimmed.to_string());
        }
        for n in 2..100 {
            let candidate = format!("{trimmed} ({n})");
            let taken: bool = self
                .conn()
                .query_row(
                    "SELECT 1 FROM user_playlists WHERE name = ?1",
                    [&candidate],
                    |_| Ok(()),
                )
                .is_ok();
            if !taken {
                return Ok(candidate);
            }
        }
        Err("Could not find a free playlist name".into())
    }

    fn touch_playlist(&mut self, playlist_id: i64) -> Result<(), String> {
        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE user_playlists SET updated_at = ?1 WHERE id = ?2",
                params![now, playlist_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn add_file_ids_to_playlist(
        &mut self,
        playlist_id: i64,
        file_ids: Vec<i64>,
    ) -> Result<AddToPlaylistBulkResult, String> {
        let playlist_exists: bool = self
            .conn()
            .query_row(
                "SELECT 1 FROM user_playlists WHERE id = ?1",
                [playlist_id],
                |_| Ok(()),
            )
            .is_ok();
        if !playlist_exists {
            return Err("Playlist not found".into());
        }
        let mut tracks_added = 0i64;
        let mut tracks_skipped = 0i64;
        for fid in file_ids {
            if self.try_add_to_playlist(playlist_id, fid)? {
                tracks_added += 1;
            } else {
                tracks_skipped += 1;
            }
        }
        if tracks_added > 0 {
            self.touch_playlist(playlist_id)?;
        }
        Ok(AddToPlaylistBulkResult {
            tracks_added,
            tracks_skipped,
        })
    }

    pub fn list_album_groups(
        &self,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AlbumGroupDto>, String> {
        let artist_expr = Self::album_artist_expr();
        let album_expr = Self::album_name_expr();
        let mut sql = format!(
            "SELECT {artist_expr} AS artist, {album_expr} AS album, COUNT(cf.id) AS track_count
             FROM collections c
             JOIN collection_files cf ON cf.collection_id = c.id AND cf.unavailable = 0
             JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
             WHERE c.kind = 'music'"
        );
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
            let q = format!("%{}%", s.trim().to_lowercase());
            sql.push_str(&format!(
                " AND (LOWER({artist_expr}) LIKE ? OR LOWER({album_expr}) LIKE ?)"
            ));
            params_vec.push(Box::new(q.clone()));
            params_vec.push(Box::new(q));
        }
        sql.push_str(&format!(
            " GROUP BY LOWER({artist_expr}), LOWER({album_expr})
              ORDER BY LOWER({artist_expr}), LOWER({album_expr})
              LIMIT ? OFFSET ?"
        ));
        params_vec.push(Box::new(limit));
        params_vec.push(Box::new(offset));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok(AlbumGroupDto {
                    artist: r.get(0)?,
                    album: r.get(1)?,
                    track_count: r.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    fn album_file_ids(&self, artist: &str, album: &str) -> Result<Vec<i64>, String> {
        let artist_expr = Self::album_artist_expr();
        let album_expr = Self::album_name_expr();
        let sql = format!(
            "SELECT cf.id FROM collection_files cf
             JOIN collections c ON c.id = cf.collection_id
             JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
             WHERE c.kind = 'music' AND cf.unavailable = 0
               AND LOWER({artist_expr}) = LOWER(?1)
               AND LOWER({album_expr}) = LOWER(?2)
             ORDER BY LOWER({artist_expr}), LOWER({album_expr}), c.sort_title, cf.disc_index, cf.track_order, cf.path"
        );
        let mut stmt = self.conn().prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![artist, album], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn add_album_to_playlist(
        &mut self,
        playlist_id: i64,
        artist: &str,
        album: &str,
    ) -> Result<AddToPlaylistBulkResult, String> {
        let file_ids = self.album_file_ids(artist, album)?;
        if file_ids.is_empty() {
            return Err("No tracks found for this album".into());
        }
        self.add_file_ids_to_playlist(playlist_id, file_ids)
    }

    pub fn add_collection_to_playlist(
        &mut self,
        playlist_id: i64,
        collection_id: i64,
    ) -> Result<AddToPlaylistBulkResult, String> {
        let file_ids: Vec<i64> = {
            let mut stmt = self
                .conn()
                .prepare(
                    "SELECT cf.id FROM collection_files cf
                     JOIN collections c ON c.id = cf.collection_id
                     JOIN library_roots r ON r.id = c.root_id AND r.is_available = 1
                     WHERE cf.collection_id = ?1 AND cf.unavailable = 0
                     ORDER BY cf.disc_index, cf.track_order, cf.path",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([collection_id], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        if file_ids.is_empty() {
            return Err("No tracks found in this album".into());
        }
        self.add_file_ids_to_playlist(playlist_id, file_ids)
    }

    pub fn create_playlist_from_album(&mut self, artist: &str, album: &str) -> Result<i64, String> {
        let file_ids = self.album_file_ids(artist, album)?;
        if file_ids.is_empty() {
            return Err("No tracks found for this album".into());
        }
        let name = self.ensure_unique_playlist_name(&Self::playlist_name_for_album(
            artist.trim(),
            album.trim(),
        ))?;
        let playlist_id = self.create_playlist(&name, false)?;
        self.add_file_ids_to_playlist(playlist_id, file_ids)?;
        Ok(playlist_id)
    }

    pub fn create_playlist_from_collection(&mut self, collection_id: i64) -> Result<i64, String> {
        let row: (String, Option<String>, Option<String>) = self
            .conn()
            .query_row(
                "SELECT title, artist, album FROM collections WHERE id = ?1 AND kind = 'music'",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "Album not found in library".to_string())?;
        let (title, artist, album) = row;
        let artist_s = artist
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Unknown Artist".into());
        let album_s = album
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(title);
        let name = self.ensure_unique_playlist_name(&Self::playlist_name_for_album(
            &artist_s, &album_s,
        ))?;
        let playlist_id = self.create_playlist(&name, false)?;
        self.add_collection_to_playlist(playlist_id, collection_id)?;
        Ok(playlist_id)
    }

    pub fn rename_playlist(&mut self, playlist_id: i64, name: &str) -> Result<(), String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("Playlist name cannot be empty".into());
        }
        let now = now_unix();
        let n = self
            .conn_mut()
            .execute(
                "UPDATE user_playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![trimmed, now, playlist_id],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Playlist not found".into());
        }
        Ok(())
    }

    pub fn delete_playlist(&mut self, playlist_id: i64) -> Result<(), String> {
        self.conn_mut()
            .execute(
                "DELETE FROM user_playlist_items WHERE playlist_id = ?1",
                [playlist_id],
            )
            .map_err(|e| e.to_string())?;
        let n = self
            .conn_mut()
            .execute("DELETE FROM user_playlists WHERE id = ?1", [playlist_id])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Playlist not found".into());
        }
        Ok(())
    }

    pub fn playlist_playback_paths(&mut self, playlist_id: i64) -> Result<Vec<PathBuf>, String> {
        let roots = self.registered_root_paths()?;
        let raw: Vec<PathBuf> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT cf.path FROM user_playlist_items i
                     JOIN collection_files cf ON cf.id = i.collection_file_id
                     WHERE i.playlist_id = ?1 AND cf.unavailable = 0
                     ORDER BY i.track_order",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([playlist_id], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|x| x.ok())
                .map(PathBuf::from)
                .collect()
        };

        let mut playable = Vec::new();
        for path in raw {
            let Some(on_disk) = tracked_file_on_disk(&path) else {
                let _ = self.mark_file_unavailable_by_path(&path);
                continue;
            };
            if !is_under_any_root(&on_disk, &roots) {
                continue;
            }
            if on_disk.to_string_lossy() != path.to_string_lossy() {
                let now = now_unix();
                let path_s = on_disk.to_string_lossy().to_string();
                let old_s = path.to_string_lossy().to_string();
                let _ = self.conn_mut().execute(
                    "UPDATE collection_files SET path = ?1, unavailable = 0, updated_at = ?2 WHERE path = ?3",
                    params![path_s, now, old_s],
                );
                let _ = self.db.update_media_path(&old_s, &path_s);
            }
            playable.push(on_disk);
        }
        if playable.is_empty() {
            return Err("No playable tracks in this playlist".into());
        }
        Ok(playable)
    }

    pub fn registered_root_paths(&self) -> Result<Vec<PathBuf>, String> {
        let mut stmt = self
            .conn()
            .prepare("SELECT path FROM library_roots")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        Ok(rows
            .filter_map(|x| x.ok())
            .filter_map(|p| canonicalize_existing(Path::new(&p)).ok())
            .collect())
    }

    /// Delete is allowed only when path is under a registered root that is currently available.
    pub fn path_delete_allowed(&self, path: &Path) -> Result<(), String> {
        let canon = canonicalize_existing(path).map_err(|e| e.to_string())?;
        let roots = self.list_roots()?;
        for r in roots {
            let Ok(root_canon) = canonicalize_existing(Path::new(&r.path)) else {
                if !r.is_available {
                    continue;
                }
                return Err("Delete blocked: library folder is not accessible".into());
            };
            if canon.starts_with(&root_canon) {
                if !r.is_available {
                    return Err(
                        "Delete blocked: library folder is not connected. Plug in the drive first."
                            .into(),
                    );
                }
                return Ok(());
            }
        }
        Err("Delete blocked: file is not under a registered library root".into())
    }

    pub fn get_playlist_default_speed(&self, playlist_id: i64) -> Result<Option<f64>, String> {
        let speed: Option<Option<f64>> = self
            .conn()
            .query_row(
                "SELECT default_playback_speed FROM user_playlists WHERE id = ?1",
                [playlist_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(speed) = speed else {
            return Err("Playlist not found".into());
        };
        Ok(speed.and_then(|s| if s.is_finite() && s >= 0.5 { Some(s) } else { None }))
    }

    pub fn set_playlist_default_speed(
        &mut self,
        playlist_id: i64,
        speed: Option<f64>,
    ) -> Result<(), String> {
        let now = now_unix();
        let n = if let Some(s) = speed {
            if !s.is_finite() || s < 0.5 || s > 4.0 {
                return Err("Speed must be between 0.5× and 4×".into());
            }
            self.conn_mut().execute(
                "UPDATE user_playlists SET default_playback_speed = ?1, updated_at = ?2 WHERE id = ?3",
                params![s, now, playlist_id],
            )
        } else {
            self.conn_mut().execute(
                "UPDATE user_playlists SET default_playback_speed = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, playlist_id],
            )
        }
        .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Playlist not found".into());
        }
        Ok(())
    }

    pub fn get_playlist_detail(&self, playlist_id: i64) -> Result<PlaylistDetailDto, String> {
        let conn = self.conn();
        let (id, name, kind, pinned, default_speed): (i64, String, String, i32, Option<f64>) =
            conn.query_row(
                "SELECT id, name, kind, is_pinned, default_playback_speed FROM user_playlists WHERE id = ?1",
                [playlist_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get::<_, Option<f64>>(4)?,
                    ))
                },
            )
            .map_err(|_| "Playlist not found".to_string())?;
        let default_playback_speed = default_speed.and_then(|s| {
            if s.is_finite() && s > 0.0 {
                Some(s)
            } else {
                None
            }
        });
        let mut stmt = conn
            .prepare(
                "SELECT i.id, i.collection_file_id, i.track_order, cf.display_title, c.title, cf.unavailable
                 FROM user_playlist_items i
                 JOIN collection_files cf ON cf.id = i.collection_file_id
                 JOIN collections c ON c.id = cf.collection_id
                 WHERE i.playlist_id = ?1
                 ORDER BY i.track_order",
            )
            .map_err(|e| e.to_string())?;
        let items: Vec<PlaylistItemDto> = stmt
            .query_map([playlist_id], |r| {
                Ok(PlaylistItemDto {
                    id: r.get(0)?,
                    collection_file_id: r.get(1)?,
                    track_order: r.get(2)?,
                    display_title: r.get(3)?,
                    collection_title: r.get(4)?,
                    unavailable: r.get::<_, i32>(5)? != 0,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();
        Ok(PlaylistDetailDto {
            id,
            name,
            kind,
            is_pinned: pinned != 0,
            default_playback_speed,
            items,
        })
    }

    pub fn remove_from_playlist(&mut self, item_id: i64) -> Result<(), String> {
        let playlist_id: Option<i64> = self
            .conn()
            .query_row(
                "SELECT playlist_id FROM user_playlist_items WHERE id = ?1",
                [item_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(pl_id) = playlist_id else {
            return Err("Playlist item not found".into());
        };
        self.conn_mut()
            .execute(
                "DELETE FROM user_playlist_items WHERE id = ?1",
                [item_id],
            )
            .map_err(|e| e.to_string())?;
        self.reindex_playlist_items(pl_id)?;
        Ok(())
    }

    pub fn reorder_playlist_items(&mut self, playlist_id: i64, item_ids: Vec<i64>) -> Result<(), String> {
        if item_ids.is_empty() {
            return Ok(());
        }
        let conn = self.conn_mut();
        for (order, item_id) in item_ids.iter().enumerate() {
            let n = conn
                .execute(
                    "UPDATE user_playlist_items SET track_order = ?1 WHERE id = ?2 AND playlist_id = ?3",
                    params![order as i32, item_id, playlist_id],
                )
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Playlist item not found".into());
            }
        }
        Ok(())
    }

    fn reindex_playlist_items(&mut self, playlist_id: i64) -> Result<(), String> {
        let ids: Vec<i64> = self
            .conn()
            .prepare(
                "SELECT id FROM user_playlist_items WHERE playlist_id = ?1 ORDER BY track_order",
            )
            .map_err(|e| e.to_string())?
            .query_map([playlist_id], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|x| x.ok())
            .collect();
        for (order, id) in ids.iter().enumerate() {
            self.conn_mut()
                .execute(
                    "UPDATE user_playlist_items SET track_order = ?1 WHERE id = ?2",
                    params![order as i32, id],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn reorder_collection_files(
        &mut self,
        collection_id: i64,
        file_ids: Vec<i64>,
    ) -> Result<(), String> {
        if file_ids.is_empty() {
            return Err("No files to reorder".into());
        }
        let expected: i64 = self
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM collection_files WHERE collection_id = ?1 AND unavailable = 0",
                [collection_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if file_ids.len() as i64 != expected {
            return Err("File list does not match collection".into());
        }
        let conn = self.conn_mut();
        for (order, file_id) in file_ids.iter().enumerate() {
            let n = conn
                .execute(
                    "UPDATE collection_files SET track_order = ?1, updated_at = ?2
                     WHERE id = ?3 AND collection_id = ?4",
                    params![order as i32, now_unix(), file_id, collection_id],
                )
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Collection file not found".into());
            }
        }
        self.mark_collection_manual(collection_id)?;
        Ok(())
    }

    pub fn update_file_display_title(
        &mut self,
        file_id: i64,
        display_title: &str,
    ) -> Result<(), String> {
        let trimmed = display_title.trim();
        if trimmed.is_empty() {
            return Err("Title cannot be empty".into());
        }
        let collection_id: i64 = self
            .conn()
            .query_row(
                "SELECT collection_id FROM collection_files WHERE id = ?1",
                [file_id],
                |r| r.get(0),
            )
            .map_err(|_| "File not found".to_string())?;
        let now = now_unix();
        let n = self
            .conn_mut()
            .execute(
                "UPDATE collection_files SET display_title = ?1, label = ?1, title_manual = 1, updated_at = ?2
                 WHERE id = ?3",
                params![trimmed, now, file_id],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("File not found".into());
        }
        self.mark_collection_manual(collection_id)?;
        Ok(())
    }

    /// Manual path repair when auto-relink fails. New path must stay under the collection's root.
    pub fn relink_collection_file(
        &mut self,
        file_id: i64,
        new_path: &str,
    ) -> Result<RelinkCollectionFileResult, String> {
        let trimmed = new_path.trim();
        if trimmed.is_empty() {
            return Err("Empty path".into());
        }
        let (old_path, collection_id, root_id): (String, i64, i64) = self
            .conn()
            .query_row(
                "SELECT cf.path, cf.collection_id, c.root_id FROM collection_files cf
                 JOIN collections c ON c.id = cf.collection_id WHERE cf.id = ?1",
                [file_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "Track not found".to_string())?;

        let root_path: String = self
            .conn()
            .query_row(
                "SELECT path FROM library_roots WHERE id = ?1",
                [root_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;

        let root_canon = canonicalize_existing(Path::new(&root_path)).map_err(|e| e.to_string())?;
        let new_canon =
            canonicalize_under_root(Path::new(trimmed), &root_canon).map_err(|e| e.to_string())?;
        if !is_audio_file(&new_canon) {
            return Err("Selected file is not a supported audio format".into());
        }

        let conflict: Option<i64> = self
            .conn()
            .query_row(
                "SELECT id FROM collection_files WHERE path = ?1 AND id != ?2",
                params![new_canon.to_string_lossy().as_ref(), file_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if conflict.is_some() {
            return Err("This file is already linked in your library at that path".into());
        }

        let track_order: i32 = self
            .conn()
            .query_row(
                "SELECT track_order FROM collection_files WHERE id = ?1",
                [file_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;

        let fname = new_canon
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Track".into());
        let (tag_title, _, _, track, disc) = probe_tags(&new_canon);
        let display = tag_title.unwrap_or(fname.clone());
        let sf = ScannedFile {
            path: new_canon.clone(),
            display_title: display.clone(),
            label: display,
            disc_index: disc.map(|d| d as i32).unwrap_or(0),
            track_index: track.map(|t| t as i32).unwrap_or(0),
        };

        let now = now_unix();
        let exists = new_canon.exists();
        let meta = fs::metadata(&new_canon).ok();
        let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let inode = file_inode(&new_canon);
        let hash = partial_file_hash(&new_canon);
        let path_s = new_canon.to_string_lossy().to_string();

        self.conn_mut()
            .execute(
                "UPDATE collection_files SET path = ?1, display_title = ?2, label = ?3,
                 track_order = ?4, disc_index = ?5, track_index = ?6, file_size = ?7, file_mtime = ?8,
                 inode = ?9, partial_hash = ?10, unavailable = ?11, updated_at = ?12 WHERE id = ?13",
                params![
                    path_s,
                    sf.display_title,
                    sf.label,
                    track_order,
                    sf.disc_index,
                    sf.track_index,
                    size,
                    mtime,
                    inode,
                    hash,
                    (!exists) as i32,
                    now,
                    file_id,
                ],
            )
            .map_err(|e| e.to_string())?;

        if old_path != path_s {
            let _ = self.db.update_media_path(&old_path, &path_s);
        }
        self.db
            .link_file_identity(&path_s, file_id)
            .map_err(|e| e.to_string())?;
        Ok(RelinkCollectionFileResult {
            old_path,
            new_path: path_s,
            collection_id,
        })
    }

    /// Re-run disc/track detection without renaming files on disk.
    pub fn fix_collection_track_order(&mut self, collection_id: i64) -> Result<(), String> {
        let (kind_s, root_id): (String, i64) = self
            .conn()
            .query_row(
                "SELECT kind, root_id FROM collections WHERE id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "Collection not found".to_string())?;

        let root_path: String = self
            .conn()
            .query_row(
                "SELECT path FROM library_roots WHERE id = ?1",
                [root_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let root_canon = canonicalize_existing(Path::new(&root_path)).map_err(|e| e.to_string())?;

        let paths: Vec<PathBuf> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT path FROM collection_files WHERE collection_id = ?1 ORDER BY track_order",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([collection_id], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|x| x.ok())
                .map(PathBuf::from)
                .collect()
        };
        if paths.is_empty() {
            return Err("No tracks to reorder".into());
        }

        let source_dir = common_parent_dir(&paths).ok_or("Cannot determine collection folder")?;
        let source_dir =
            canonicalize_under_root(&source_dir, &root_canon).map_err(|e| e.to_string())?;

        let kind = ContentKind::parse(&kind_s).unwrap_or(ContentKind::Audiobook);
        let (layout, files) = match kind {
            ContentKind::Audiobook | ContentKind::Mixed => gather_audiobook_files(&source_dir),
            ContentKind::Music => {
                let mut audio = if source_dir.is_file() {
                    vec![source_dir.clone()]
                } else {
                    self.collect_audio_files(&root_canon, &source_dir)?
                };
                if audio.is_empty() {
                    return Err("No audio files found in collection folder".into());
                }
                natord_sort_paths(&mut audio);
                let scanned: Vec<ScannedFile> = audio
                    .into_iter()
                    .enumerate()
                    .map(|(i, path)| {
                        let (tag_title, _, _, track, disc) = probe_tags(&path);
                        let display = tag_title.unwrap_or_else(|| format!("Track {}", i + 1));
                        ScannedFile {
                            path,
                            display_title: display.clone(),
                            label: display,
                            disc_index: disc.map(|d| d as i32).unwrap_or(0),
                            track_index: track.map(|t| t as i32).unwrap_or(i as i32 + 1),
                        }
                    })
                    .collect();
                let layout = if scanned.len() == 1 {
                    LayoutKind::SingleFile
                } else {
                    LayoutKind::FlatMulti
                };
                (layout, scanned)
            }
        };

        if files.is_empty() {
            return Err("No audio files found after re-scan".into());
        }

        let now = now_unix();
        self.conn_mut()
            .execute(
                "UPDATE collections SET layout_kind = ?1, updated_at = ?2 WHERE id = ?3",
                params![layout.as_str(), now, collection_id],
            )
            .map_err(|e| e.to_string())?;

        for (order, sf) in files.iter().enumerate() {
            self.upsert_collection_file(collection_id, order as i32, sf, now)?;
        }
        let _ = self.try_extract_cover(collection_id, &files);
        Ok(())
    }

    pub fn set_playlist_pinned(&mut self, playlist_id: i64, pinned: bool) -> Result<(), String> {
        let now = now_unix();
        if pinned {
            self.conn_mut()
                .execute(
                    "UPDATE user_playlists SET is_pinned = 0, updated_at = ?1 WHERE id != ?2",
                    params![now, playlist_id],
                )
                .map_err(|e| e.to_string())?;
        }
        let n = self
            .conn_mut()
            .execute(
                "UPDATE user_playlists SET is_pinned = ?1, updated_at = ?2 WHERE id = ?3",
                params![pinned as i32, now, playlist_id],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("Playlist not found".into());
        }
        Ok(())
    }

    pub fn scan_status_all(&self) -> Result<Vec<ScanStatusDto>, String> {
        let roots = self.list_roots()?;
        Ok(roots
            .into_iter()
            .map(|r| ScanStatusDto {
                root_id: r.id,
                scanning: false,
                last_scan_at: r.last_scan_at,
                last_scan_status: r.last_scan_status,
                collections_found: r.collection_count,
            })
            .collect())
    }

    pub fn collection_mpris_meta(
        &self,
        collection_id: i64,
        track_path: &str,
    ) -> Result<(String, Option<String>, Option<String>, Option<String>), String> {
        let conn = self.conn();
        let (title, author, artist, album, cover): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT title, author, artist, album, cover_path FROM collections WHERE id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(|_| "Collection not found".to_string())?;

        let track_label: Option<String> = conn
            .query_row(
                "SELECT display_title FROM collection_files WHERE collection_id = ?1 AND path = ?2",
                params![collection_id, track_path],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let display_title = track_label
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(title);
        let performer = author.or(artist);
        Ok((display_title, performer, album, cover))
    }

    pub fn add_to_library(&mut self, input: AddToLibraryInput) -> Result<i64, String> {
        let path = canonicalize_existing(Path::new(input.path.trim())).map_err(|e| e.to_string())?;
        let kind = ContentKind::parse(&input.content_kind).ok_or("Invalid content kind")?;

        let roots = self.list_roots()?;
        let root_canons: Vec<(i64, PathBuf)> = roots
            .iter()
            .filter_map(|r| {
                canonicalize_existing(Path::new(&r.path))
                    .ok()
                    .map(|c| (r.id, c))
            })
            .collect();

        let root_id = root_canons
            .iter()
            .find(|(_, rc)| path.starts_with(rc))
            .map(|(id, _)| *id);

        let root_id = match root_id {
            Some(id) => id,
            None => {
                let parent = if path.is_dir() {
                    path.clone()
                } else {
                    path.parent()
                        .ok_or("Invalid path")?
                        .to_path_buf()
                };
                let dto = self.add_root(AddLibraryRootInput {
                    path: parent.to_string_lossy().to_string(),
                    label: None,
                    content_kind: kind.as_str().to_string(),
                    scan_rule: Some(ScanRule::default_for(kind).as_str().to_string()),
                    scan_subfolders: Some(kind == ContentKind::Audiobook),
                })?;
                dto.id
            }
        };

        let root_path: String = self
            .conn()
            .query_row(
                "SELECT path FROM library_roots WHERE id = ?1",
                [root_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let root_canon = canonicalize_existing(Path::new(&root_path)).map_err(|e| e.to_string())?;

        let grouping = input.grouping.trim();
        let title = input
            .metadata
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                if path.is_dir() {
                    path.file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Collection".into())
                } else {
                    path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Track".into())
                }
            });

        let (layout, files) = match grouping {
            "file-is-item" if path.is_dir() => {
                let mut audio = self.collect_audio_files(&root_canon, &path)?;
                if audio.is_empty() {
                    return Err("No audio files found in folder".into());
                }
                natord_sort_paths(&mut audio);
                let scanned: Vec<ScannedFile> = audio
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let (tag_title, _, _, _, _) = probe_tags(&p);
                        let display = tag_title.unwrap_or_else(|| format!("Track {}", i + 1));
                        ScannedFile {
                            path: p,
                            display_title: display.clone(),
                            label: display,
                            disc_index: 0,
                            track_index: i as i32 + 1,
                        }
                    })
                    .collect();
                let layout = if scanned.len() == 1 {
                    LayoutKind::SingleFile
                } else {
                    LayoutKind::FlatMulti
                };
                (layout, scanned)
            }
            _ if path.is_file() => {
                let (tag_title, _, _, _, _) = probe_tags(&path);
                let display = tag_title.unwrap_or_else(|| title.clone());
                (
                    LayoutKind::SingleFile,
                    vec![ScannedFile {
                        path: path.clone(),
                        display_title: display.clone(),
                        label: display,
                        disc_index: 0,
                        track_index: 1,
                    }],
                )
            }
            _ if path.is_dir() && kind == ContentKind::Audiobook => gather_audiobook_files(&path),
            _ if path.is_dir() => {
                let mut audio = self.collect_audio_files(&root_canon, &path)?;
                if audio.is_empty() {
                    return Err("No audio files found in folder".into());
                }
                natord_sort_paths(&mut audio);
                let scanned: Vec<ScannedFile> = audio
                    .into_iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let (tag_title, _, _, track, disc) = probe_tags(&p);
                        let display = tag_title.unwrap_or_else(|| format!("Track {}", i + 1));
                        ScannedFile {
                            path: p,
                            display_title: display.clone(),
                            label: display,
                            disc_index: disc.map(|d| d as i32).unwrap_or(0),
                            track_index: track.map(|t| t as i32).unwrap_or(i as i32 + 1),
                        }
                    })
                    .collect();
                let layout = if scanned.len() == 1 {
                    LayoutKind::SingleFile
                } else {
                    LayoutKind::FlatMulti
                };
                (layout, scanned)
            }
            _ => return Err("Path must be a folder or audio file".into()),
        };

        if files.is_empty() {
            return Err("No audio files found".into());
        }

        self.upsert_collection(root_id, kind, &title, layout, &files, Some(input.metadata), None)?;
        let collection_id: i64 = self
            .conn()
            .query_row(
                "SELECT id FROM collections WHERE root_id = ?1 AND title = ?2 AND kind = ?3",
                params![root_id, title, kind.as_str()],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        self.conn_mut()
            .execute(
                "UPDATE collections SET is_manual = 1 WHERE id = ?1",
                [collection_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(collection_id)
    }

    pub fn plan_metadata_lookup(
        &self,
        collection_id: i64,
        enabled: bool,
    ) -> Result<MetadataLookupPlan, String> {
        if !enabled {
            return Ok(MetadataLookupPlan::Disabled);
        }
        let (kind, title, author, artist, album): (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = self
            .conn()
            .query_row(
                "SELECT kind, title, author, artist, album FROM collections WHERE id = ?1",
                [collection_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(|_| "Collection not found".to_string())?;

        let title_q = title.trim().to_string();
        if title_q.is_empty() {
            return Ok(MetadataLookupPlan::EmptyTitle);
        }

        let secondary = if kind == "audiobook" {
            author.clone()
        } else {
            artist.clone().or_else(|| album.clone())
        }
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
        let cache_key = format!(
            "{}\u{001f}{}\u{001f}{}",
            kind.to_lowercase(),
            title_q.to_lowercase(),
            secondary
        );

        let cached: Option<(String, i64)> = self
            .conn()
            .query_row(
                "SELECT payload_json, fetched_at FROM metadata_cache \
                 WHERE source = 'online' AND lookup_key = ?1",
                [&cache_key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((payload, fetched_at)) = cached {
            if now_unix().saturating_sub(fetched_at) < METADATA_CACHE_TTL_SECS {
                if let Ok(suggestions) =
                    serde_json::from_str::<Vec<MetadataSuggestionDto>>(&payload)
                {
                    if !suggestions.is_empty() {
                        return Ok(MetadataLookupPlan::Cached(suggestions));
                    }
                }
            }
        }

        Ok(MetadataLookupPlan::Fetch(MetadataLookupRequest {
            kind,
            title: title_q,
            author,
            artist,
            album,
            cache_key,
        }))
    }

    pub fn store_metadata_lookup_cache(
        &mut self,
        cache_key: &str,
        suggestions: &[MetadataSuggestionDto],
    ) -> Result<(), String> {
        if suggestions.is_empty() {
            return Ok(());
        }
        let now = now_unix();
        let json = serde_json::to_string(suggestions).map_err(|e| e.to_string())?;
        self.conn_mut()
            .execute(
                "INSERT INTO metadata_cache (source, lookup_key, payload_json, fetched_at)
                 VALUES ('online', ?1, ?2, ?3)
                 ON CONFLICT(source, lookup_key) DO UPDATE SET payload_json = ?2, fetched_at = ?3",
                params![cache_key, json, now],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn try_extract_cover(&mut self, collection_id: i64, files: &[ScannedFile]) -> Result<(), String> {
        let covers_dir = covers_data_dir()?;
        for sf in files {
            if !sf.path.exists() {
                continue;
            }
            if let Some(dest) = extract_cover_from_file(&sf.path, &covers_dir, collection_id) {
                let now = now_unix();
                self.conn_mut()
                    .execute(
                        "UPDATE collections SET cover_path = ?1, updated_at = ?2 WHERE id = ?3",
                        params![dest.to_string_lossy().as_ref(), now, collection_id],
                    )
                    .map_err(|e| e.to_string())?;
                break;
            }
        }
        Ok(())
    }
}

/// Perform the outbound catalogue search. Must run off the app mutex (blocking HTTP).
pub fn fetch_metadata_online(req: &MetadataLookupRequest) -> Result<Vec<MetadataSuggestionDto>, String> {
    if req.kind == "audiobook" {
        lookup_openlibrary(&req.title, req.author.as_deref())
    } else {
        lookup_musicbrainz(
            &req.title,
            req.artist.as_deref().or(req.album.as_deref()),
        )
    }
}

fn covers_data_dir() -> Result<PathBuf, String> {
    let pd = directories::ProjectDirs::from("com", "chaptercheck", "ChapterCheck")
        .ok_or_else(|| "Cannot resolve application data directory".to_string())?;
    let dir = pd.data_dir().join("covers");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Contact-style User-Agent. MusicBrainz requires a meaningful UA with a way to
/// reach the maintainer; Open Library asks for the same courtesy.
const HTTP_USER_AGENT: &str =
    "ChapterCheck/0.1 ( https://github.com/aSoftwareByDesignRepository/ChapterCheck )";

/// How long a non-empty online result stays valid before we look it up again.
const METADATA_CACHE_TTL_SECS: i64 = 60 * 60 * 24 * 30; // 30 days

/// Strict allow-list of the exact endpoints we are permitted to contact. This is
/// the SSRF backstop: every outbound request is checked against it, so a future
/// bug that lets user input influence the URL can never reach an arbitrary host.
fn http_allowed(url: &str) -> bool {
    url.starts_with("https://openlibrary.org/search.json?")
        || url.starts_with("https://musicbrainz.org/ws/2/")
}

/// Perform a hardened blocking GET and decode JSON.
///
/// * Enforces the allow-list (defence in depth alongside the callers).
/// * A short timeout so the UI never hangs.
/// * Distinguishes a real service failure (`Err`) from a successful empty
///   response (the caller decides what an empty payload means). A non-2xx
///   status is treated as an error so transient outages / rate limits surface
///   to the user as "try again" instead of a misleading "no results".
fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    http_get_json_with_retry(url, false)
}

fn http_get_json_with_retry(url: &str, is_retry: bool) -> Result<serde_json::Value, String> {
    if !http_allowed(url) {
        return Err("Blocked: target is not on the allow-list".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .connect_timeout(std::time::Duration::from_secs(6))
        .user_agent(HTTP_USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .map_err(|e| format!("Network error: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 503 && !is_retry && url.contains("musicbrainz.org") {
        std::thread::sleep(std::time::Duration::from_millis(1100));
        return http_get_json_with_retry(url, true);
    }
    if !status.is_success() {
        return Err(format!("Service unavailable (HTTP {})", status.as_u16()));
    }
    resp.json::<serde_json::Value>()
        .map_err(|e| format!("Could not read the response: {e}"))
}

/// Audiobook lookups use the Open Library search API (Internet Archive): an
/// open, non-profit, key-free catalogue that returns title + author. A
/// title-only search that matches nothing returns HTTP 200 with an empty
/// `docs` array, which we surface as "no suggestions".
fn lookup_openlibrary(
    title: &str,
    author: Option<&str>,
) -> Result<Vec<MetadataSuggestionDto>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(Vec::new());
    }
    let mut url = format!(
        "https://openlibrary.org/search.json?title={}&limit=5&fields=title,author_name",
        urlencoding_encode(title)
    );
    if let Some(a) = author.map(str::trim).filter(|s| !s.is_empty()) {
        url.push_str(&format!("&author={}", urlencoding_encode(a)));
    }
    let body = http_get_json(&url)?;
    let docs = body
        .get("docs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for doc in docs.iter() {
        let t = doc
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let a = doc
            .get("author_name")
            .and_then(|v| v.as_array())
            .and_then(|authors| authors.first())
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        if t.is_none() && a.is_none() {
            continue;
        }
        let dedup = format!(
            "{}\u{001f}{}",
            t.unwrap_or("").to_lowercase(),
            a.as_deref().unwrap_or("").to_lowercase()
        );
        if !seen.insert(dedup) {
            continue;
        }
        out.push(MetadataSuggestionDto {
            title: t.map(|s| s.to_string()),
            author: a,
            narrator: None,
            artist: None,
            album: None,
            source: "Open Library".into(),
        });
        if out.len() >= 5 {
            break;
        }
    }
    Ok(out)
}

fn lookup_musicbrainz(
    title: &str,
    artist: Option<&str>,
) -> Result<Vec<MetadataSuggestionDto>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Ok(Vec::new());
    }
    // Build a Lucene query, escaping user input so special characters can never
    // change the query's meaning, then percent-encode the whole value so the URL
    // is always well-formed.
    let mut query = format!("release:{}", lucene_escape(title));
    if let Some(a) = artist.map(str::trim).filter(|s| !s.is_empty()) {
        query.push_str(&format!(" AND artist:{}", lucene_escape(a)));
    }
    let url = format!(
        "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit=5",
        urlencoding_encode(&query)
    );
    let body = http_get_json(&url)?;
    let releases = body
        .get("releases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for rel in releases.iter() {
        let album = rel
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let artist_name = rel
            .get("artist-credit")
            .and_then(|v| v.as_array())
            .and_then(|credits| credits.first())
            .and_then(|c| c.get("name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        if album.is_none() && artist_name.is_none() {
            continue;
        }
        let dedup = format!(
            "{}\u{001f}{}",
            album.as_deref().unwrap_or("").to_lowercase(),
            artist_name.as_deref().unwrap_or("").to_lowercase()
        );
        if !seen.insert(dedup) {
            continue;
        }
        out.push(MetadataSuggestionDto {
            title: album.clone(),
            author: None,
            narrator: None,
            artist: artist_name,
            album,
            source: "MusicBrainz".into(),
        });
        if out.len() >= 5 {
            break;
        }
    }
    Ok(out)
}

/// Escape the characters that Lucene (MusicBrainz's query parser) treats as
/// operators, so a title like `Sign o' the Times (Deluxe)` is searched
/// literally instead of being parsed as syntax.
fn lucene_escape(s: &str) -> String {
    const SPECIAL: &[char] = &[
        '+', '-', '&', '|', '!', '(', ')', '{', '}', '[', ']', '^', '"', '~', '*', '?', ':', '\\',
        '/',
    ];
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if SPECIAL.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn extract_cover_from_file(
    audio_path: &Path,
    covers_dir: &Path,
    collection_id: i64,
) -> Option<PathBuf> {
    use lofty::file::TaggedFileExt;
    use lofty::picture::PictureType;
    let tagged = lofty::read_from_path(audio_path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag
        .pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;
    let ext = match picture.mime_type() {
        Some(lofty::picture::MimeType::Png) => "png",
        Some(lofty::picture::MimeType::Jpeg) => "jpg",
        _ => "jpg",
    };
    let dest = covers_dir.join(format!("{collection_id}.{ext}"));
    fs::write(&dest, picture.data()).ok()?;
    Some(dest)
}
