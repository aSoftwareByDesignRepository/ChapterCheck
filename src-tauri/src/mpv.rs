use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const MPV_SOCKET_NAME: &str = "chaptercheck-mpv.sock";
const MPV_SOCKET_LEGACY_PREFIX: &str = "chaptercheck-mpv-";

#[derive(Debug, thiserror::Error)]
pub enum MpvError {
    #[error("mpv failed to start: {0}")]
    Spawn(String),
    #[error("mpv IPC: {0}")]
    Ipc(String),
    #[error("timed out waiting for mpv IPC socket")]
    SocketTimeout,
    #[error("invalid JSON from mpv: {0}")]
    Json(String),
    #[error("mpv returned error: {0}")]
    Command(String),
}

struct MpvConnection {
    child: Child,
    sock_path: PathBuf,
    io: BufReader<UnixStream>,
    next_id: i64,
}

impl Drop for MpvConnection {
    fn drop(&mut self) {
        terminate_child(&mut self.child);
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

fn mpv_property_unavailable(cmd_err: &str) -> bool {
    let t = cmd_err.trim_matches('"').to_lowercase();
    t.contains("property unavailable")
        || t.contains("property not found")
        || t == "no data"
}

impl MpvConnection {
    fn write_cmd(&mut self, value: &Value) -> Result<(), MpvError> {
        let mut payload = serde_json::to_string(value).map_err(|e| MpvError::Json(e.to_string()))?;
        payload.push('\n');
        self.io
            .get_mut()
            .write_all(payload.as_bytes())
            .map_err(|e| MpvError::Ipc(e.to_string()))?;
        self.io
            .get_mut()
            .flush()
            .map_err(|e| MpvError::Ipc(e.to_string()))?;
        Ok(())
    }

    fn read_until_request(&mut self, req_id: i64) -> Result<Value, MpvError> {
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = self
                .io
                .read_line(&mut buf)
                .map_err(|e| MpvError::Ipc(e.to_string()))?;
            if n == 0 {
                return Err(MpvError::Ipc("socket closed".into()));
            }
            let line = buf.trim();
            if line.is_empty() {
                continue;
            }
            let msg: Value =
                serde_json::from_str(line).map_err(|e| MpvError::Json(format!("{e}: {line}")))?;
            if msg.get("event").is_some() {
                continue;
            }
            if msg.get("request_id").and_then(|v| v.as_i64()) == Some(req_id) {
                return Ok(msg);
            }
        }
    }

    fn request(&mut self, command: Value) -> Result<Value, MpvError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut envelope = serde_json::Map::new();
        if let Value::Object(map) = command {
            envelope.extend(map);
        } else {
            return Err(MpvError::Json("command must be object".into()));
        }
        envelope.insert("request_id".into(), json!(id));
        self.write_cmd(&Value::Object(envelope))?;
        let response = self.read_until_request(id)?;
        match response.get("error") {
            Some(Value::String(s)) if s == "success" => Ok(response),
            Some(other) => Err(MpvError::Command(other.to_string())),
            None => Err(MpvError::Command("missing error field".into())),
        }
    }

    fn get_prop_f64(&mut self, name: &str) -> Result<Option<f64>, MpvError> {
        match self.request(json!({ "command": ["get_property", name] })) {
            Ok(res) => Ok(res.get("data").and_then(|v| v.as_f64())),
            Err(MpvError::Command(msg)) if mpv_property_unavailable(&msg) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn get_prop_bool(&mut self, name: &str) -> Result<Option<bool>, MpvError> {
        match self.request(json!({ "command": ["get_property", name] })) {
            Ok(res) => Ok(res.get("data").and_then(|v| v.as_bool())),
            Err(MpvError::Command(msg)) if mpv_property_unavailable(&msg) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn get_prop_json(&mut self, name: &str) -> Result<Option<Value>, MpvError> {
        match self.request(json!({ "command": ["get_property", name] })) {
            Ok(res) => Ok(res.get("data").cloned()),
            Err(MpvError::Command(msg)) if mpv_property_unavailable(&msg) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn set_prop_f64(&mut self, name: &str, value: f64) -> Result<(), MpvError> {
        self.request(json!({ "command": ["set_property", name, value] }))?;
        Ok(())
    }

    fn set_prop_bool(&mut self, name: &str, value: bool) -> Result<(), MpvError> {
        self.request(json!({ "command": ["set_property", name, value] }))?;
        Ok(())
    }

    fn wait_file_ready(&mut self, deadline: Instant) -> Result<(), MpvError> {
        loop {
            let idle = self.get_prop_bool("idle-active")?.unwrap_or(true);
            if !idle {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(MpvError::Ipc("timed out waiting for file load".into()));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Load a file and only start audible playback when `autoplay` is true.
    ///
    /// mpv may begin decoding as soon as `loadfile` is accepted; we always pause
    /// before load, wait until the file is actually ready, then set the final
    /// pause state so restore-on-launch cannot leak audio when the user opted out.
    fn loadfile_controlled(&mut self, path: &str, start: f64, autoplay: bool) -> Result<(), MpvError> {
        self.set_prop_bool("pause", true)?;
        let args = if start > 0.05 {
            vec![
                Value::String("loadfile".into()),
                Value::String(path.into()),
                Value::String("replace".into()),
                Value::String(format!("start={start:.3}")),
            ]
        } else {
            vec![
                Value::String("loadfile".into()),
                Value::String(path.into()),
                Value::String("replace".into()),
            ]
        };
        self.request(json!({ "command": Value::Array(args) }))?;
        self.wait_file_ready(Instant::now() + Duration::from_secs(20))?;
        self.set_prop_bool("pause", !autoplay)?;
        Ok(())
    }
}

/// Single batched read of properties used for UI transport state.
#[derive(Debug, Clone)]
pub struct MpvTransportRead {
    pub position_sec: f64,
    pub duration_sec: Option<f64>,
    pub paused: bool,
    pub speed: f64,
    pub eof: bool,
    pub idle: bool,
}

pub struct MpvController {
    conn: Option<MpvConnection>,
}

impl Default for MpvController {
    fn default() -> Self {
        Self { conn: None }
    }
}

impl Drop for MpvController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl MpvController {
    pub fn ensure_running(&mut self) -> Result<(), MpvError> {
        if self.conn.is_some() {
            return Ok(());
        }
        self.conn = Some(start_mpv()?);
        Ok(())
    }

    /// Drop the current mpv session so the next operation spawns a fresh process.
    pub fn reset_session(&mut self) {
        self.shutdown();
    }

    pub fn shutdown(&mut self) {
        if let Some(mut c) = self.conn.take() {
            terminate_child(&mut c.child);
            let _ = std::fs::remove_file(&c.sock_path);
        }
    }

    fn ipc_recoverable(err: &MpvError) -> bool {
        match err {
            MpvError::Ipc(msg) => {
                let m = msg.to_lowercase();
                m.contains("socket closed")
                    || m.contains("connection reset")
                    || m.contains("broken pipe")
                    || m.contains("not connected")
            }
            MpvError::Json(_) => false,
            MpvError::Spawn(_) | MpvError::SocketTimeout | MpvError::Command(_) => true,
        }
    }

    fn with_conn<T>(&mut self, f: impl FnOnce(&mut MpvConnection) -> Result<T, MpvError>) -> Result<T, MpvError> {
        self.ensure_running()?;
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| MpvError::Ipc("no connection".into()))?;
        let out = f(conn);
        if let Err(ref e) = out {
            if Self::ipc_recoverable(e) {
                self.reset_session();
            }
        }
        out
    }

    pub fn load_file(&mut self, path: &str, start: f64) -> Result<(), MpvError> {
        self.load_file_controlled(path, start, true)
    }

    /// Load file and leave playback **paused** at `start` (no audible start).
    pub fn load_file_start_paused(&mut self, path: &str, start: f64) -> Result<(), MpvError> {
        self.load_file_controlled(path, start, false)
    }

    pub fn load_file_controlled(&mut self, path: &str, start: f64, autoplay: bool) -> Result<(), MpvError> {
        self.with_conn(|c| c.loadfile_controlled(path, start, autoplay))
    }

    pub fn pause(&mut self) -> Result<(), MpvError> {
        self.with_conn(|c| c.set_prop_bool("pause", true))
    }

    pub fn resume(&mut self) -> Result<(), MpvError> {
        self.with_conn(|c| c.set_prop_bool("pause", false))
    }

    pub fn set_pause(&mut self, paused: bool) -> Result<(), MpvError> {
        self.with_conn(|c| c.set_prop_bool("pause", paused))
    }

    pub fn toggle_pause(&mut self) -> Result<bool, MpvError> {
        self.with_conn(|c| {
            let cur = c.get_prop_bool("pause")?.unwrap_or(false);
            let next = !cur;
            c.set_prop_bool("pause", next)?;
            Ok(next)
        })
    }

    pub fn seek(&mut self, seconds: f64) -> Result<(), MpvError> {
        self.with_conn(|c| {
            // mpv returns a generic "error running command" for seek when nothing is loaded.
            if c.get_prop_bool("idle-active")?.unwrap_or(true) {
                return Ok(());
            }
            c.request(json!({ "command": ["seek", seconds, "absolute"] }))?;
            Ok(())
        })
    }

    pub fn seek_relative(&mut self, delta: f64) -> Result<(), MpvError> {
        self.with_conn(|c| {
            if c.get_prop_bool("idle-active")?.unwrap_or(true) {
                return Ok(());
            }
            c.request(json!({ "command": ["seek", delta, "relative"] }))?;
            Ok(())
        })
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<(), MpvError> {
        self.with_conn(|c| c.set_prop_f64("speed", speed))
    }

    pub fn time_pos(&mut self) -> Result<Option<f64>, MpvError> {
        self.with_conn(|c| c.get_prop_f64("time-pos"))
    }

    pub fn duration(&mut self) -> Result<Option<f64>, MpvError> {
        self.with_conn(|c| c.get_prop_f64("duration"))
    }

    pub fn time_pos_lenient(&mut self) -> f64 {
        self.time_pos()
            .ok()
            .flatten()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(0.0)
    }

    pub fn duration_lenient(&mut self) -> Option<f64> {
        self.duration()
            .ok()
            .flatten()
            .filter(|v| v.is_finite() && *v > 0.0)
    }

    pub fn eof_reached(&mut self) -> Result<bool, MpvError> {
        Ok(self
            .with_conn(|c| c.get_prop_bool("eof-reached"))?
            .unwrap_or(false))
    }

    pub fn eof_reached_lenient(&mut self) -> bool {
        self.eof_reached().unwrap_or(false)
    }

    /// Raw `get_property` data (e.g. `chapter-list` for M4B).
    pub fn get_property_json(&mut self, name: &str) -> Result<Option<Value>, MpvError> {
        self.with_conn(|c| c.get_prop_json(name))
    }

    /// Passive transport read for background consumers (e.g. the OS media-key
    /// sync loop). Returns `None` when mpv is not currently connected or any
    /// read fails. Unlike [`read_transport_state`], this never spawns a new mpv
    /// process and never resets the session, so it cannot interfere with the
    /// foreground engine lifecycle or with manual recovery.
    pub fn peek_transport(&mut self) -> Option<MpvTransportRead> {
        let conn = self.conn.as_mut()?;
        let position_sec = conn
            .get_prop_f64("time-pos")
            .ok()?
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(0.0);
        let duration_sec = conn
            .get_prop_f64("duration")
            .ok()?
            .filter(|v| v.is_finite() && *v > 0.0);
        let paused = conn.get_prop_bool("pause").ok()?.unwrap_or(false);
        let speed = conn
            .get_prop_f64("speed")
            .ok()?
            .filter(|v| v.is_finite())
            .unwrap_or(1.0);
        let eof = conn.get_prop_bool("eof-reached").ok()?.unwrap_or(false);
        let idle = conn.get_prop_bool("idle-active").ok()?.unwrap_or(true);
        Some(MpvTransportRead {
            position_sec,
            duration_sec,
            paused,
            speed,
            eof,
            idle,
        })
    }

    /// Read all transport-related mpv properties in one IPC session (one mutex hold from the caller).
    pub fn read_transport_state(&mut self) -> Result<MpvTransportRead, MpvError> {
        self.with_conn(|c| {
            let position_sec = c
                .get_prop_f64("time-pos")?
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(0.0);
            let duration_sec = c
                .get_prop_f64("duration")?
                .filter(|v| v.is_finite() && *v > 0.0);
            let paused = c.get_prop_bool("pause")?.unwrap_or(false);
            let speed = c
                .get_prop_f64("speed")?
                .filter(|v| v.is_finite())
                .unwrap_or(1.0);
            let eof = c.get_prop_bool("eof-reached")?.unwrap_or(false);
            let idle = c.get_prop_bool("idle-active")?.unwrap_or(true);
            Ok(MpvTransportRead {
                position_sec,
                duration_sec,
                paused,
                speed,
                eof,
                idle,
            })
        })
    }
}

/// Kill orphaned ChapterCheck mpv engines left behind by crashed or force-killed app runs.
pub fn cleanup_orphaned_processes() {
    cleanup_orphaned_mpv_processes();
    if let Some(runtime_dir) = runtime_dir() {
        cleanup_stale_mpv_sockets(&runtime_dir);
    }
}

fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(std::env::temp_dir()))
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn configure_child_process(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            // If the app exits abruptly, the kernel terminates mpv with us.
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_child_process(_cmd: &mut Command) {}

#[cfg(unix)]
fn cleanup_orphaned_mpv_processes() {
    let my_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(pid) = name.parse::<i32>() else {
            continue;
        };
        if pid as u32 == my_pid {
            continue;
        }
        let cmdline_path = entry.path().join("cmdline");
        let Ok(raw) = std::fs::read(cmdline_path) else {
            continue;
        };
        if !raw.windows(4).any(|w| w == b"mpv\0") {
            continue;
        }
        let cmdline = String::from_utf8_lossy(&raw);
        if !cmdline.contains("chaptercheck-mpv") {
            continue;
        }
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn cleanup_orphaned_mpv_processes() {}

fn cleanup_stale_mpv_sockets(runtime_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == MPV_SOCKET_NAME
            || (name.starts_with(MPV_SOCKET_LEGACY_PREFIX) && name.ends_with(".sock"))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn start_mpv() -> Result<MpvConnection, MpvError> {
    cleanup_orphaned_processes();

    let runtime_dir = runtime_dir().unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&runtime_dir);
    let sock_path = runtime_dir.join(MPV_SOCKET_NAME);
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    let mpv_bin = std::env::var("MPV_PATH").unwrap_or_else(|_| "mpv".into());
    let ipc = format!("--input-ipc-server={}", sock_path.display());
    let mut cmd = Command::new(&mpv_bin);
    cmd.args([
        "--idle=yes",
        "--keep-open=yes",
        "--pause",
        "--no-video",
        "--no-terminal",
        "--really-quiet",
        "--no-config",
        "--load-scripts=no",
        &ipc,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    configure_child_process(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        MpvError::Spawn(format!(
            "{e}. Install mpv (e.g. `sudo apt install mpv`) or set MPV_PATH."
        ))
    })?;

    let deadline = Instant::now() + Duration::from_secs(8);
    let stream = loop {
        if sock_path.exists() {
            if let Ok(s) = UnixStream::connect(&sock_path) {
                break s;
            }
        }
        if Instant::now() > deadline {
            terminate_child(&mut child);
            let _ = std::fs::remove_file(&sock_path);
            return Err(MpvError::SocketTimeout);
        }
        std::thread::sleep(Duration::from_millis(40));
    };

    let io = BufReader::new(stream);
    Ok(MpvConnection {
        child,
        sock_path,
        io,
        next_id: 1,
    })
}
