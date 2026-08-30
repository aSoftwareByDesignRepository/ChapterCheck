//! One-shot grants for actions that destroy files or send titles off-machine.
//!
//! The webview confirm sheet is not a trust boundary. A grant is issued only
//! after a native OS dialog whose copy is owned by Rust (path included).
//! `delete_*` / enabling online lookup consume that grant.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const GRANT_TTL: Duration = Duration::from_secs(120);

pub const USER_CANCELLED: &str = "CANCELLED_BY_USER";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DestructiveKind {
    DeleteFile(PathBuf),
    DeleteSession(String),
    EnableOnlineMetadata,
}

pub struct DestructiveGrantSet {
    grants: HashMap<DestructiveKind, Instant>,
    ttl: Duration,
}

impl Default for DestructiveGrantSet {
    fn default() -> Self {
        Self {
            grants: HashMap::new(),
            ttl: GRANT_TTL,
        }
    }
}

impl DestructiveGrantSet {
    pub fn grant(&mut self, kind: DestructiveKind) {
        self.sweep();
        self.grants.insert(kind, Instant::now());
    }

    pub fn consume(&mut self, kind: &DestructiveKind) -> bool {
        self.sweep();
        self.grants.remove(kind).is_some()
    }

    fn sweep(&mut self) {
        let now = Instant::now();
        let ttl = self.ttl;
        self.grants
            .retain(|_, issued| grant_is_fresh(*issued, now, ttl));
    }
}

fn grant_is_fresh(issued: Instant, now: Instant, ttl: Duration) -> bool {
    now.checked_duration_since(issued)
        .map(|age| age < ttl)
        .unwrap_or(false)
}

/// Stable fingerprint of the current queue so a confirm cannot delete a
/// different set of files than the user saw.
pub fn session_fingerprint(paths: &[PathBuf]) -> String {
    let mut sorted: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    sorted.sort();
    let mut hasher = Sha256::new();
    for s in &sorted {
        hasher.update(s.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

pub struct OsConfirmCopy {
    pub title: String,
    pub body: String,
    pub ok: String,
    pub cancel: String,
}

pub fn delete_file_os_copy(german: bool, file_name: &str, full_path: &str) -> OsConfirmCopy {
    if german {
        OsConfirmCopy {
            title: "Datei wirklich löschen?".into(),
            body: format!(
                "Diese Datei wird dauerhaft gelöscht und kann nicht zurückgeholt werden.\n\n{file_name}\n\n{full_path}"
            ),
            ok: "Löschen".into(),
            cancel: "Abbrechen".into(),
        }
    } else {
        OsConfirmCopy {
            title: "Delete this file for good?".into(),
            body: format!(
                "This file is permanently deleted and cannot be undone.\n\n{file_name}\n\n{full_path}"
            ),
            ok: "Delete".into(),
            cancel: "Cancel".into(),
        }
    }
}

pub fn delete_session_os_copy(german: bool, count: usize) -> OsConfirmCopy {
    if german {
        OsConfirmCopy {
            title: "Dateien wirklich löschen?".into(),
            body: format!(
                "{count} Dateien in der Warteschlange werden dauerhaft gelöscht. Das kann nicht rückgängig gemacht werden."
            ),
            ok: "Löschen".into(),
            cancel: "Abbrechen".into(),
        }
    } else {
        OsConfirmCopy {
            title: "Delete these files for good?".into(),
            body: format!(
                "{count} files in the queue will be permanently deleted. This cannot be undone."
            ),
            ok: "Delete".into(),
            cancel: "Cancel".into(),
        }
    }
}

pub fn enable_online_os_copy(german: bool) -> OsConfirmCopy {
    if german {
        OsConfirmCopy {
            title: "Titel im Internet nachschlagen?".into(),
            body: "KapitelCheck sendet Buchtitel und Künstlernamen an Open Library und MusicBrainz, um Cover und Namen zu finden.".into(),
            ok: "Erlauben".into(),
            cancel: "Nicht jetzt".into(),
        }
    } else {
        OsConfirmCopy {
            title: "Look up titles on the internet?".into(),
            body: "ChapterCheck will send book titles and artist names to Open Library and MusicBrainz to find covers and names.".into(),
            ok: "Allow".into(),
            cancel: "Not now".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn consume_requires_a_prior_grant() {
        let mut set = DestructiveGrantSet::default();
        let kind = DestructiveKind::DeleteFile(PathBuf::from("/tmp/book.m4b"));
        assert!(!set.consume(&kind));
        set.grant(kind.clone());
        assert!(set.consume(&kind));
        assert!(!set.consume(&kind), "delete grant is one-shot");
    }

    #[test]
    fn session_grant_does_not_unlock_a_single_file() {
        let mut set = DestructiveGrantSet::default();
        set.grant(DestructiveKind::DeleteSession("abc".into()));
        assert!(!set.consume(&DestructiveKind::DeleteFile(PathBuf::from("/tmp/book.m4b"))));
        assert!(set.consume(&DestructiveKind::DeleteSession("abc".into())));
    }

    #[test]
    fn expired_grant_is_not_consumable() {
        let mut set = DestructiveGrantSet {
            grants: HashMap::new(),
            ttl: Duration::from_millis(1),
        };
        set.grant(DestructiveKind::EnableOnlineMetadata);
        std::thread::sleep(Duration::from_millis(20));
        assert!(!set.consume(&DestructiveKind::EnableOnlineMetadata));
    }

    #[test]
    fn zero_ttl_and_exact_boundary_are_expired() {
        let t = Instant::now();
        assert!(!grant_is_fresh(t, t, Duration::ZERO));
        assert!(!grant_is_fresh(
            t,
            t + Duration::from_secs(120),
            Duration::from_secs(120)
        ));
        assert!(grant_is_fresh(
            t,
            t + Duration::from_secs(119),
            Duration::from_secs(120)
        ));
        let mut set = DestructiveGrantSet {
            grants: HashMap::new(),
            ttl: Duration::ZERO,
        };
        set.grants
            .insert(DestructiveKind::EnableOnlineMetadata, Instant::now());
        assert!(!set.consume(&DestructiveKind::EnableOnlineMetadata));
    }

    #[test]
    fn session_fingerprint_is_order_insensitive_and_path_sensitive() {
        let a = PathBuf::from("/tmp/a.mp3");
        let b = PathBuf::from("/tmp/b.mp3");
        assert_eq!(
            session_fingerprint(&[a.clone(), b.clone()]),
            session_fingerprint(&[b.clone(), a.clone()])
        );
        assert_ne!(
            session_fingerprint(&[a.clone()]),
            session_fingerprint(&[a, b])
        );
    }

    #[test]
    fn delete_file_os_copy_always_includes_name_and_path() {
        let copy = delete_file_os_copy(false, "Dune.m4b", "/home/alex/Dune.m4b");
        assert!(copy.body.contains("Dune.m4b"));
        assert!(copy.body.contains("/home/alex/Dune.m4b"));
        assert!(copy.title.contains("Delete"));
        let de = delete_file_os_copy(true, "Dune.m4b", "/home/alex/Dune.m4b");
        assert!(de.body.contains("Dune.m4b"));
        assert!(de.body.contains("/home/alex/Dune.m4b"));
        assert!(de.ok.contains("Löschen"));
    }

    #[test]
    fn enable_online_copy_names_the_services() {
        let en = enable_online_os_copy(false);
        assert!(en.body.contains("Open Library"));
        assert!(en.body.contains("MusicBrainz"));
        let de = enable_online_os_copy(true);
        assert!(de.body.contains("Open Library"));
        assert!(de.body.contains("MusicBrainz"));
    }
}
