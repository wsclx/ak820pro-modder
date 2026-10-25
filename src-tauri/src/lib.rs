use ak820_protocol::{
    commands::clock::TftDateTime,
    commands::keymap::Keymap,
    commands::lighting::{Direction, LightingConfig, Mode},
    commands::macros::{Macro, MACRO_BYTE_LIMIT, MACRO_SLOT_COUNT, MAX_ACTIONS_PER_MACRO},
    commands::per_key_rgb::CustomLedMap,
    commands::system::{DeviceInfoReport, GameMode, SleepPreset, SLEEP_PRESETS},
    commands::tft::{TftAnimation, TftUploadReport},
    commands::tft_image::{self, FitMode},
    commands::tft_presets::{self, TftPresetInfo},
    device::ProbeReport,
    Connection, DeviceInfo,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod app_lifecycle;
#[cfg(target_os = "macos")]
mod audio_reactive;
mod automations;
mod host_keyboard;
mod icloud_sync;
mod now_playing;
mod now_playing_tft;
mod presets;
mod starter_library;
mod tft_io;
mod tft_memory;
mod tft_text;
use automations::{Automation, RunResult};
use now_playing::NowPlaying;
use now_playing_tft::NowPlayingTftState;
use presets::Preset;
use starter_library::StarterAutomation;
use std::sync::Arc;
use tft_memory::TftMemory;

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
    let host = host_keyboard::load(&app.path().app_data_dir().map_err(|e| e.to_string())?)?;
    // Builds before the modifier rule saved bare keys; registering "A" would
    // swallow that key system-wide. Register none until the editor, which
    // validates on save, has stored the macros again.
    if let Err(e) = host_keyboard::validate(&host) {
        tracing::warn!("Mac macro shortcuts not registered: {e}");
        return Ok(());
    }
    for m in host.macros {
        if m.shortcut.trim().is_empty() {
            continue;
        }
        let shortcut: Shortcut = m
            .shortcut
            .parse()
            .map_err(|e| format!("Invalid Mac macro shortcut: {e}"))?;
        if mgr.is_registered(shortcut) {
            return Err(format!("Shortcut already used: {}", m.shortcut));
        }
        mgr.on_shortcut(shortcut, move |_, _, event| {
            if event.state() == ShortcutState::Pressed {
                let m = m.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = host_keyboard::play(m, false) {
                        tracing::warn!(%error, "Mac macro failed");
                    }
                });
            }
        })
        .map_err(|e| e.to_string())?;
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

#[tauri::command]
fn host_keyboard_status() -> host_keyboard::Status {
    host_keyboard::status()
}
#[tauri::command]
fn host_macro_stop() {
    host_keyboard::cancel();
}
#[tauri::command]
async fn host_macro_run(app: AppHandle, id: u64) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let m = host_keyboard::load(&dir)?
        .macros
        .into_iter()
        .find(|m| m.id == id)
        .ok_or("Save this macro first")?;
    tokio::task::spawn_blocking(move || host_keyboard::play(m, true))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn host_macros_load(app: AppHandle) -> Result<host_keyboard::Config, String> {
    host_keyboard::load(&app.path().app_data_dir().map_err(|e| e.to_string())?)
}
#[tauri::command]
fn host_macros_save(app: AppHandle, config: host_keyboard::Config) -> Result<(), String> {
    host_keyboard::validate(&config)?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let previous = host_keyboard::load(&dir)?;
    // Validate shortcut syntax and marker conflicts before replacing any registrations.
    let automation_shortcuts: Vec<Shortcut> = automations::load(dir.clone())?
        .iter()
        .filter_map(|a| a.marker_hid.and_then(hid_to_shortcut_label))
        .filter_map(|s| s.parse().ok())
        .collect();
    let mut shortcuts = Vec::new();
    for m in &config.macros {
        if m.shortcut.is_empty() {
            continue;
        }
        let shortcut: Shortcut = m
            .shortcut
            .parse()
            .map_err(|e| format!("Invalid shortcut: {e}"))?;
        if shortcuts.contains(&shortcut) || automation_shortcuts.contains(&shortcut) {
            return Err("Shortcut already used by another macro or automation".into());
        }
        shortcuts.push(shortcut);
    }
    host_keyboard::save(&dir, &config)?;
    if let Err(error) = refresh_automation_shortcuts(&app) {
        host_keyboard::save(&dir, &previous)?;
        let _ = refresh_automation_shortcuts(&app);
        return Err(error);
    }
    Ok(())
}
#[tauri::command]
async fn host_remaps_get() -> Result<Vec<host_keyboard::Remap>, String> {
    tokio::task::spawn_blocking(host_keyboard::get_remaps)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn host_remaps_saved(app: AppHandle) -> Result<Vec<host_keyboard::Remap>, String> {
    host_keyboard::saved_remaps(&app.path().app_data_dir().map_err(|e| e.to_string())?)
}
#[tauri::command]
async fn host_remaps_apply(
    app: AppHandle,
    remaps: Vec<host_keyboard::Remap>,
    restore: bool,
) -> Result<Vec<host_keyboard::Remap>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || host_keyboard::apply_remaps(&dir, remaps, restore))
        .await
        .map_err(|e| e.to_string())?
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
    ///
    /// **Auto-reconnect**: if a *cached* `Connection` returns a stale-handle
    /// error (device unplugged, USB sleep, hidapi handle invalidated), the
    /// slot is cleared and the closure runs once more. The retry re-enters
    /// `ensure_open()`, which re-enumerates and re-opens the HID device —
    /// so the first action after a re-plug succeeds transparently without
    /// the user having to click Refresh twice.
    ///
    /// We require `FnMut` (not `FnOnce`) for this. All current call sites
    /// borrow their inputs (`&keymap`, `&map`, etc.) rather than consuming
    /// them, so the closure can be called twice without lifetime issues.
    async fn with<R, F>(&self, mut f: F) -> Result<R, AppError>
    where
        F: FnMut(&mut Option<Connection>) -> Result<R, AppError>,
    {
        let mut guard = self.0.lock().await;
        let had_cached_conn = guard.is_some();
        match f(&mut guard) {
            Ok(v) => Ok(v),
            Err(err) if had_cached_conn && is_stale_handle_error(&err) => {
                // The cached HID handle is no good. Drop it and retry once
                // with a fresh open — covers unplug/replug and macOS sleep
                // wake-up where hidapi returns "Device is disconnected".
                tracing::info!(?err, "stale HID handle, clearing slot and retrying once");
                *guard = None;
                f(&mut guard)
            }
            Err(err) => Err(err),
        }
    }
}

/// Pattern-match on the stringified error to decide whether the cached HID
/// handle is suspect. Errors raised by `Connection::open_control()` itself
/// (i.e. when the slot was already empty) shouldn't trigger a retry — the
/// caller in [`ConnState::with`] gates this with `had_cached_conn`.
fn is_stale_handle_error(err: &AppError) -> bool {
    let AppError::Protocol(msg) = err;
    // hidapi wraps platform errors with a "HID error: …" prefix, the body
    // typically mentions "disconnected" or "Device not found" depending
    // on which call (read / write / re-enumeration) tripped first.
    msg.contains("disconnected") || msg.contains("Device not found") || msg.contains("HID error")
}

pub(crate) fn ensure_open(slot: &mut Option<Connection>) -> Result<&Connection, AppError> {
    if slot.is_none() {
        *slot = Some(Connection::open_control()?);
    }
    Ok(slot.as_ref().unwrap())
}

#[tauri::command]
fn get_capabilities() -> Result<ak820_protocol::capabilities::DeviceCapabilities, AppError> {
    let candidates = ak820_protocol::enumerate()?;
    let device = ak820_protocol::select_control(&candidates)
        .ok_or_else(|| AppError::Protocol("Keyboard is not connected".into()))?;
    Ok(ak820_protocol::capabilities::DeviceCapabilities::for_protocol(device.protocol))
}

#[derive(Serialize)]
struct LegacyStatus {
    device: DeviceInfo,
    battery: ak820_protocol::legacy::BatteryStatus,
}

#[tauri::command]
async fn get_legacy_status(state: State<'_, Arc<ConnState>>) -> Result<LegacyStatus, AppError> {
    state
        .with(|slot| {
            let connection = ensure_open(slot)?;
            Ok(LegacyStatus {
                device: connection.info().clone(),
                battery: connection.get_legacy_battery()?,
            })
        })
        .await
}

#[tauri::command]
async fn set_legacy_sleep(state: State<'_, Arc<ConnState>>, minutes: u8) -> Result<(), AppError> {
    state
        .with(|slot| {
            ensure_open(slot)?.set_legacy_sleep(minutes)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn sync_legacy_clock(state: State<'_, Arc<ConnState>>) -> Result<(), AppError> {
    let now = tokio::task::spawn_blocking(ak820_protocol::legacy::local_now)
        .await
        .map_err(|e| AppError::Protocol(format!("local time task: {e}")))??;
    state
        .with(|slot| {
            ensure_open(slot)?.set_tft_datetime(&now.into())?;
            Ok(())
        })
        .await
}

#[tauri::command]
fn list_devices() -> Result<Vec<DeviceInfo>, AppError> {
    Ok(ak820_protocol::enumerate()?)
}

/// Read-only liveness probe. Crucially does NOT touch the cached HID handle
/// in `ConnState` — enumeration is cheap, and locking the mutex from a polling
/// loop would block view-level operations (lighting set / system reads).
/// Uses the same endpoint selection as `Connection::open_control`, so
/// "connected" always refers to the interface the next command will open.
#[tauri::command]
fn probe_device() -> Result<ProbeReport, AppError> {
    let candidates = ak820_protocol::enumerate()?;
    Ok(match ak820_protocol::select_control(&candidates) {
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
    })
}

#[tauri::command]
async fn close_device(state: State<'_, Arc<ConnState>>) -> Result<(), AppError> {
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
async fn get_device_info(state: State<'_, Arc<ConnState>>) -> Result<DeviceInfoReport, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_device_info()?)
        })
        .await
}

#[tauri::command]
async fn get_game_mode(state: State<'_, Arc<ConnState>>) -> Result<GameMode, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_game_mode()?)
        })
        .await
}

#[tauri::command]
async fn set_game_mode(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    mode: GameMode,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_game_mode(&mode)?;
            Ok(())
        })
        .await?;
    restore_tft_quietly(memory.inner());
    Ok(())
}

#[tauri::command]
fn list_sleep_presets() -> Vec<SleepPreset> {
    SLEEP_PRESETS.to_vec()
}

#[tauri::command]
async fn get_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_keymap()?)
        })
        .await
}

#[tauri::command]
async fn get_fn_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
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
async fn get_default_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_default_keymap()?)
        })
        .await
}

#[tauri::command]
async fn get_default_fn_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_default_fn_keymap()?)
        })
        .await
}

#[tauri::command]
async fn set_keymap(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    keymap: Keymap,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_keymap(&keymap)?;
            Ok(())
        })
        .await?;
    restore_tft_quietly(memory.inner());
    Ok(())
}

#[tauri::command]
async fn set_fn_keymap(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    keymap: Keymap,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_fn_keymap(&keymap)?;
            Ok(())
        })
        .await?;
    restore_tft_quietly(memory.inner());
    Ok(())
}

#[tauri::command]
async fn get_macros(state: State<'_, Arc<ConnState>>) -> Result<Vec<Macro>, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_macros()?)
        })
        .await
}

#[tauri::command]
async fn set_macros(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    macros: Vec<Macro>,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_macros(&macros)?;
            Ok(())
        })
        .await?;
    restore_tft_quietly(memory.inner());
    Ok(())
}

#[tauri::command]
async fn get_custom_led(state: State<'_, Arc<ConnState>>) -> Result<CustomLedMap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            // Family B has no colour-bank read-back; the editor starts black.
            if conn.info().protocol == ak820_protocol::device::DeviceProtocol::LegacyFeature {
                return Ok(CustomLedMap::default());
            }
            Ok(conn.get_custom_led()?)
        })
        .await
}

#[tauri::command]
async fn set_custom_led(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    map: CustomLedMap,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_custom_led(&map)?;
            Ok(())
        })
        .await?;
    restore_tft_quietly(memory.inner());
    Ok(())
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
    tokio::task::spawn_blocking(move || automations::save(&automations::lock(), dir, &list_clone))
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
            let lock = automations::lock();
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
            automations::save(&lock, dir, &list)?;
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
        let lock = automations::lock();
        let mut list = automations::load(dir_clone.clone())?;
        if let Some(pos) = list.iter().position(|a| a.id == id) {
            if list[pos].marker_hid.is_some() {
                list[pos].marker_hid = None;
                list[pos].updated_at = unix_millis();
                automations::save(&lock, dir_clone, &list)?;
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
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    id: String,
    options: ApplyPresetOptions,
) -> Result<ApplyPresetReport, AppError> {
    let preset =
        presets::find(&id).ok_or_else(|| AppError::Protocol(format!("preset '{id}' not found")))?;
    let keymap = options.keymap && !preset.keymap_overrides.is_empty();
    let fn_keymap = options.fn_keymap && !preset.fn_keymap_overrides.is_empty();
    // Only key overrides need to know the board; automation seeds also apply
    // with no keyboard attached.
    if keymap || fn_keymap {
        let capabilities = get_capabilities()?;
        if (keymap && !capabilities.keymap) || (fn_keymap && !capabilities.fn_keymap) {
            return Err(AppError::Protocol(
                "Onboard key remapping is not decoded for this firmware; no preset changes were sent"
                    .into(),
            ));
        }
    }
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
                let lock = automations::lock();
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
                automations::save(&lock, dir_clone, &list)?;
                Ok((added, skipped))
            })
            .await
            .map_err(|e| AppError::Protocol(format!("join: {e}")))?
            .map_err(AppError::Protocol)?;
            report.automations_added = added.0;
            report.automations_skipped_existing = added.1;
        }
    }

    // Cross-cutting presets potentially touch lighting + keymap +
    // macros, any of which the firmware may treat as TFT-resetting.
    // The restore is cheap when memory is empty.
    restore_tft_quietly(memory.inner());

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
async fn force_reconnect(state: State<'_, Arc<ConnState>>) -> Result<(), AppError> {
    state
        .with(|slot| {
            slot.take();
            Ok(())
        })
        .await
}

#[tauri::command]
async fn apply_lighting(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
    config: LightingConfig,
) -> Result<(), AppError> {
    // Stale-handle retry is now handled generically by `ConnState::with`.
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_lighting(&config)?;
            Ok(())
        })
        .await?;
    // SET_LED_EFFECT is observed (and confirmed against the AJAZZ web
    // driver) to reset the TFT to its factory animation. Re-apply what
    // the user last asked the TFT to show.
    restore_tft_quietly(memory.inner());
    Ok(())
}

/// Schedule a re-apply of the last user-requested TFT content. Free no-op
/// when the memory is empty (the common case for users who never touch
/// the TFT). Debounced and fire-and-forget: the caller's command returns
/// right away and a burst of commands costs one upload. See
/// `tft_memory.rs`.
fn restore_tft_quietly(memory: &Arc<TftMemory>) {
    memory.schedule_restore();
}

fn direction_name(d: &Direction) -> &'static str {
    match d {
        Direction::Left => "left",
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::Right => "right",
    }
}

/* ---------------------------------------------- audio-reactive (Phase 6) --
 *
 * Three commands the Lighting view uses:
 *   audio_reactive_start    - open SCStream, switch firmware to Custom,
 *                             begin streaming Frame→CustomLedMap at 30 fps.
 *                             Returns Err on TCC-denied / no displays /
 *                             permission-revoked-at-runtime cases so the
 *                             toggle can revert and show a useful message.
 *   audio_reactive_stop     - signal the loop, return immediately. The
 *                             loop self-clears its handle slot ≤33 ms later.
 *   audio_reactive_status   - non-blocking is-running probe for the UI's
 *                             periodic poll.
 *
 * On non-macOS hosts (no SCStream) the commands stay registered but each
 * returns `Err("audio-reactive requires macOS 13+")` so the UI gets a
 * clean message rather than a missing-command error.
 */

#[cfg(target_os = "macos")]
#[tauri::command]
async fn audio_reactive_start(
    conn: State<'_, Arc<ConnState>>,
    audio: State<'_, Arc<audio_reactive::AudioReactiveState>>,
) -> Result<(), AppError> {
    let audio = audio.inner().clone();
    let conn = conn.inner().clone();
    if !get_capabilities()?.audio_reactive {
        return Err(AppError::Protocol(
            audio_reactive::STREAMING_UNAVAILABLE.into(),
        ));
    }
    audio.start(conn).await
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn audio_reactive_stop(
    audio: State<'_, Arc<audio_reactive::AudioReactiveState>>,
) -> Result<(), AppError> {
    audio.stop().await;
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn audio_reactive_status(
    audio: State<'_, Arc<audio_reactive::AudioReactiveState>>,
) -> Result<bool, AppError> {
    Ok(audio.is_running().await)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn audio_reactive_start() -> Result<(), AppError> {
    Err(AppError::Protocol(
        "audio-reactive requires macOS 13+ (ScreenCaptureKit not available)".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn audio_reactive_stop() -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn audio_reactive_status() -> Result<bool, AppError> {
    Ok(false)
}

/* --------------------------------------------------- iCloud Sync (Phase 6) --
 *
 * The backend merges the automations library with iCloud Drive per entry
 * (see `icloud_sync.rs`). The frontend keeps the "enabled" flag (in
 * localStorage) and decides *when* to sync: on app start (pull), after
 * every save (push), or via "Sync now" in the SyncCard.
 *
 * Three commands:
 *   icloud_sync_status   - cheap probe: is iCloud Drive present, does the
 *                           remote file exist, which conflicts are open?
 *   icloud_sync_push     - merge local and iCloud, write both sides
 *   icloud_sync_pull     - the same merge; optionally settles one conflict
 *                           (`resolve: { id, keep: "local" | "remote" }`)
 *
 * Push and pull are the same operation since the merge is two-way; the
 * names stay because the call sites mean "after a save" and "on start /
 * on demand". Both return a `SyncReport` (changes + open conflicts).
 */

#[tauri::command]
async fn icloud_sync_status(app: AppHandle) -> Result<icloud_sync::SyncStatus, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    // Spawn-blocking because the probe reads files and we don't want to
    // stall the tokio worker even briefly on a slow disk.
    tokio::task::spawn_blocking(move || icloud_sync::status(&dir))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))
}

#[tauri::command]
async fn icloud_sync_push(app: AppHandle) -> Result<icloud_sync::SyncReport, AppError> {
    icloud_sync_pull(app, None).await
}

#[tauri::command]
async fn icloud_sync_pull(
    app: AppHandle,
    resolve: Option<icloud_sync::Resolution>,
) -> Result<icloud_sync::SyncReport, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let report = tokio::task::spawn_blocking(move || icloud_sync::sync(&dir, resolve))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))??;
    // Entries from iCloud may carry other marker bindings, so re-register
    // the global shortcuts. Same call as `save_automations` runs.
    if report.local_changed {
        refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    }
    Ok(report)
}

/* ------------------------------------------------- TFT presets (Phase 5c) --
 *
 * Commands the TFT view uses:
 *   list_tft_presets    - returns the static catalogue (id, name, desc,
 *                         frame count, total ms). No device touched.
 *   apply_tft_preset    - builds the named animation and uploads it.
 *   apply_tft_image     - decodes a PNG / JPEG / GIF and uploads it.
 *   tft_factory_default - back to the firmware's built-in animation.
 *   tft_sync_clock      - set the RTC behind the stock clock page.
 *
 * Uploads go through `tft_io`, which serialises all TFT traffic and
 * opens a dedicated handle on the 0xFF67 interface per upload; that
 * handle is separate from the `ConnState` control connection (0xFF68).
 */

/// Async because the first call builds every preset to measure it
/// (~130 ms release); sync commands would run that on the main thread.
#[tauri::command]
async fn list_tft_presets() -> Result<Vec<TftPresetInfo>, AppError> {
    tokio::task::spawn_blocking(tft_presets::catalogue)
        .await
        .map_err(|e| AppError::Protocol(format!("join presets: {e}")))
}

/// Restore the TFT to its firmware-default animation: built-in animation
/// index 0 via `SET_TFT_BUILT_IN_INDEX` on the **control** interface,
/// like the CLI's `tft select-index` (it used to go to the TFT interface,
/// which drops control frames). Holds the TFT lock so it can't land in
/// the middle of an upload.
///
/// Also clears the TFT memory: the user has explicitly asked for the
/// default, so a later Lighting change mustn't drag the old content back.
#[tauri::command]
async fn tft_factory_default(
    state: State<'_, Arc<ConnState>>,
    memory: State<'_, Arc<TftMemory>>,
) -> Result<(), AppError> {
    memory.forget().await;
    let _tft = tft_io::lock().await;
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_tft_built_in_index(0)?;
            Ok(())
        })
        .await
}

/// Set the keyboard's RTC (shown on the stock TFT clock page) to the
/// host's local time.
#[tauri::command]
async fn tft_sync_clock(state: State<'_, Arc<ConnState>>) -> Result<TftDateTime, AppError> {
    let now = TftDateTime::now_local()?;
    let _tft = tft_io::lock().await;
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_tft_datetime(&now)?;
            Ok(())
        })
        .await?;
    Ok(now)
}

/// Upload a user-supplied image (PNG, JPEG, or GIF) to the TFT.
/// `fit` is one of `"fill"` / `"contain"` / `"stretch"`. File read and
/// decode run on the blocking pool; a multi-frame GIF resize is real work.
/// `allow_legacy` is the user's confirmation to overwrite a family-B
/// board's stored animation.
#[tauri::command]
async fn apply_tft_image(
    memory: State<'_, Arc<TftMemory>>,
    path: String,
    fit: String,
    allow_legacy: bool,
) -> Result<TftUploadReport, AppError> {
    let fit_mode = FitMode::parse_lenient(&fit);
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let anim = tokio::task::spawn_blocking(move || -> Result<TftAnimation, AppError> {
        let bytes =
            std::fs::read(&path).map_err(|e| AppError::Protocol(format!("read image: {e}")))?;
        Ok(tft_image::animation_from_bytes(&bytes, fit_mode)?)
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join decode: {e}")))??;

    let anim = Arc::new(anim);
    let report = tft_io::upload(anim.clone(), allow_legacy).await?;
    // A confirmed family-B upload is a flash write; never repeat it as a
    // side effect (see `tft_memory`).
    if !allow_legacy {
        memory.remember(format!("image {name}"), anim).await;
    }
    Ok(report)
}

#[tauri::command]
async fn apply_tft_preset(
    memory: State<'_, Arc<TftMemory>>,
    id: String,
    allow_legacy: bool,
) -> Result<TftUploadReport, AppError> {
    // Build off-thread: the animated presets allocate and compute a few
    // hundred KB of pixels.
    let id_for_build = id.clone();
    let anim = tokio::task::spawn_blocking(move || tft_presets::build(&id_for_build))
        .await
        .map_err(|e| AppError::Protocol(format!("join build: {e}")))?
        .ok_or_else(|| AppError::Protocol(format!("unknown TFT preset id: {id}")))?;

    let anim = Arc::new(anim);
    let report = tft_io::upload(anim.clone(), allow_legacy).await?;
    if !allow_legacy {
        memory.remember(format!("preset {id}"), anim).await;
    }
    Ok(report)
}

/// Explicit user-facing "forget what TFT content was last applied".
/// Use this when the user has bound the TFT to live system info (Phase
/// 5e) and doesn't want a stale preset re-uploaded under their feet.
#[tauri::command]
async fn tft_forget_memory(memory: State<'_, Arc<TftMemory>>) -> Result<(), AppError> {
    memory.forget().await;
    Ok(())
}

/* --------------------------------------------- Now-Playing TFT (Phase 5e) --
 *
 * Three commands the TFT view uses to bind the panel to live Now-Playing
 * info. On macOS this means Music.app + Spotify desktop via osascript;
 * on other platforms `now_playing::fetch` returns None and the panel
 * shows "Nothing playing" indefinitely (the start/stop commands still
 * work cross-platform so the UI doesn't need conditional imports).
 */

#[tauri::command]
async fn now_playing_tft_start(
    state: State<'_, Arc<NowPlayingTftState>>,
    memory: State<'_, Arc<TftMemory>>,
) -> Result<(), AppError> {
    let state = state.inner().clone();
    let memory = memory.inner().clone();
    if !get_capabilities()?.now_playing_tft {
        return Err(AppError::Protocol("Live TFT refresh is unavailable on this firmware: every image upload rewrites saved screen content".into()));
    }
    state.start(memory).await
}

#[tauri::command]
async fn now_playing_tft_stop(state: State<'_, Arc<NowPlayingTftState>>) -> Result<(), AppError> {
    state.stop().await;
    Ok(())
}

#[tauri::command]
async fn now_playing_tft_status(
    state: State<'_, Arc<NowPlayingTftState>>,
) -> Result<bool, AppError> {
    Ok(state.is_running().await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let context = tauri::generate_context!();
    let app_name = context
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "AK820 Pro Modder".into());
    let builder = tauri::Builder::default()
        // Register first so a second launch exits before shortcuts/HID state start.
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !app_lifecycle::is_background_launch(args) {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || app_lifecycle::show(&handle));
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .app_name(app_name)
                .build(),
        )
        .manage(Arc::new(app_lifecycle::Runtime::default()))
        .manage(Arc::new(ConnState::default()))
        .manage(Arc::new(TftMemory::default()))
        .manage(Arc::new(NowPlayingTftState::default()))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init());

    // Audio-reactive state is macOS-only; on Linux/Windows we still
    // expose the three Tauri commands (they stub out with "requires
    // macOS 13+") so the frontend can render its disabled state
    // without conditionally importing.
    #[cfg(target_os = "macos")]
    let builder = builder.manage(Arc::new(audio_reactive::AudioReactiveState::default()));

    builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            app_lifecycle::app_lifecycle_status,
            app_lifecycle::set_start_at_login,
            app_lifecycle::set_keep_running_on_close,
            app_lifecycle::hide_to_menu_bar,
            app_lifecycle::quit_keyboard_app,
            app_lifecycle::take_app_settings_request,
            host_keyboard_status,
            host_macro_stop,
            host_macro_run,
            host_macros_load,
            host_macros_save,
            host_remaps_get,
            host_remaps_saved,
            host_remaps_apply,
            list_devices,
            get_capabilities,
            get_legacy_status,
            set_legacy_sleep,
            sync_legacy_clock,
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
            audio_reactive_start,
            audio_reactive_stop,
            audio_reactive_status,
            icloud_sync_status,
            icloud_sync_push,
            icloud_sync_pull,
            list_tft_presets,
            apply_tft_preset,
            apply_tft_image,
            tft_factory_default,
            tft_forget_memory,
            tft_sync_clock,
            now_playing_tft_start,
            now_playing_tft_stop,
            now_playing_tft_status,
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
                .item(&MenuItem::with_id(app, "app_settings", "App settings…", true, Some("CmdOrCtrl+,"))?)
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
                "app_settings" => app_lifecycle::request_settings(app),
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
            if let Err(e) = app_lifecycle::init(app.handle()) {
                tracing::warn!("Menu bar initialization failed: {e}");
            }
            Ok(())
        })
        .build(context)
        .expect("error while building AK820 Pro app")
        .run(|app, event| match event {
            tauri::RunEvent::Ready => app_lifecycle::startup(app),
            tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. }
                if label == "main" => app_lifecycle::handle_close(app, &api),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => app_lifecycle::show(app),
            _ => {}
        });
}
