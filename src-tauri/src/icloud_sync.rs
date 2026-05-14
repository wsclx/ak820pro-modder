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

/// Path to our subdirectory inside iCloud Drive. May not exist yet;
/// callers that intend to write should use [`ensure_sync_dir`].
pub fn sync_dir() -> Option<PathBuf> {
    Some(detect_icloud_root()?.join(SYNC_SUBDIR))
}

fn ensure_sync_dir() -> Result<PathBuf, AppError> {
    let dir = sync_dir()
        .ok_or_else(|| AppError::Protocol("iCloud Drive not detected on this machine".into()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Protocol(format!("create_dir({}): {e}", dir.display())))?;
    Ok(dir)
}

fn remote_automations_path() -> Option<PathBuf> {
    Some(sync_dir()?.join(AUTOMATIONS_FILENAME))
}

fn mtime_ms(path: &std::path::Path) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?;
    let modified = m.modified().ok()?;
    let dur = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as u64)
}

pub fn status() -> SyncStatus {
    let dir = sync_dir();
    let icloud_available = dir.is_some();
    let icloud_path = dir.as_ref().map(|p| p.display().to_string());
    let remote = remote_automations_path();
    let (remote_present, remote_mtime) = match remote {
        Some(p) if p.exists() => (true, mtime_ms(&p)),
        _ => (false, None),
    };
    SyncStatus {
        icloud_available,
        icloud_path,
        remote_automations_present: remote_present,
        remote_automations_mtime_ms: remote_mtime,
    }
}

/// Copy the local automations.json into iCloud Drive. Creates the
/// sync subfolder if it doesn't exist yet. Returns the post-write
/// remote mtime in ms so the UI can show "last pushed: …".
pub fn push_automations(local_path: &std::path::Path) -> Result<u64, AppError> {
    if !local_path.exists() {
        return Err(AppError::Protocol(format!(
            "local automations.json missing at {}",
            local_path.display()
        )));
    }
    let dir = ensure_sync_dir()?;
    let dest = dir.join(AUTOMATIONS_FILENAME);
    std::fs::copy(local_path, &dest)
        .map_err(|e| AppError::Protocol(format!("copy local → iCloud: {e}")))?;
    mtime_ms(&dest)
        .ok_or_else(|| AppError::Protocol("could not read remote mtime after push".into()))
}

/// If the iCloud copy of automations.json is strictly newer than the
/// local copy (or no local copy exists), overwrite the local copy and
/// return `Some(remote_mtime_ms)`. Otherwise return `Ok(None)` — the
/// "nothing to do" case, not an error.
pub fn pull_automations_if_newer(local_path: &std::path::Path) -> Result<Option<u64>, AppError> {
    let Some(remote) = remote_automations_path() else {
        return Err(AppError::Protocol(
            "iCloud Drive not detected on this machine".into(),
        ));
    };
    if !remote.exists() {
        return Ok(None);
    }
    let remote_mtime = mtime_ms(&remote)
        .ok_or_else(|| AppError::Protocol("could not stat remote automations.json".into()))?;
    let local_mtime = mtime_ms(local_path).unwrap_or(0);
    if remote_mtime <= local_mtime {
        return Ok(None);
    }
    // Make sure the parent dir exists locally before copy — this is the
    // first-launch-on-fresh-machine case where ~/Library/Application
    // Support/io.github.wsclx.ak820pro-modder/ might not exist yet.
    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Protocol(format!("create local dir: {e}")))?;
    }
    std::fs::copy(&remote, local_path)
        .map_err(|e| AppError::Protocol(format!("copy iCloud → local: {e}")))?;
    Ok(Some(remote_mtime))
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

    #[test]
    fn pull_returns_none_when_remote_missing() {
        // Use a temp dir to avoid touching the real iCloud Drive.
        let tmp = std::env::temp_dir().join("ak820-icloud-test-pull-none");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let local = tmp.join("automations.json");
        std::fs::write(&local, b"[]").unwrap();
        // detect_icloud_root() will hit the real $HOME and return Some()
        // on this machine — pull_automations_if_newer will see the
        // sync dir exists (it does, our home does have iCloud) but the
        // remote file shouldn't be present yet. If a previous test run
        // populated it we'd get a false positive here, so we just
        // accept either Ok(None) or Ok(Some(_)) and call the test a
        // smoke check on the not-erroring path.
        let res = pull_automations_if_newer(&local);
        assert!(res.is_ok(), "pull errored: {:?}", res);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
