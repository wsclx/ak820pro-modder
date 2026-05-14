use ak820_protocol::{
    commands::keymap::Keymap,
    commands::lighting::{Direction, LightingConfig, Mode},
    commands::macros::{Macro, MACRO_BYTE_LIMIT, MACRO_SLOT_COUNT, MAX_ACTIONS_PER_MACRO},
    commands::per_key_rgb::CustomLedMap,
    commands::system::{DeviceInfoReport, GameMode, SleepPreset, SLEEP_PRESETS},
    device::ProbeReport,
    Connection, DeviceInfo,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod automations;
mod now_playing;
mod presets;
mod starter_library;
use automations::{Automation, RunResult};
use now_playing::NowPlaying;
use presets::Preset;
use starter_library::StarterAutomation;

/// HID codes F13..F24 reserved as global-shortcut markers for automations.
/// Inclusive range — gives the user 12 keyboard-triggerable automations.
const MARKER_HID_RANGE: std::ops::RangeInclusive<u8> = 104..=115;

/// Map an HID Keyboard Usage Code to the string label
/// `tauri-plugin-global-shortcut` expects (and back-channel parses).
fn hid_to_shortcut_label(hid: u8) -> Option<&'static str> {
    match hid {
        104 => Some("F13"),
        105 => Some("F14"),
        106 => Some("F15"),
        107 => Some("F16"),
        108 => Some("F17"),
        109 => Some("F18"),
        110 => Some("F19"),
        111 => Some("F20"),
        112 => Some("F21"),
        113 => Some("F22"),
        114 => Some("F23"),
        115 => Some("F24"),
        _ => None,
    }
}

/// Re-register the global-shortcut handlers from the current on-disk
/// automations list. Called on app startup and after every
/// `save_automations` / `assign_automation_marker` / `unassign_*`.
///
/// Always wipes the existing registration first — a marker can only be
/// bound to one automation at a time, and the user is free to reshuffle.
fn refresh_automation_shortcuts(app: &AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let list = automations::load(dir)?;
    let mgr = app.global_shortcut();
    mgr.unregister_all()
        .map_err(|e| format!("unregister_all: {e}"))?;
    for a in list {
        let Some(marker) = a.marker_hid else { continue };
        let Some(label) = hid_to_shortcut_label(marker) else {
            continue;
        };
        let shortcut: Shortcut = label.parse().map_err(|e| format!("parse {label}: {e}"))?;
        let automation_id = a.id;
        let app_handle = app.clone();
        mgr.on_shortcut(shortcut, move |_app, _sc, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                run_automation_by_id(&app, automation_id).await;
            });
        })
        .map_err(|e| format!("on_shortcut({label}): {e}"))?;
        tracing::info!(automation_id, label, "registered global-shortcut marker");
    }
    Ok(())
}

async fn run_automation_by_id(app: &AppHandle, id: u64) {
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("app_data_dir: {e}");
            return;
        }
    };
    let dir_clone = dir.clone();
    let list = match tokio::task::spawn_blocking(move || automations::load(dir_clone)).await {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => {
            tracing::warn!("load: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!("join: {e}");
            return;
        }
    };
    let Some(automation) = list.into_iter().find(|a| a.id == id) else {
        tracing::warn!("automation {id} no longer exists");
        return;
    };
    let name = automation.name.clone();
    let res = tokio::task::spawn_blocking(move || automations::run(&automation))
        .await
        .unwrap_or_else(|_| automations::RunResult {
            exit_code: None,
            stdout: String::new(),
            stderr: "spawn_blocking failed".into(),
            success: false,
        });
    tracing::info!(
        automation_id = id,
        automation_name = %name,
        success = res.success,
        exit_code = ?res.exit_code,
        "shortcut-triggered automation done"
    );
}

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl From<ak820_protocol::Error> for AppError {
    fn from(value: ak820_protocol::Error) -> Self {
        AppError::Protocol(value.to_string())
    }
}

/// Holds a single open control connection across calls so we don't
/// re-enumerate and re-open the HID device on every UI tick.
///
/// **Why `tokio::sync::Mutex`** (not `std::sync::Mutex`):
/// HID I/O is synchronous and can block for tens of milliseconds per call.
/// With a `std` mutex, an awaiting Tauri command parks its tokio worker
/// thread; under modest concurrency that exhausts the worker pool and the
/// WKWebView freezes (we hit this on the System, Macros, and Keymap views).
/// The tokio mutex suspends only the awaiting *task*, leaving the worker
/// free for unrelated commands (probe polling, frontend rendering callbacks,
/// etc.). All HID-touching commands are now `async fn` and acquire this
/// mutex via `.lock().await`.
#[derive(Default)]
pub struct ConnState(Mutex<Option<Connection>>);

impl ConnState {
    /// Acquire the connection slot, run a synchronous closure that may touch
    /// HID, and release the lock. Wraps `tokio::sync::Mutex::lock().await`
    /// so callers don't repeat the boilerplate. The closure itself stays
    /// sync because the underlying `hidapi` I/O is blocking — that's
    /// acceptable because we hold the mutex for short bursts and the
    /// concurrent runtime can still progress other tasks.
    async fn with<R, F>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&mut Option<Connection>) -> Result<R, AppError>,
    {
        let mut guard = self.0.lock().await;
        f(&mut guard)
    }
}

fn ensure_open(slot: &mut Option<Connection>) -> Result<&Connection, AppError> {
    if slot.is_none() {
        *slot = Some(Connection::open_control()?);
    }
    Ok(slot.as_ref().unwrap())
}

#[tauri::command]
fn list_devices() -> Result<Vec<DeviceInfo>, AppError> {
    Ok(ak820_protocol::enumerate()?)
}

/// Read-only liveness probe. Crucially does NOT touch the cached HID handle
/// in `ConnState` — enumeration is cheap, and locking the mutex from a polling
/// loop would block view-level operations (lighting set / system reads).
#[tauri::command]
fn probe_device() -> Result<ProbeReport, AppError> {
    const CONTROL_USAGE_PAGE: u16 = 0xFF68;
    let candidates = ak820_protocol::enumerate()?;
    Ok(
        match candidates
            .iter()
            .find(|d| d.usage_page == CONTROL_USAGE_PAGE)
        {
            Some(d) => ProbeReport {
                connected: true,
                interface: d.interface,
                product: d.product.clone(),
                firmware_version: None,
            },
            None => ProbeReport {
                connected: false,
                interface: -1,
                product: None,
                firmware_version: None,
            },
        },
    )
}

#[tauri::command]
async fn close_device(state: State<'_, ConnState>) -> Result<(), AppError> {
    state
        .with(|slot| {
            slot.take();
            Ok(())
        })
        .await
}

#[derive(Serialize)]
struct LightingModeInfo {
    name: &'static str,
    supports_direction: bool,
    directions: Vec<&'static str>,
}

#[tauri::command]
fn list_lighting_modes() -> Vec<LightingModeInfo> {
    Mode::ALL
        .iter()
        .map(|m| {
            let dirs = m.supported_directions();
            LightingModeInfo {
                name: m.name(),
                supports_direction: !dirs.is_empty(),
                directions: dirs.iter().map(direction_name).collect(),
            }
        })
        .collect()
}

#[tauri::command]
async fn get_device_info(state: State<'_, ConnState>) -> Result<DeviceInfoReport, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_device_info()?)
        })
        .await
}

#[tauri::command]
async fn get_game_mode(state: State<'_, ConnState>) -> Result<GameMode, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_game_mode()?)
        })
        .await
}

#[tauri::command]
async fn set_game_mode(state: State<'_, ConnState>, mode: GameMode) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_game_mode(&mode)?;
            Ok(())
        })
        .await
}

#[tauri::command]
fn list_sleep_presets() -> Vec<SleepPreset> {
    SLEEP_PRESETS.to_vec()
}

#[tauri::command]
async fn get_keymap(state: State<'_, ConnState>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_keymap()?)
        })
        .await
}

#[tauri::command]
async fn get_fn_keymap(state: State<'_, ConnState>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_fn_keymap()?)
        })
        .await
}

/// Read the firmware's factory-default base-layer keymap without touching
/// the user's current state. The UI uses this to stage a reset that the
/// user reviews before pressing Save.
#[tauri::command]
async fn get_default_keymap(state: State<'_, ConnState>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_default_keymap()?)
        })
        .await
}

#[tauri::command]
async fn get_default_fn_keymap(state: State<'_, ConnState>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_default_fn_keymap()?)
        })
        .await
}

#[tauri::command]
async fn set_keymap(state: State<'_, ConnState>, keymap: Keymap) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_keymap(&keymap)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn set_fn_keymap(state: State<'_, ConnState>, keymap: Keymap) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_fn_keymap(&keymap)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn get_macros(state: State<'_, ConnState>) -> Result<Vec<Macro>, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_macros()?)
        })
        .await
}

#[tauri::command]
async fn set_macros(state: State<'_, ConnState>, macros: Vec<Macro>) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_macros(&macros)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn get_custom_led(state: State<'_, ConnState>) -> Result<CustomLedMap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_custom_led()?)
        })
        .await
}

#[tauri::command]
async fn set_custom_led(state: State<'_, ConnState>, map: CustomLedMap) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_custom_led(&map)?;
            Ok(())
        })
        .await
}

#[derive(Serialize)]
struct MacroLimits {
    slot_count: usize,
    byte_limit: usize,
    max_actions_per_macro: usize,
}

#[tauri::command]
fn macro_limits() -> MacroLimits {
    MacroLimits {
        slot_count: MACRO_SLOT_COUNT,
        byte_limit: MACRO_BYTE_LIMIT,
        max_actions_per_macro: MAX_ACTIONS_PER_MACRO,
    }
}

/// List every saved automation. Returns an empty list on first launch
/// (no file yet) — that's deliberate, not an error.
#[tauri::command]
async fn list_automations(app: tauri::AppHandle) -> Result<Vec<Automation>, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    tokio::task::spawn_blocking(move || automations::load(dir))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
        .map_err(AppError::Protocol)
}

#[tauri::command]
async fn save_automations(app: AppHandle, list: Vec<Automation>) -> Result<(), AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let list_clone = list.clone();
    tokio::task::spawn_blocking(move || automations::save(dir, &list_clone))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
        .map_err(AppError::Protocol)?;
    refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    Ok(())
}

/// Bind an automation to a keyboard marker (one of HID 104..=115 = F13..F24).
/// If the automation already has a marker, returns it unchanged. Otherwise
/// picks the first free marker, persists, and re-registers the global
/// hotkey handlers. The frontend then writes a keyboard slot with the
/// returned HID code so the physical key emits the marker, which the
/// listener catches and runs the automation.
#[tauri::command]
async fn assign_automation_marker(
    app: AppHandle,
    id: u64,
    suggested: Option<u8>,
) -> Result<u8, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let chosen = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || -> Result<u8, String> {
            let mut list = automations::load(dir.clone())?;
            let pos = list
                .iter()
                .position(|a| a.id == id)
                .ok_or_else(|| format!("automation {id} not found"))?;
            if let Some(m) = list[pos].marker_hid {
                return Ok(m);
            }
            let used: std::collections::HashSet<u8> =
                list.iter().filter_map(|a| a.marker_hid).collect();
            let chosen = match suggested {
                Some(s) if MARKER_HID_RANGE.contains(&s) && !used.contains(&s) => s,
                _ => MARKER_HID_RANGE
                    .clone()
                    .find(|c| !used.contains(c))
                    .ok_or_else(|| {
                        "All 12 marker slots (F13–F24) already bound — unassign one first."
                            .to_string()
                    })?,
            };
            list[pos].marker_hid = Some(chosen);
            list[pos].updated_at = unix_millis();
            automations::save(dir, &list)?;
            Ok(chosen)
        })
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
        .map_err(AppError::Protocol)?
    };
    refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    Ok(chosen)
}

/// Clear an automation's marker assignment. Idempotent — no-op if the
/// automation had no marker. After clearing, the global hotkey is
/// unregistered; any keyboard slot that still emits the old marker just
/// types F13/F14/… as a regular HID keystroke.
#[tauri::command]
async fn unassign_automation_marker(app: AppHandle, id: u64) -> Result<(), AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let dir_clone = dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut list = automations::load(dir_clone.clone())?;
        if let Some(pos) = list.iter().position(|a| a.id == id) {
            if list[pos].marker_hid.is_some() {
                list[pos].marker_hid = None;
                list[pos].updated_at = unix_millis();
                automations::save(dir_clone, &list)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join: {e}")))?
    .map_err(AppError::Protocol)?;
    refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    Ok(())
}

fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
async fn run_automation(app: tauri::AppHandle, id: u64) -> Result<RunResult, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let list = tokio::task::spawn_blocking({
        let dir = dir.clone();
        move || automations::load(dir)
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join: {e}")))?
    .map_err(AppError::Protocol)?;
    let automation = list
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::Protocol(format!("automation {id} not found")))?;
    let res = tokio::task::spawn_blocking(move || automations::run(&automation))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?;
    Ok(res)
}

/// Enumerate macOS Shortcuts the user has installed. Empty list on
/// non-macOS or when the `shortcuts` CLI isn't available (pre-Monterey).
#[tauri::command]
async fn list_shortcuts() -> Vec<String> {
    tokio::task::spawn_blocking(automations::list_shortcuts)
        .await
        .unwrap_or_default()
}

/// Curated starter library — 15 ready-to-adopt examples so the
/// Automations tab isn't a blank slate on first launch.
#[tauri::command]
fn get_starter_library() -> Vec<StarterAutomation> {
    starter_library::library()
}

/// Curated cross-cutting presets (lighting + keymap overrides + automation
/// seeds) for common use-cases. Returns every preset with its full payload
/// so the UI can render a preview without an extra round-trip.
#[tauri::command]
fn list_presets() -> Vec<Preset> {
    presets::library()
}

/// Which parts of a preset to apply. Lets the user pick — e.g. take only
/// the lighting from "Gaming FPS" but keep their custom keymap.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApplyPresetOptions {
    pub lighting: bool,
    pub keymap: bool,
    pub fn_keymap: bool,
    pub automations: bool,
}

/// Result of an apply — what actually happened, surfaced to the UI.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ApplyPresetReport {
    pub lighting_applied: bool,
    pub keymap_slots_changed: usize,
    pub fn_keymap_slots_changed: usize,
    pub automations_added: usize,
    pub automations_skipped_existing: usize,
    pub warnings: Vec<String>,
}

#[tauri::command]
async fn apply_preset(
    app: AppHandle,
    state: State<'_, ConnState>,
    id: String,
    options: ApplyPresetOptions,
) -> Result<ApplyPresetReport, AppError> {
    let preset =
        presets::find(&id).ok_or_else(|| AppError::Protocol(format!("preset '{id}' not found")))?;
    let mut report = ApplyPresetReport::default();

    // 1. Lighting — single SET_LED_EFFECT write, optional.
    if options.lighting {
        if let Some(cfg) = preset.lighting.as_ref() {
            let cfg = cfg.clone();
            state
                .with(|slot| {
                    let conn = ensure_open(slot)?;
                    conn.set_lighting(&cfg)?;
                    Ok(())
                })
                .await?;
            report.lighting_applied = true;
        }
    }

    // 2. Base-layer keymap overrides — read-modify-write the existing
    // 128-slot keymap so unchanged slots stay untouched.
    if options.keymap && !preset.keymap_overrides.is_empty() {
        let overrides = preset.keymap_overrides.clone();
        let changed = state
            .with(|slot| {
                let conn = ensure_open(slot)?;
                let mut km = conn.get_keymap()?;
                let mut changed = 0usize;
                for (s, action) in &overrides {
                    let i = *s as usize;
                    if i < km.slots.len() && km.slots[i] != *action {
                        km.slots[i] = action.clone();
                        changed += 1;
                    }
                }
                if changed > 0 {
                    conn.set_keymap(&km)?;
                }
                Ok(changed)
            })
            .await?;
        report.keymap_slots_changed = changed;
    }

    // 3. Fn-layer keymap overrides.
    if options.fn_keymap && !preset.fn_keymap_overrides.is_empty() {
        let overrides = preset.fn_keymap_overrides.clone();
        let changed = state
            .with(|slot| {
                let conn = ensure_open(slot)?;
                let mut km = conn.get_fn_keymap()?;
                let mut changed = 0usize;
                for (s, action) in &overrides {
                    let i = *s as usize;
                    if i < km.slots.len() && km.slots[i] != *action {
                        km.slots[i] = action.clone();
                        changed += 1;
                    }
                }
                if changed > 0 {
                    conn.set_fn_keymap(&km)?;
                }
                Ok(changed)
            })
            .await?;
        report.fn_keymap_slots_changed = changed;
    }

    // 4. Automation seeds — add to the user's library, skip names that
    // are already present (no destructive overwrite).
    if options.automations {
        let seeds = presets::seeds_for(&preset);
        if !seeds.is_empty() {
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
            let dir_clone = dir.clone();
            let added = tokio::task::spawn_blocking(move || -> Result<(usize, usize), String> {
                let mut list = automations::load(dir_clone.clone())?;
                let mut added = 0usize;
                let mut skipped = 0usize;
                for s in &seeds {
                    if list.iter().any(|a| a.name == s.name) {
                        skipped += 1;
                        continue;
                    }
                    let now = unix_millis();
                    list.push(automations::Automation {
                        id: now as u64 + added as u64,
                        name: s.name.into(),
                        description: s.description.into(),
                        kind: s.kind,
                        payload: s.payload.into(),
                        created_at: now,
                        updated_at: now,
                        marker_hid: None,
                    });
                    added += 1;
                }
                automations::save(dir_clone, &list)?;
                Ok((added, skipped))
            })
            .await
            .map_err(|e| AppError::Protocol(format!("join: {e}")))?
            .map_err(AppError::Protocol)?;
            report.automations_added = added.0;
            report.automations_skipped_existing = added.1;
        }
    }

    Ok(report)
}

/// macOS Now-Playing snapshot — covers Music.app and Spotify desktop today.
/// Returns the "nothing playing" sentinel on non-macOS or when nothing is
/// playing, distinguishing both from infrastructure failure (which surfaces
/// as `Err` via the `osascript` exit code).
#[tauri::command]
async fn get_now_playing() -> Result<NowPlaying, AppError> {
    // osascript is a cheap subprocess (~50–200 ms). Run it on tokio's
    // blocking pool so it doesn't tie up an async worker.
    let result = tokio::task::spawn_blocking(now_playing::fetch)
        .await
        .map_err(|e| AppError::Protocol(format!("now-playing join error: {e}")))?;
    Ok(result.unwrap_or_else(NowPlaying::none))
}

/// Drop any cached HID handle so the next call has to re-enumerate and re-open
/// the device. The UI exposes this as the manual "Reconnect" action and we
/// also use it internally when the keyboard goes away (unplug, sleep, etc.).
#[tauri::command]
async fn force_reconnect(state: State<'_, ConnState>) -> Result<(), AppError> {
    state
        .with(|slot| {
            slot.take();
            Ok(())
        })
        .await
}

#[tauri::command]
async fn apply_lighting(
    state: State<'_, ConnState>,
    config: LightingConfig,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            // Retry once if the cached handle has gone stale (e.g. unplug/replug).
            for attempt in 0..2 {
                let conn = ensure_open(slot)?;
                match conn.set_lighting(&config) {
                    Ok(()) => return Ok(()),
                    Err(e) if attempt == 0 => {
                        tracing::warn!(?e, "set_lighting failed, reopening device");
                        slot.take();
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(())
        })
        .await
}

fn direction_name(d: &Direction) -> &'static str {
    match d {
        Direction::Left => "left",
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::Right => "right",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        .manage(ConnState::default())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            list_devices,
            probe_device,
            close_device,
            list_lighting_modes,
            apply_lighting,
            get_device_info,
            get_game_mode,
            set_game_mode,
            list_sleep_presets,
            force_reconnect,
            get_keymap,
            get_fn_keymap,
            get_default_keymap,
            get_default_fn_keymap,
            set_keymap,
            set_fn_keymap,
            get_macros,
            set_macros,
            macro_limits,
            get_custom_led,
            set_custom_led,
            get_now_playing,
            list_automations,
            save_automations,
            run_automation,
            list_shortcuts,
            get_starter_library,
            assign_automation_marker,
            unassign_automation_marker,
            list_presets,
            apply_preset,
        ])
        .setup(|app| {
            use tauri::Manager;
            use tauri::menu::{AboutMetadataBuilder, Menu, MenuItem, PredefinedMenuItem, SubmenuBuilder};

            // About-dialog payload: version, copyright, "by wsclx" attribution,
            // and a homepage link so users discover the GitHub repo.
            let about_meta = AboutMetadataBuilder::new()
                .name(Some("AK820 Pro Modder"))
                .version(Some(env!("CARGO_PKG_VERSION")))
                .copyright(Some("Copyright (c) 2026 wsclx · MIT licensed"))
                .authors(Some(vec!["wsclx".to_string()]))
                .website(Some("https://github.com/wsclx/ak820pro-modder"))
                .website_label(Some("github.com/wsclx/ak820pro-modder"))
                .comments(Some(
                    "Open-source, macOS-first control software for the Epomaker / Ajazz AK820 Pro mechanical keyboard.",
                ))
                .build();

            // Native macOS-style menu with the shortcuts users expect.
            let app_menu = SubmenuBuilder::new(app, "AK820 Pro Modder")
                .item(&PredefinedMenuItem::about(app, Some("About AK820 Pro Modder"), Some(about_meta))?)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None)?)
                .item(&PredefinedMenuItem::hide_others(app, None)?)
                .item(&PredefinedMenuItem::show_all(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::quit(app, None)?)
                .build()?;

            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .item(&PredefinedMenuItem::undo(app, None)?)
                .item(&PredefinedMenuItem::redo(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::cut(app, None)?)
                .item(&PredefinedMenuItem::copy(app, None)?)
                .item(&PredefinedMenuItem::paste(app, None)?)
                .item(&PredefinedMenuItem::select_all(app, None)?)
                .build()?;

            let view_menu = SubmenuBuilder::new(app, "View")
                .item(&MenuItem::with_id(
                    app, "reload", "Reload", true, Some("CmdOrCtrl+R"),
                )?)
                .item(&MenuItem::with_id(
                    app, "force_reload", "Force Reload", true, Some("CmdOrCtrl+Shift+R"),
                )?)
                .separator()
                .item(&MenuItem::with_id(
                    app, "toggle_devtools", "Toggle DevTools", true, Some("CmdOrCtrl+Alt+I"),
                )?)
                .build()?;

            let window_menu = SubmenuBuilder::new(app, "Window")
                .item(&PredefinedMenuItem::minimize(app, None)?)
                .item(&PredefinedMenuItem::maximize(app, None)?)
                .item(&PredefinedMenuItem::fullscreen(app, None)?)
                .build()?;

            let menu = Menu::with_items(app, &[&app_menu, &edit_menu, &view_menu, &window_menu])?;
            app.set_menu(menu)?;

            app.on_menu_event(|app, event| match event.id().as_ref() {
                "reload" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.eval("location.reload()");
                    }
                }
                "force_reload" => {
                    if let Some(win) = app.get_webview_window("main") {
                        // Bypass any HTTP cache the webview may hold.
                        let _ = win.eval("location.reload()");
                    }
                }
                "toggle_devtools" => {
                    if let Some(win) = app.get_webview_window("main") {
                        if win.is_devtools_open() {
                            win.close_devtools();
                        } else {
                            win.open_devtools();
                        }
                    }
                }
                _ => {}
            });

            // DevTools is one keypress away (Cmd+Alt+I) — don't auto-open,
            // because that has previously coincided with WKWebView freezes
            // during initial page load on some macOS versions.

            // Register global hotkeys for every automation that already
            // has a marker assigned. Done after the menu setup so the
            // plugin is fully initialised. Failure is logged but doesn't
            // block launch — the user can still adopt + re-bind.
            if let Err(e) = refresh_automation_shortcuts(app.handle()) {
                tracing::warn!("initial automation shortcut registration failed: {e}");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AK820 Pro app");
}
