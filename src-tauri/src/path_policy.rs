//! Canonical path checks and registered-root containment for catalog security.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathPolicyError {
    #[error("{0}")]
    Message(String),
}

pub fn canonicalize_existing(path: &Path) -> Result<PathBuf, PathPolicyError> {
    path.canonicalize()
        .map_err(|e| PathPolicyError::Message(format!("Cannot access {}: {e}", path.display())))
}

/// Resolve a catalog-stored path to the canonical on-disk location.
///
/// Only canonical paths are returned so a dangling or unreadable symlink cannot
/// bypass root containment. Missing or inaccessible files are `None`.
pub fn tracked_file_on_disk(path: &Path) -> Option<PathBuf> {
    canonicalize_existing(path).ok().filter(|c| c.is_file())
}

/// After canonicalize, ensure `path` is the same as or nested under `root`.
pub fn is_under_root(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

/// Reject symlink escapes: canonicalize then verify still under root.
pub fn canonicalize_under_root(path: &Path, root: &Path) -> Result<PathBuf, PathPolicyError> {
    let root_canon = canonicalize_existing(root)?;
    let path_canon = canonicalize_existing(path)?;
    if !is_under_root(&path_canon, &root_canon) {
        return Err(PathPolicyError::Message(format!(
            "Path {} is outside library root {}",
            path_canon.display(),
            root_canon.display()
        )));
    }
    Ok(path_canon)
}

/// True when `path` resolves under any registered root.
pub fn is_under_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    canonicalize_existing(path)
        .ok()
        .map(|c| roots.iter().any(|r| is_under_root(&c, r)))
        .unwrap_or(false)
}

/// Library roots that would scan the whole machine or kernel virtual filesystems.
pub fn is_forbidden_library_root(path: &Path) -> bool {
    let Ok(canon) = canonicalize_existing(path) else {
        return false;
    };
    if canon.parent().is_none() {
        return true;
    }
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(home_canon) = canonicalize_existing(Path::new(&home)) {
            if canon == home_canon {
                return true;
            }
        }
    }
    let s = canon.to_string_lossy();
    const BLOCKED_PREFIXES: &[&str] = &[
        "/proc", "/sys", "/dev", "/etc", "/boot", "/run", "/root", "/usr", "/var",
    ];
    if BLOCKED_PREFIXES
        .iter()
        .any(|b| s == *b || s.starts_with(&format!("{b}/")))
    {
        return true;
    }
    const BLOCKED_EXACT: &[&str] = &["/home", "/opt", "/tmp", "/media", "/mnt"];
    BLOCKED_EXACT.iter().any(|b| s == *b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn canonicalize_under_root_accepts_nested_file() {
        let base = std::env::temp_dir().join("cc_path_policy_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("book")).unwrap();
        let f = base.join("book/ch1.mp3");
        fs::File::create(&f).unwrap().write_all(b"x").unwrap();
        let got = canonicalize_under_root(&f, &base).unwrap();
        assert!(got.ends_with("ch1.mp3"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn tracked_file_on_disk_rejects_missing() {
        let missing = std::env::temp_dir().join("cc_path_policy_missing_no_such_file.mp3");
        let _ = fs::remove_file(&missing);
        assert!(tracked_file_on_disk(&missing).is_none());
    }

    #[test]
    fn tracked_file_on_disk_returns_canonical_file() {
        let base = std::env::temp_dir().join("cc_path_policy_tracked");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let f = base.join("t.mp3");
        fs::File::create(&f).unwrap().write_all(b"x").unwrap();
        let got = tracked_file_on_disk(&f).unwrap();
        assert!(got.is_file());
        assert!(got.ends_with("t.mp3"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn canonicalize_under_root_rejects_symlink_escape() {
        let base = std::env::temp_dir().join("cc_path_policy_symlink");
        let _ = fs::remove_dir_all(&base);
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.mp3");
        fs::File::create(&secret).unwrap().write_all(b"x").unwrap();
        let link = root.join("escape.mp3");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let err = canonicalize_under_root(&link, &root).unwrap_err();
        assert!(
            err.to_string().contains("outside library root"),
            "symlink out of root must be rejected, got {err}"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn is_under_root_does_not_match_sibling_prefix() {
        let music = PathBuf::from("/home/user/music");
        let music_extra = PathBuf::from("/home/user/music-extra/album.mp3");
        assert!(!is_under_root(&music_extra, &music));
        let nested = PathBuf::from("/home/user/music/album.mp3");
        assert!(is_under_root(&nested, &music));
    }

    #[test]
    fn filesystem_root_is_forbidden_library_root() {
        assert!(is_forbidden_library_root(Path::new("/")));
        assert!(
            is_forbidden_library_root(Path::new("/proc")),
            "/proc itself must be blocked"
        );
        assert!(
            is_forbidden_library_root(Path::new("/proc/self")),
            "paths under /proc must be blocked"
        );
        let tmp = std::env::temp_dir();
        if tmp.exists() {
            let canon = tmp.canonicalize().unwrap_or(tmp.clone());
            if canon != Path::new("/tmp") {
                assert!(
                    !is_forbidden_library_root(&tmp),
                    "a nested temp folder must still be allowed as a library"
                );
            }
        }
    }

    #[test]
    fn home_and_os_roots_are_forbidden_library_roots() {
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            if home.is_dir() {
                assert!(
                    is_forbidden_library_root(&home),
                    "the user's home directory must not be a library root (would scan everything in it)"
                );
            }
        }
        if Path::new("/etc").is_dir() {
            assert!(
                is_forbidden_library_root(Path::new("/etc")),
                "/etc must not be a library root"
            );
        }
        if Path::new("/usr").is_dir() {
            assert!(
                is_forbidden_library_root(Path::new("/usr")),
                "/usr must not be a library root"
            );
        }
        if Path::new("/home").is_dir() {
            assert!(
                is_forbidden_library_root(Path::new("/home")),
                "/home itself must not be a library root; a folder *inside* a user's home is still allowed"
            );
        }
    }

    #[test]
    fn is_under_any_root_requires_real_membership() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("cc_any_root_{stamp}"));
        let root = base.join("lib");
        let other = base.join("other");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&other).unwrap();
        let inside = root.join("a.mp3");
        fs::File::create(&inside).unwrap().write_all(b"x").unwrap();
        let roots = vec![root.canonicalize().unwrap()];
        assert!(is_under_any_root(&inside, &roots));
        assert!(!is_under_any_root(&other, &roots));
        assert!(!is_under_any_root(&inside, &[]));
        let _ = fs::remove_dir_all(&base);
    }
}
