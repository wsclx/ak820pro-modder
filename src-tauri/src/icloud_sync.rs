//! iCloud-Drive based profile sync.
//!
//! Mirrors the app's user-mutable state (right now just `automations.json`;
//! more files later) into `~/Library/Mobile Documents/com~apple~CloudDocs/
//! ak820pro-modder/`. macOS's iCloud Drive daemon handles the actual
//! cross-device shuttling for free — we just have to put the file there
//! and read it back.
//!
//! # Why not the entitlement-gated `iCloud~<bundleid>` container?
//!
//! Apple's "official" iCloud container path for an app is
//! `~/Library/Mobile Documents/iCloud~io~github~wsclx~ak820pro-modder/`,
//! but reaching it needs `com.apple.developer.icloud-services`
//! entitlements *and* a provisioning profile *and* a signed app bundle.
//! For an OSS app we'd be punishing every user who builds from source.
//!
//! Instead we write to the user's plain iCloud Drive root under a
//! well-named folder. Same daemon does the same syncing, no entitlements
//! required, the user sees an obvious `ak820pro-modder/` folder in
//! Finder under iCloud Drive that they can inspect or wipe by hand.
//!
//! # Sync model
//!
//! Last-write-wins by `mtime` per file. `push()` copies local → iCloud,
//! `pull_if_newer()` copies iCloud → local *only* when the iCloud copy
//! is strictly newer. The frontend orchestrates when to call which
//! (pull on startup, push after every save). No per-record merging in
//! this phase — same automations.json file replaces the other side
//! wholesale. Per-ID merge is a follow-up.

use serde::Serialize;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::AppError;

/// Subdirectory of the user's iCloud Drive that holds our synced files.
/// Picked to be self-explanatory in Finder ("ak820pro-modder") rather
/// than a reverse-DNS bundle ID.
const SYNC_SUBDIR: &str = "ak820pro-modder";

/// File name inside the sync dir for the automations payload. Matches
/// the local filename exactly so manual inspection (`diff`, etc.) is
/// trivial.
const AUTOMATIONS_FILENAME: &str = "automations.json";

/// Snapshot of the sync subsystem's view of the world. Returned by the
/// `icloud_sync_status` command so the UI can render its toggle + status
/// line without making multiple round-trips.
#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    /// `true` when `~/Library/Mobile Documents/com~apple~CloudDocs/`
    /// exists on this machine. Doesn't necessarily mean iCloud Drive is
    /// signed in or syncing — only that the filesystem path is there
    /// for us to write to.
    pub icloud_available: bool,
    /// Display path to our sync subfolder, when `icloud_available`.
    /// UI shows this so the user can verify they're looking at the
    /// right place in Finder.
    pub icloud_path: Option<String>,
    /// `true` when the automations.json file exists in the iCloud
    /// folder. False on a freshly-created sync setup.
    pub remote_automations_present: bool,
    /// mtime of the remote automations.json in milliseconds since epoch.
    /// `None` when no remote copy yet.
    pub remote_automations_mtime_ms: Option<u64>,
}

/// Locate the user's iCloud Drive root, if iCloud Drive is set up on
/// this machine. Doesn't probe sync state — just checks the filesystem.
pub fn detect_icloud_root() -> Option<PathBuf> {
    // We deliberately use `HOME` rather than `dirs::home_dir()`. The
    // latter would pull in a dependency for a one-line lookup; the
    // former is stable + std-only.
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join("Library/Mobile Documents/com~apple~CloudDocs");
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

fn mtime_ms(path: &std::path::Path) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?;
    let modified = m.modified().ok()?;
    let dur = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as u64)
}

/// Compute the snapshot using a caller-supplied iCloud root. Used by
/// the public [`status`] (with the real iCloud Drive root) and by the
/// unit tests (with a tmp dir as a fake root). Returns the "no iCloud"
/// shape when `root` is `None` — same as on a host without iCloud
/// Drive set up.
fn status_with_root(root: Option<&std::path::Path>) -> SyncStatus {
    let sync_dir = root.map(|r| r.join(SYNC_SUBDIR));
    let icloud_path = sync_dir.as_ref().map(|p| p.display().to_string());
    let remote = sync_dir.as_ref().map(|d| d.join(AUTOMATIONS_FILENAME));
    let (remote_present, remote_mtime) = match remote {
        Some(ref p) if p.exists() => (true, mtime_ms(p)),
        _ => (false, None),
    };
    SyncStatus {
        icloud_available: root.is_some(),
        icloud_path,
        remote_automations_present: remote_present,
        remote_automations_mtime_ms: remote_mtime,
    }
}

pub fn status() -> SyncStatus {
    status_with_root(detect_icloud_root().as_deref())
}

/// Push `local_path` into the sync subdirectory under `icloud_root`.
/// Returns the post-write remote mtime in milliseconds. Splitting the
/// iCloud root out as a parameter keeps the function unit-testable
/// against a tmp directory — `push_automations()` is the thin wrapper
/// that supplies the real root.
fn push_automations_into(
    icloud_root: &std::path::Path,
    local_path: &std::path::Path,
) -> Result<u64, AppError> {
    if !local_path.exists() {
        return Err(AppError::Protocol(format!(
            "local automations.json missing at {}",
            local_path.display()
        )));
    }
    let dir = icloud_root.join(SYNC_SUBDIR);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Protocol(format!("create_dir({}): {e}", dir.display())))?;
    let dest = dir.join(AUTOMATIONS_FILENAME);
    std::fs::copy(local_path, &dest)
        .map_err(|e| AppError::Protocol(format!("copy local → iCloud: {e}")))?;
    mtime_ms(&dest)
        .ok_or_else(|| AppError::Protocol("could not read remote mtime after push".into()))
}

/// Copy the local automations.json into iCloud Drive. Creates the
/// sync subfolder if it doesn't exist yet. Returns the post-write
/// remote mtime in ms so the UI can show "last pushed: …".
pub fn push_automations(local_path: &std::path::Path) -> Result<u64, AppError> {
    let root = detect_icloud_root()
        .ok_or_else(|| AppError::Protocol("iCloud Drive not detected on this machine".into()))?;
    push_automations_into(&root, local_path)
}

/// Test-friendly variant of [`pull_automations_if_newer`] taking an
/// explicit iCloud root. Same semantics as the public function;
/// the production wrapper just supplies the real root.
fn pull_automations_if_newer_from(
    icloud_root: &std::path::Path,
    local_path: &std::path::Path,
) -> Result<Option<u64>, AppError> {
    let remote = icloud_root.join(SYNC_SUBDIR).join(AUTOMATIONS_FILENAME);
    if !remote.exists() {
        return Ok(None);
    }
    let remote_mtime = mtime_ms(&remote)
        .ok_or_else(|| AppError::Protocol("could not stat remote automations.json".into()))?;
    let local_mtime = mtime_ms(local_path).unwrap_or(0);
    if remote_mtime <= local_mtime {
        return Ok(None);
    }
    // First-launch-on-fresh-machine case: app_data_dir may not exist yet.
    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Protocol(format!("create local dir: {e}")))?;
    }
    std::fs::copy(&remote, local_path)
        .map_err(|e| AppError::Protocol(format!("copy iCloud → local: {e}")))?;
    Ok(Some(remote_mtime))
}

/// If the iCloud copy of automations.json is strictly newer than the
/// local copy (or no local copy exists), overwrite the local copy and
/// return `Some(remote_mtime_ms)`. Otherwise return `Ok(None)` — the
/// "nothing to do" case, not an error.
pub fn pull_automations_if_newer(local_path: &std::path::Path) -> Result<Option<u64>, AppError> {
    let root = detect_icloud_root()
        .ok_or_else(|| AppError::Protocol("iCloud Drive not detected on this machine".into()))?;
    pull_automations_if_newer_from(&root, local_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_struct_serialises_with_expected_keys() {
        // Frontend type contract — if we ever rename a field here we
        // want the test to break before the UI does.
        let s = SyncStatus {
            icloud_available: false,
            icloud_path: None,
            remote_automations_present: false,
            remote_automations_mtime_ms: None,
        };
        let j = serde_json::to_value(&s).unwrap();
        assert!(j.get("icloud_available").is_some());
        assert!(j.get("icloud_path").is_some());
        assert!(j.get("remote_automations_present").is_some());
        assert!(j.get("remote_automations_mtime_ms").is_some());
    }

    /// Allocate an isolated tmp dir per-test so concurrent `cargo
    /// test` runs (and reruns of the same test) don't trip over each
    /// other. Returns `(fake_icloud_root, local_app_data_dir)`.
    fn fresh_test_dirs(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let pid = std::process::id();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ak820-icloud-{tag}-{pid}-{nonce}"));
        let _ = std::fs::remove_dir_all(&base);
        let icloud = base.join("fake-icloud");
        let app_data = base.join("fake-app-data");
        std::fs::create_dir_all(&icloud).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        (icloud, app_data)
    }

    #[test]
    fn pull_returns_none_when_remote_missing() {
        // Hermetic: fake iCloud root with no automations.json inside.
        // Previously this test depended on the host's real iCloud Drive
        // setup — broke on CI because the runner has no iCloud account
        // and `detect_icloud_root()` returned None. The refactored
        // `pull_automations_if_newer_from()` lets us inject the root.
        let (icloud, app_data) = fresh_test_dirs("pull-none");
        let local = app_data.join("automations.json");
        std::fs::write(&local, b"[]").unwrap();

        let res = pull_automations_if_newer_from(&icloud, &local).unwrap();
        assert!(
            res.is_none(),
            "expected Ok(None) when remote is missing, got {res:?}",
        );

        // Local file untouched.
        let content = std::fs::read(&local).unwrap();
        assert_eq!(content, b"[]");

        std::fs::remove_dir_all(icloud.parent().unwrap()).ok();
    }

    #[test]
    fn pull_overwrites_local_when_remote_is_newer() {
        let (icloud, app_data) = fresh_test_dirs("pull-newer");

        // Stage a local file with content A and an older mtime, plus a
        // remote file with content B and a newer mtime. We achieve the
        // ordering by writing the local *first* and sleeping briefly
        // before staging the remote; mtime resolution on macOS/Linux
        // filesystems is sub-second but a 30 ms gap is plenty.
        let local = app_data.join("automations.json");
        std::fs::write(&local, b"local").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let sync_dir = icloud.join(SYNC_SUBDIR);
        std::fs::create_dir_all(&sync_dir).unwrap();
        let remote = sync_dir.join(AUTOMATIONS_FILENAME);
        std::fs::write(&remote, b"remote-newer").unwrap();

        let res = pull_automations_if_newer_from(&icloud, &local).unwrap();
        assert!(res.is_some(), "expected pull to fire");
        let content = std::fs::read(&local).unwrap();
        assert_eq!(content, b"remote-newer");

        std::fs::remove_dir_all(icloud.parent().unwrap()).ok();
    }

    #[test]
    fn pull_is_noop_when_local_is_newer() {
        let (icloud, app_data) = fresh_test_dirs("pull-noop");

        let sync_dir = icloud.join(SYNC_SUBDIR);
        std::fs::create_dir_all(&sync_dir).unwrap();
        let remote = sync_dir.join(AUTOMATIONS_FILENAME);
        std::fs::write(&remote, b"remote-older").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let local = app_data.join("automations.json");
        std::fs::write(&local, b"local-newer").unwrap();

        let res = pull_automations_if_newer_from(&icloud, &local).unwrap();
        assert!(res.is_none(), "expected Ok(None) when local is newer");
        let content = std::fs::read(&local).unwrap();
        assert_eq!(content, b"local-newer");

        std::fs::remove_dir_all(icloud.parent().unwrap()).ok();
    }

    #[test]
    fn push_creates_sync_subdir_and_copies_file() {
        let (icloud, app_data) = fresh_test_dirs("push");
        let local = app_data.join("automations.json");
        std::fs::write(&local, b"local-content").unwrap();

        // The sync subdir doesn't exist yet — push should create it.
        let mtime = push_automations_into(&icloud, &local).unwrap();
        assert!(mtime > 0, "mtime should be a real timestamp");

        let remote = icloud.join(SYNC_SUBDIR).join(AUTOMATIONS_FILENAME);
        assert!(remote.exists());
        let content = std::fs::read(&remote).unwrap();
        assert_eq!(content, b"local-content");

        std::fs::remove_dir_all(icloud.parent().unwrap()).ok();
    }

    #[test]
    fn status_reports_no_icloud_when_root_absent() {
        let s = status_with_root(None);
        assert!(!s.icloud_available);
        assert!(s.icloud_path.is_none());
        assert!(!s.remote_automations_present);
        assert!(s.remote_automations_mtime_ms.is_none());
    }

    #[test]
    fn status_reports_remote_when_file_present() {
        let (icloud, _app_data) = fresh_test_dirs("status-remote");
        let sync_dir = icloud.join(SYNC_SUBDIR);
        std::fs::create_dir_all(&sync_dir).unwrap();
        let remote = sync_dir.join(AUTOMATIONS_FILENAME);
        std::fs::write(&remote, b"x").unwrap();

        let s = status_with_root(Some(&icloud));
        assert!(s.icloud_available);
        assert!(s.icloud_path.is_some());
        assert!(s.remote_automations_present);
        assert!(s.remote_automations_mtime_ms.is_some());

        std::fs::remove_dir_all(icloud.parent().unwrap()).ok();
    }
}
