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
use tauri::State;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod now_playing;
use now_playing::NowPlaying;

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
            set_keymap,
            set_fn_keymap,
            get_macros,
            set_macros,
            macro_limits,
            get_custom_led,
            set_custom_led,
            get_now_playing,
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AK820 Pro app");
}
