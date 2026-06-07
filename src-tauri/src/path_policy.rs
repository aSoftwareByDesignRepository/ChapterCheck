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

/// Resolve a catalog-stored path to the best on-disk location.
pub fn tracked_file_on_disk(path: &Path) -> Option<PathBuf> {
    if let Ok(canon) = canonicalize_existing(path) {
        if canon.exists() {
            return Some(canon);
        }
    }
    if path.exists() {
        return Some(path.to_path_buf());
    }
    None
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
}
