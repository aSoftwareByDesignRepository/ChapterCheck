//! OS media-key / headphone integration.
//!
//! On Linux this registers an `org.mpris.MediaPlayer2` D-Bus service via
//! [`souvlaki`]. The desktop environment routes media keys and Bluetooth / wired
//! headset (AVRCP) buttons — Play, Pause, Play/Pause toggle, Next, Previous,
//! Stop, Seek — to whichever player owns that interface. Without it, those
//! hardware buttons do nothing for this app.
//!
//! Design notes:
//! - A single dedicated thread *owns* the [`MediaControls`] handle for the whole
//!   process lifetime. The handle is only ever touched from that thread, which
//!   sidesteps the `Send`/`Sync` constraints of the underlying D-Bus connection.
//! - Incoming hardware events are delivered by souvlaki on its own internal
//!   thread (the `attach` closure). They call the same Rust entry points as the
//!   on-screen buttons and keyboard shortcuts (`toggle_playback_inner`,
//!   `skip_next_inner`, `seek_delta_inner`, …), then nudge the sync loop so the
//!   OS reflects the new state immediately.
//! - The sync loop pushes metadata, playback status and position to the OS once
//!   per second (and on every nudge). It reads playback state *passively* and
//!   never spawns mpv, so it cannot perturb the foreground engine.
//! - [`nudge`] is called whenever transport changes from the UI so the desktop
//!   routes the next headset press to this player instead of a browser tab.

use crate::AppState;
use serde::Serialize;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Relative seek applied for a plain MPRIS `Seek` media key (audiobook-friendly,
/// matches the on-screen ±30 s buttons). Explicit `SeekBy`/`SetPosition` events
/// carry their own offset and are honoured directly.
const SEEK_STEP_SECS: f64 = 30.0;

static NUDGE_TX: OnceLock<Mutex<Option<mpsc::Sender<()>>>> = OnceLock::new();
static REGISTERED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
pub struct OsMediaStatusDto {
    pub available: bool,
    pub player_name: String,
}

/// Immutable snapshot of what the OS media controls should display, built under
/// the app-state lock and then handed to the (lock-free) push step.
#[derive(Clone, PartialEq)]
pub(crate) struct OsMediaSnapshot {
    pub has_track: bool,
    /// No file is loaded in the engine (idle): report `Stopped`.
    pub stopped: bool,
    /// Actively playing (not paused, not ended, not idle).
    pub playing: bool,
    pub position_sec: f64,
    pub duration_sec: Option<f64>,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Identity of the current track (its path); used to detect metadata changes.
    pub track_key: String,
    pub cover_url: Option<String>,
}

impl OsMediaSnapshot {
    pub(crate) fn stopped() -> Self {
        Self {
            has_track: false,
            stopped: true,
            playing: false,
            position_sec: 0.0,
            duration_sec: None,
            title: String::new(),
            artist: None,
            album: None,
            track_key: String::new(),
            cover_url: None,
        }
    }
}

pub fn player_dbus_name() -> &'static str {
    "org.mpris.MediaPlayer2.chaptercheck"
}

pub fn status() -> OsMediaStatusDto {
    OsMediaStatusDto {
        available: REGISTERED.load(Ordering::Acquire),
        player_name: player_dbus_name().to_string(),
    }
}

/// Wake the sync loop immediately (e.g. after UI transport changes).
pub fn nudge() {
    if let Ok(guard) = NUDGE_TX.get_or_init(|| Mutex::new(None)).lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(());
        }
    }
}

/// Spawn the media-controls thread. Failure to register (e.g. no D-Bus session
/// bus, as in a headless build environment) is logged and otherwise ignored:
/// the application keeps working without hardware-key support.
pub fn spawn(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            if run(app).is_ok() {
                REGISTERED.store(true, Ordering::Release);
            }
        });
}

fn run(app: AppHandle) -> Result<(), ()> {
    let config = PlatformConfig {
        dbus_name: "chaptercheck",
        display_name: "ChapterCheck",
        hwnd: None,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ChapterCheck: OS media controls unavailable: {e:?}");
            return Err(());
        }
    };

    let (nudge_tx, nudge_rx) = mpsc::channel::<()>();
    if let Ok(mut slot) = NUDGE_TX.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(nudge_tx.clone());
    }

    let handler_app = app.clone();
    if let Err(e) = controls.attach(move |event| {
        handle_event(&handler_app, event);
        let _ = nudge_tx.send(());
    }) {
        eprintln!("ChapterCheck: failed to attach OS media controls: {e:?}");
        return Err(());
    }

    let mut last: Option<OsMediaSnapshot> = None;
    if let Some(snap) = snapshot(&app) {
        push(&mut controls, &mut last, snap);
    }

    loop {
        if let Some(snap) = snapshot(&app) {
            push(&mut controls, &mut last, snap);
        }
        match nudge_rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(()),
        }
        while nudge_rx.try_recv().is_ok() {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaActionClass {
    Transport,
    Skip,
    Seek,
}

fn action_class(event: &MediaControlEvent) -> MediaActionClass {
    match event {
        MediaControlEvent::Seek(_) | MediaControlEvent::SeekBy(_, _) | MediaControlEvent::SetPosition(_) => {
            MediaActionClass::Seek
        }
        MediaControlEvent::Next | MediaControlEvent::Previous => MediaActionClass::Skip,
        _ => MediaActionClass::Transport,
    }
}

/// Drop duplicate transport events within a short window (common with BT headsets
/// and when both MPRIS and the webview receive the same key).
fn should_debounce_event(event: &MediaControlEvent) -> bool {
    static LAST: OnceLock<Mutex<(Option<MediaActionClass>, Instant)>> = OnceLock::new();
    let gate = LAST.get_or_init(|| Mutex::new((None, Instant::now())));
    let Ok(mut last) = gate.lock() else {
        return false;
    };
    let now = Instant::now();
    let class = action_class(event);
    if class != MediaActionClass::Seek {
        if let Some(prev) = last.0 {
            if prev == class && now.duration_since(last.1) < Duration::from_millis(280) {
                return true;
            }
        }
        *last = (Some(class), now);
    }
    false
}

/// Route a hardware media event into the playback engine using the same
/// commands as the UI, then notify the frontend so it updates without waiting
/// for its next poll.
fn handle_event(app: &AppHandle, event: MediaControlEvent) {
    if should_debounce_event(&event) {
        return;
    }
    let result: Result<(), String> = match event {
        MediaControlEvent::Play => crate::media_play(app.clone()),
        MediaControlEvent::Pause => crate::set_paused(app.clone(), true),
        MediaControlEvent::Toggle => crate::media_toggle(app.clone()),
        MediaControlEvent::Next => crate::skip_next(app.clone()),
        MediaControlEvent::Previous => crate::skip_prev(app.clone()),
        MediaControlEvent::Stop => crate::set_paused(app.clone(), true),
        MediaControlEvent::Seek(SeekDirection::Forward) => {
            crate::seek_delta(app.clone(), SEEK_STEP_SECS)
        }
        MediaControlEvent::Seek(SeekDirection::Backward) => {
            crate::seek_delta(app.clone(), -SEEK_STEP_SECS)
        }
        MediaControlEvent::SeekBy(direction, dur) => {
            let secs = dur.as_secs_f64();
            let signed = match direction {
                SeekDirection::Forward => secs,
                SeekDirection::Backward => -secs,
            };
            crate::seek_delta(app.clone(), signed)
        }
        MediaControlEvent::SetPosition(MediaPosition(pos)) => {
            crate::seek_seconds(app.clone(), pos.as_secs_f64())
        }
        MediaControlEvent::Raise => {
            raise_window(app);
            Ok(())
        }
        _ => Ok(()),
    };

    match result {
        Ok(()) => nudge(),
        Err(e) => {
            let _ = app.emit("abp:user-error", e);
        }
    }
}

fn raise_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn snapshot(app: &AppHandle) -> Option<OsMediaSnapshot> {
    let state = app.state::<AppState>();
    for _ in 0..12 {
        if let Ok(mut guard) = state.inner.try_lock() {
            return Some(guard.os_media_snapshot());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

fn push(controls: &mut MediaControls, last: &mut Option<OsMediaSnapshot>, snap: OsMediaSnapshot) {
    let metadata_changed = last.as_ref().map_or(true, |prev| {
        prev.track_key != snap.track_key
            || prev.title != snap.title
            || prev.artist != snap.artist
            || prev.album != snap.album
            || prev.duration_sec != snap.duration_sec
            || prev.cover_url != snap.cover_url
    });

    if metadata_changed {
        if snap.has_track {
            let cover_ref = snap.cover_url.as_deref();
            let _ = controls.set_metadata(MediaMetadata {
                title: Some(snap.title.as_str()),
                artist: snap.artist.as_deref(),
                album: snap.album.as_deref(),
                duration: snap.duration_sec.and_then(safe_duration),
                cover_url: cover_ref,
            });
        } else {
            let _ = controls.set_metadata(MediaMetadata::default());
        }
    }

    let progress = Some(MediaPosition(
        safe_duration(snap.position_sec).unwrap_or(Duration::ZERO),
    ));
    let playback = if !snap.has_track || snap.stopped {
        MediaPlayback::Stopped
    } else if snap.playing {
        MediaPlayback::Playing { progress }
    } else {
        MediaPlayback::Paused { progress }
    };
    let _ = controls.set_playback(playback);

    *last = Some(snap);
}

/// `Duration::from_secs_f64` panics on negative / non-finite input. Guard it.
fn safe_duration(secs: f64) -> Option<Duration> {
    if secs.is_finite() && secs >= 0.0 {
        Some(Duration::from_secs_f64(secs))
    } else {
        None
    }
}
