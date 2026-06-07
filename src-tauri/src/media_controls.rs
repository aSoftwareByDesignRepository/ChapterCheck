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

use crate::AppState;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Relative seek applied for a plain MPRIS `Seek` media key (audiobook-friendly,
/// matches the on-screen ±30 s buttons). Explicit `SeekBy`/`SetPosition` events
/// carry their own offset and are honoured directly.
const SEEK_STEP_SECS: f64 = 30.0;

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
        }
    }
}

/// Spawn the media-controls thread. Failure to register (e.g. no D-Bus session
/// bus, as in a headless build environment) is logged and otherwise ignored:
/// the application keeps working without hardware-key support.
pub fn spawn(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || run(app));
}

fn run(app: AppHandle) {
    let config = PlatformConfig {
        dbus_name: "chaptercheck",
        display_name: "ChapterCheck",
        hwnd: None,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ChapterCheck: OS media controls unavailable: {e:?}");
            return;
        }
    };

    // The sender lives inside the (static) event closure owned by `controls`,
    // which lives for the whole thread, so the receiver never disconnects.
    let (nudge_tx, nudge_rx) = mpsc::channel::<()>();
    let handler_app = app.clone();
    if let Err(e) = controls.attach(move |event| {
        handle_event(&handler_app, event);
        let _ = nudge_tx.send(());
    }) {
        eprintln!("ChapterCheck: failed to attach OS media controls: {e:?}");
        return;
    }

    let mut last: Option<OsMediaSnapshot> = None;
    loop {
        if let Some(snap) = snapshot(&app) {
            push(&mut controls, &mut last, snap);
        }
        // Wake on the next hardware event, otherwise refresh once a second so the
        // lock-screen position stays current.
        match nudge_rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        // Coalesce any bursts of events into a single sync.
        while nudge_rx.try_recv().is_ok() {}
    }
}

/// Route a hardware media event into the playback engine using the same
/// commands as the UI, then notify the frontend so it updates without waiting
/// for its next poll.
fn handle_event(app: &AppHandle, event: MediaControlEvent) {
    let result: Result<(), String> = match event {
        MediaControlEvent::Play => crate::media_play(app.clone()),
        MediaControlEvent::Pause => crate::set_paused(app.clone(), true),
        MediaControlEvent::Toggle => crate::media_toggle(app.clone()),
        MediaControlEvent::Next => crate::skip_next(app.clone()),
        MediaControlEvent::Previous => crate::skip_prev(app.clone()),
        // We have no hard "stop"; pausing is the safe, reversible equivalent and
        // preserves the resume position.
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
        // Volume, OpenUri and Quit are intentionally ignored: this app exposes no
        // volume control and must never be closed by a stray headphone signal.
        _ => Ok(()),
    };

    match result {
        Ok(()) => {
            let _ = app.emit("abp:transport-changed", ());
        }
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
    let mut guard = state.inner.lock().ok()?;
    Some(guard.os_media_snapshot())
}

fn push(controls: &mut MediaControls, last: &mut Option<OsMediaSnapshot>, snap: OsMediaSnapshot) {
    let metadata_changed = last.as_ref().map_or(true, |prev| {
        prev.track_key != snap.track_key
            || prev.title != snap.title
            || prev.artist != snap.artist
            || prev.album != snap.album
            || prev.duration_sec != snap.duration_sec
    });

    if metadata_changed {
        if snap.has_track {
            let _ = controls.set_metadata(MediaMetadata {
                title: Some(snap.title.as_str()),
                artist: snap.artist.as_deref(),
                album: snap.album.as_deref(),
                duration: snap.duration_sec.and_then(safe_duration),
                cover_url: None,
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
