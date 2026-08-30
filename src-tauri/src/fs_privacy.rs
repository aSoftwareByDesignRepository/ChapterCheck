//! Owner-only permissions for library data and the mpv IPC socket.
//!
//! Listening history and the mpv JSON IPC must not be reachable by other
//! local accounts on a shared machine (world-readable SQLite or a socket
//! under `/tmp`).

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// True when group/other have no permission bits (0700/0600 after masking).
#[cfg(unix)]
pub fn owner_only_mode(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o077 == 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn owner_only_mode(_path: &Path) -> bool {
    true
}

#[cfg(unix)]
pub fn restrict_dir_owner_only(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub fn restrict_dir_owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn restrict_file_owner_only(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub fn restrict_file_owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Private directory for the mpv Unix socket. Never `/tmp` (world-writable).
pub fn ipc_runtime_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        if try_prepare_private_dir(&xdg) {
            return xdg;
        }
    }
    let fallback = crate::catalog::app_data_dir()
        .map(|d| d.join("run"))
        .unwrap_or_else(|_| {
            std::env::temp_dir().join(format!("chaptercheck-run-{}", std::process::id()))
        });
    let _ = try_prepare_private_dir(&fallback);
    fallback
}

fn try_prepare_private_dir(path: &Path) -> bool {
    if fs::create_dir_all(path).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        let Ok(meta) = fs::metadata(path) else {
            return false;
        };
        // Never adopt a directory other users can write (e.g. /tmp or a 0777 folder).
        if meta.permissions().mode() & 0o002 != 0 {
            return false;
        }
    }
    if restrict_dir_owner_only(path).is_err() {
        return false;
    }
    owner_only_mode(path)
}

pub fn restrict_sqlite_sidecars(db_path: &Path) {
    let _ = restrict_file_owner_only(db_path);
    let mut wal = db_path.as_os_str().to_os_string();
    wal.push("-wal");
    let _ = restrict_file_owner_only(Path::new(&wal));
    let mut shm = db_path.as_os_str().to_os_string();
    shm.push("-shm");
    let _ = restrict_file_owner_only(Path::new(&shm));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn stamp() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn restrict_dir_clears_group_and_other_bits() {
        let dir = std::env::temp_dir().join(format!("cc_priv_dir_{}", stamp()));
        fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            let mut open = fs::metadata(&dir).unwrap().permissions();
            open.set_mode(0o755);
            fs::set_permissions(&dir, open).unwrap();
            assert!(!owner_only_mode(&dir));
            restrict_dir_owner_only(&dir).unwrap();
            assert!(owner_only_mode(&dir));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restrict_file_clears_group_and_other_bits() {
        let dir = std::env::temp_dir().join(format!("cc_priv_file_{}", stamp()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("library.sqlite3");
        fs::write(&f, b"x").unwrap();
        #[cfg(unix)]
        {
            let mut open = fs::metadata(&f).unwrap().permissions();
            open.set_mode(0o644);
            fs::set_permissions(&f, open).unwrap();
            assert!(!owner_only_mode(&f));
            restrict_file_owner_only(&f).unwrap();
            assert!(owner_only_mode(&f));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ipc_runtime_dir_rejects_world_writable_xdg() {
        let world = std::env::temp_dir().join(format!("cc_xdg_world_{}", stamp()));
        fs::create_dir_all(&world).unwrap();
        #[cfg(unix)]
        {
            let mut open = fs::metadata(&world).unwrap().permissions();
            open.set_mode(0o777);
            fs::set_permissions(&world, open).unwrap();
            let old = std::env::var("XDG_RUNTIME_DIR").ok();
            std::env::set_var("XDG_RUNTIME_DIR", &world);
            let got = ipc_runtime_dir();
            match old {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
            assert_ne!(got, world, "must not place the mpv socket in a world-writable dir");
            assert!(
                owner_only_mode(&got),
                "mpv IPC dir must be owner-only, got {}",
                got.display()
            );
        }
        let _ = fs::remove_dir_all(&world);
    }
}
