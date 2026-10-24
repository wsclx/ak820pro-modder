//! Host-side automations library.
//!
//! Stores user-defined AppleScript / Shortcut / Shell-command entries that
//! the app can execute on demand. Each automation has a stable id and gets
//! persisted to a JSON file under the app's data directory, so the library
//! survives app restarts and is portable between Tauri builds.
//!
//! **Scope for v0.6 (manual execution only)**: there is no keyboard-side
//! trigger yet. The user clicks "Run" in the Automations view and the
//! configured command executes host-side. v0.7 will add a `marker_key`
//! field plus a global-hotkey listener (Carbon `RegisterEventHotKey`)
//! that fires the same execution path when the keyboard sends a marker
//! keycode. Persisting the schema with that future field already in mind
//! keeps the upgrade boring.
//!
//! Storage path on macOS:
//! `~/Library/Application Support/io.github.wsclx.ak820pro-modder/automations.json`

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

/// What kind of host command an automation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationKind {
    /// AppleScript / OSA — `payload` is the script body (`osascript -e`).
    AppleScript,
    /// macOS Shortcut — `payload` is the shortcut's display name; we shell
    /// out to `shortcuts run "<name>"`. Requires macOS 12+ Monterey for the
    /// `shortcuts` CLI to be present.
    Shortcut,
    /// Plain shell command via `sh -c "<payload>"`. The user is warned in
    /// the UI before saving — anything goes on the host side.
    Shell,
}

/// One library entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    /// Stable identifier — generated client-side as a u64 timestamp + counter.
    pub id: u64,
    /// User-supplied display label.
    pub name: String,
    /// Free-form description, optional.
    #[serde(default)]
    pub description: String,
    pub kind: AutomationKind,
    /// Body / shortcut name / shell command — interpreted per `kind`.
    pub payload: String,
    /// Unix millis. Used only for sort order in the UI.
    #[serde(default)]
    pub created_at: i64,
    /// Unix millis of the last edit. The iCloud merge compares it with the
    /// value seen at the last sync to tell which Mac changed the entry.
    /// Files from before that merge may lack it; they load as 0.
    #[serde(default)]
    pub updated_at: i64,

    /// **v0.7 placeholder** — when populated, the global-hotkey listener
    /// will route this HID key code to `run()`. Today it's persisted but
    /// has no effect.
    #[serde(default)]
    pub marker_hid: Option<u8>,
}

/// Output of a single execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// `true` if `exit_code == Some(0)`.
    pub success: bool,
}

/// Marks a deleted automation. Kept next to the library (locally and in
/// iCloud) so the sync merge can tell "deleted on another Mac" from "that
/// Mac never had it", and a stale copy can't bring the entry back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub id: u64,
    /// Unix millis of the deletion.
    pub deleted_at: i64,
}

/// How long a tombstone is kept. It must outlive every stale copy of the
/// entry, i.e. the longest stretch a Mac goes without syncing; 180 days
/// covers a laptop left in a drawer for a season. A tombstone is ~40 bytes,
/// so keeping it that long costs nothing. A Mac that was away even longer
/// can bring the entry back, which is visible and undone by deleting it
/// again; it never loses data.
pub const TOMBSTONE_TTL_MS: i64 = 180 * 24 * 60 * 60 * 1000;

impl Tombstone {
    pub fn expired(&self, now: i64) -> bool {
        now - self.deleted_at >= TOMBSTONE_TTL_MS
    }
}

/// File name of the tombstone list, the same locally and in iCloud.
pub const TOMBSTONES_FILENAME: &str = "automations-tombstones.json";

/// Compute the persistence path. Caller may have to create parent dirs.
pub fn storage_path(app_data: PathBuf) -> PathBuf {
    app_data.join("automations.json")
}

pub fn tombstones_path(app_data: PathBuf) -> PathBuf {
    app_data.join(TOMBSTONES_FILENAME)
}

/// Parse a JSON file. A missing file yields `T::default()` (empty list or
/// map), not an error, so first launch and pre-sync installs just work.
pub fn read_json<T: DeserializeOwned + Default>(p: &Path) -> Result<T, String> {
    if !p.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(p).map_err(|e| format!("read {p:?}: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {p:?}: {e}"))
}

/// Atomic-ish write: write to a sibling tempfile then rename so a crash
/// mid-write can't corrupt the existing file.
pub fn write_json<T: Serialize + ?Sized>(p: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    let body = serde_json::to_string_pretty(value).map_err(|e| format!("serialize: {e}"))?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write {tmp:?}: {e}"))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("rename {tmp:?} → {p:?}: {e}"))?;
    Ok(())
}

/// Serialises every read-modify-write of the library files (list,
/// tombstones, sync base). Without it an iCloud sync that read the list
/// before a marker assignment wrote it would put back the old list. Only
/// taken inside `spawn_blocking`, never held across an `.await`. Plain
/// reads don't need it: every write is an atomic rename.
static LIBRARY_LOCK: Mutex<()> = Mutex::new(());

/// Proof that the caller holds [`LIBRARY_LOCK`]; [`save`] demands one so a
/// new caller can't forget it. Take it before the `load` of a
/// load-modify-save, not just around the `save`.
pub struct LibraryGuard {
    _guard: MutexGuard<'static, ()>,
}

pub fn lock() -> LibraryGuard {
    // Writes are atomic, so a panic while holding the lock leaves no torn
    // file behind; poisoning carries no information here.
    LibraryGuard {
        _guard: LIBRARY_LOCK.lock().unwrap_or_else(|e| e.into_inner()),
    }
}

/// Load all automations. A missing file is treated as an empty library —
/// not an error — so the user sees an empty UI on first launch.
pub fn load(app_data: PathBuf) -> Result<Vec<Automation>, String> {
    read_json(&storage_path(app_data))
}

/// Persist a new full list. Entries that were on disk but are missing from
/// `list` get a tombstone so the iCloud merge spreads the deletion instead
/// of re-adding the entry from another Mac. Recorded even with sync off:
/// turning sync on later must not revive what was deleted meanwhile.
pub fn save(_lock: &LibraryGuard, app_data: PathBuf, list: &[Automation]) -> Result<(), String> {
    let p = storage_path(app_data.clone());
    // An unreadable old file just yields no tombstones, like before.
    let previous: Vec<Automation> = read_json(&p).unwrap_or_default();
    let removed: Vec<u64> = previous
        .iter()
        .map(|a| a.id)
        .filter(|id| !list.iter().any(|a| a.id == *id))
        .collect();
    if !removed.is_empty() {
        let tp = tombstones_path(app_data);
        let now = crate::unix_millis();
        // A broken tombstone file must not block deleting; losing it can
        // only revive entries, never drop them.
        let mut tombs: Vec<Tombstone> = read_json(&tp).unwrap_or_default();
        tombs.retain(|t| !removed.contains(&t.id) && !t.expired(now));
        tombs.extend(removed.iter().map(|&id| Tombstone {
            id,
            deleted_at: now,
        }));
        write_json(&tp, &tombs)?;
    }
    write_json(&p, list)
}

/// Execute one automation host-side and capture stdout/stderr/exit code.
///
/// Shell-out details:
/// - `AppleScript` → `osascript -e <payload>` (single string mode keeps
///   multi-line scripts working; AppleScript itself handles the newlines).
/// - `Shortcut`    → `shortcuts run "<payload>"`. Requires macOS 12+.
/// - `Shell`       → `sh -c <payload>`.
///
/// We don't impose timeouts here — long-running shortcuts (e.g. a Make
/// Coffee shortcut that pauses) are legitimate. Callers wanting a timeout
/// can wrap us with `tokio::time::timeout` on the IPC side.
pub fn run(a: &Automation) -> RunResult {
    let mut cmd = match a.kind {
        AutomationKind::AppleScript => {
            let mut c = Command::new("osascript");
            c.arg("-e").arg(&a.payload);
            c
        }
        AutomationKind::Shortcut => {
            let mut c = Command::new("shortcuts");
            c.arg("run").arg(&a.payload);
            c
        }
        AutomationKind::Shell => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&a.payload);
            c
        }
    };
    match cmd.output() {
        Ok(out) => {
            let exit_code = out.status.code();
            RunResult {
                exit_code,
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                success: exit_code == Some(0),
            }
        }
        Err(e) => RunResult {
            exit_code: None,
            stdout: String::new(),
            stderr: format!("failed to spawn: {e}"),
            success: false,
        },
    }
}

/// Enumerate the macOS Shortcuts the user has installed. Returns an empty
/// list on non-macOS or when `shortcuts` (the CLI) isn't available
/// (pre-Monterey).
#[cfg(target_os = "macos")]
pub fn list_shortcuts() -> Vec<String> {
    match Command::new("shortcuts").arg("list").output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|s| s.to_owned())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn list_shortcuts() -> Vec<String> {
    Vec::new()
}
