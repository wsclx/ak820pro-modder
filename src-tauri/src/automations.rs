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

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Compute the persistence path. Caller may have to create parent dirs.
pub fn storage_path(app_data: PathBuf) -> PathBuf {
    app_data.join("automations.json")
}

/// Load all automations. A missing file is treated as an empty library —
/// not an error — so the user sees an empty UI on first launch.
pub fn load(app_data: PathBuf) -> Result<Vec<Automation>, String> {
    let p = storage_path(app_data);
    if !p.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("read {p:?}: {e}"))?;
    let parsed: Vec<Automation> =
        serde_json::from_str(&raw).map_err(|e| format!("parse {p:?}: {e}"))?;
    Ok(parsed)
}

/// Persist a new full list. Atomic-ish: write to a sibling tempfile then
/// rename so a crash mid-write can't corrupt the existing list.
pub fn save(app_data: PathBuf, list: &[Automation]) -> Result<(), String> {
    let p = storage_path(app_data.clone());
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    let body = serde_json::to_string_pretty(list).map_err(|e| format!("serialize: {e}"))?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write {tmp:?}: {e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("rename {tmp:?} → {p:?}: {e}"))?;
    Ok(())
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
