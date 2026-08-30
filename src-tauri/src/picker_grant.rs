//! One-shot grants for folders the user actually picked (dialog or OS open).
//!
//! `add_library_root` takes a raw path. Without this, a compromised webview
//! can register `$HOME/Documents` and scan it. A grant is issued only from
//! `pick_library_folder`, `pick_open_folder`, or an OS file-manager open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const GRANT_TTL: Duration = Duration::from_secs(1_800);

pub struct PickerGrantSet {
    grants: HashMap<PathBuf, Instant>,
    ttl: Duration,
}

impl Default for PickerGrantSet {
    fn default() -> Self {
        Self {
            grants: HashMap::new(),
            ttl: GRANT_TTL,
        }
    }
}

impl PickerGrantSet {
    pub fn grant(&mut self, canonical_dir: PathBuf) {
        self.sweep();
        self.grants.insert(canonical_dir, Instant::now());
    }

    /// True iff `canonical_dir` was granted and not yet consumed or expired.
    /// Does not consume — a failed `add_root` must not burn the pick.
    pub fn contains(&mut self, canonical_dir: &Path) -> bool {
        self.sweep();
        self.grants.contains_key(canonical_dir)
    }

    /// True iff `canonical_dir` was granted and not yet consumed or expired.
    pub fn consume(&mut self, canonical_dir: &Path) -> bool {
        self.sweep();
        self.grants.remove(canonical_dir).is_some()
    }

    fn sweep(&mut self) {
        let now = Instant::now();
        let ttl = self.ttl;
        self.grants
            .retain(|_, issued| grant_is_fresh(*issued, now, ttl));
    }
}

/// Fresh iff age is strictly less than TTL. Equal-to-TTL is expired (one-shot
/// window must not linger an extra tick after the deadline).
fn grant_is_fresh(issued: Instant, now: Instant, ttl: Duration) -> bool {
    now.checked_duration_since(issued)
        .map(|age| age < ttl)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn grant_ttl_is_thirty_minutes() {
        assert_eq!(GRANT_TTL, Duration::from_secs(1_800));
    }

    #[test]
    fn consume_requires_a_prior_grant() {
        let mut set = PickerGrantSet::default();
        let dir = PathBuf::from("/tmp/cc_grant_audiobooks");
        assert!(!set.consume(&dir), "ungranted path must not link");
        set.grant(dir.clone());
        assert!(set.consume(&dir), "picked path must link once");
        assert!(!set.consume(&dir), "grant is one-shot");
    }

    #[test]
    fn contains_does_not_consume_the_grant() {
        let mut set = PickerGrantSet::default();
        let dir = PathBuf::from("/tmp/cc_grant_peek");
        assert!(!set.contains(&dir));
        set.grant(dir.clone());
        assert!(set.contains(&dir), "peek must see the pick");
        assert!(set.contains(&dir), "peek must not burn the grant");
        assert!(set.consume(&dir), "consume still one-shot after peeks");
        assert!(!set.contains(&dir));
    }

    #[test]
    fn consume_does_not_accept_a_sibling_path() {
        let mut set = PickerGrantSet::default();
        set.grant(PathBuf::from("/tmp/cc_grant_a"));
        assert!(!set.consume(Path::new("/tmp/cc_grant_b")));
        assert!(set.consume(Path::new("/tmp/cc_grant_a")));
    }

    #[test]
    fn expired_grant_is_not_consumable() {
        let mut set = PickerGrantSet {
            grants: HashMap::new(),
            ttl: Duration::from_millis(1),
        };
        set.grant(PathBuf::from("/tmp/cc_grant_expired"));
        std::thread::sleep(Duration::from_millis(20));
        assert!(
            !set.consume(Path::new("/tmp/cc_grant_expired")),
            "stale picker grant must not link a folder"
        );
    }

    #[test]
    fn zero_ttl_and_exact_boundary_are_expired() {
        // Kills age < ttl → age <= ttl (and zero-TTL Instant races).
        let t = Instant::now();
        assert!(
            !grant_is_fresh(t, t, Duration::ZERO),
            "age=0 ttl=0 must be expired"
        );
        assert!(
            !grant_is_fresh(t, t + Duration::from_secs(30), Duration::from_secs(30)),
            "age == ttl must be expired (strict <)"
        );
        assert!(grant_is_fresh(
            t,
            t + Duration::from_secs(29),
            Duration::from_secs(30)
        ));
        let mut set = PickerGrantSet {
            grants: HashMap::new(),
            ttl: Duration::ZERO,
        };
        let dir = PathBuf::from("/tmp/cc_grant_zero_ttl");
        set.grants.insert(dir.clone(), Instant::now());
        assert!(!set.contains(&dir));
        assert!(!set.consume(&dir));
    }
}
