//! Window lifetime and per-user login launch preferences, independent of the keyboard.
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub keep_running_on_close: bool,
}
#[derive(Default)]
pub struct Runtime {
    preferences: Mutex<Preferences>,
    tray_ready: AtomicBool,
    settings_requested: AtomicBool,
    foreground_requested: AtomicBool,
    login_gate: Mutex<()>,
    tray: Mutex<Option<tauri::tray::TrayIcon>>,
}
#[derive(Serialize)]
pub struct Status {
    pub keep_running_on_close: bool,
    pub start_at_login: Option<bool>,
    pub login_error: Option<String>,
    pub menu_bar_ready: bool,
}
#[tauri::command]
pub async fn app_lifecycle_status(app: AppHandle) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Runtime>>();
        let keep_running_on_close = state
            .preferences
            .lock()
            .map_err(|e| e.to_string())?
            .keep_running_on_close;
        let _login_guard = state.login_gate.lock().map_err(|e| e.to_string())?;
        let login = app.autolaunch().is_enabled();
        Ok(Status {
            keep_running_on_close,
            start_at_login: login.as_ref().ok().copied(),
            login_error: login.err().map(|e| e.to_string()),
            menu_bar_ready: state.tray_ready.load(Ordering::SeqCst),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_start_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Runtime>>();
        let _guard = state.login_gate.lock().map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        if enabled
            && !std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .contains(".app/Contents/MacOS/")
        {
            return Err("Open an installed app bundle before enabling start at login".into());
        }
        if enabled {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        }
        .map_err(|e| e.to_string())?;
        if app.autolaunch().is_enabled().map_err(|e| e.to_string())? != enabled {
            return Err("The login registration did not match the requested setting".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_keep_running_on_close(app: AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Runtime>>();
        let mut prefs = state.preferences.lock().map_err(|e| e.to_string())?;
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        persist_preferences(&dir, &mut prefs, enabled)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// Commit the in-memory preference only after replacing its file. A failed
// write leaves the currently running policy intact.
fn persist_preferences(dir: &Path, current: &mut Preferences, enabled: bool) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let next = Preferences {
        keep_running_on_close: enabled,
    };
    let tmp = dir.join("app-preferences.json.tmp");
    let result = (|| {
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join("app-preferences.json")).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result?;
    *current = next;
    Ok(())
}

// Persist a pending navigation until React is listening, including during startup.
#[tauri::command]
pub fn take_app_settings_request(app: AppHandle) -> bool {
    app.state::<Arc<Runtime>>()
        .settings_requested
        .swap(false, Ordering::SeqCst)
}

pub fn request_settings(app: &AppHandle) {
    app.state::<Arc<Runtime>>()
        .settings_requested
        .store(true, Ordering::SeqCst);
    show(app);
    let _ = app.emit("open-app-settings", ());
}

pub fn show(app: &AppHandle) {
    app.state::<Arc<Runtime>>()
        .foreground_requested
        .store(true, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
        tracing::warn!("show app policy: {e}");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
fn hide(app: &AppHandle) -> Result<(), String> {
    if !app
        .state::<Arc<Runtime>>()
        .tray_ready
        .load(Ordering::SeqCst)
    {
        return Err("The menu bar icon is unavailable; the window will remain open".into());
    }
    // Keep Dock reopening available after an interactive launch, even if macOS
    // crowds a status item behind the display notch or other menu bar items.
    app.tray_by_id("ak820-menu-bar")
        .ok_or("The menu bar icon is unavailable")?
        .set_visible(true)
        .map_err(|e| e.to_string())?;
    app.get_webview_window("main")
        .ok_or("The main window is unavailable")?
        .hide()
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn hide_to_menu_bar(app: AppHandle) -> Result<(), String> {
    hide(&app)
}
#[tauri::command]
pub fn quit_keyboard_app(app: AppHandle) {
    app.exit(0);
}

// A monochrome 2× keyboard glyph. Template rendering follows the menu bar theme.
fn tray_icon() -> tauri::image::Image<'static> {
    let mut rgba = vec![0u8; 44 * 44 * 4];
    for y in 0..44 {
        for x in 0..44 {
            let outline = (4..40).contains(&x)
                && (10..34).contains(&y)
                && (!(6..38).contains(&x) || !(12..32).contains(&y));
            let keys = [14, 20].iter().any(|r| (*r..*r + 3).contains(&y))
                && [9, 15, 21, 27, 33]
                    .iter()
                    .any(|c| (*c..*c + 3).contains(&x));
            let space = (13..31).contains(&x) && (26..29).contains(&y);
            if outline || keys || space {
                rgba[(y * 44 + x) * 4 + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 44, 44)
}
pub fn init(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<Arc<Runtime>>();
    let dir = app.path().app_data_dir()?;
    match std::fs::read(dir.join("app-preferences.json")) {
        Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
            Ok(prefs) => *state.preferences.lock().unwrap() = prefs,
            Err(e) => tracing::warn!("Could not read app preferences; using defaults: {e}"),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("Could not read app preferences; using defaults: {e}"),
    }
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(
                app,
                "tray_show",
                "Show AK820 Pro Modder",
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(app, "tray_settings", "App settings…", true, None::<&str>)?,
            &MenuItem::with_id(app, "tray_hide", "Hide window", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "tray_quit",
                "Quit AK820 Pro Modder",
                true,
                None::<&str>,
            )?,
        ],
    )?;
    let tray = tauri::tray::TrayIconBuilder::with_id("ak820-menu-bar")
        .icon(tray_icon())
        .icon_as_template(true)
        .tooltip("AK820 Pro Modder")
        .title("AK820")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray_show" => show(app),
            "tray_settings" => request_settings(app),
            "tray_hide" => {
                if let Err(e) = hide(app) {
                    tracing::warn!("hide window: {e}");
                }
            }
            "tray_quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    #[cfg(target_os = "macos")]
    if let Err(e) = configure_menu_bar_position(&tray) {
        tracing::warn!("Could not restore menu bar position: {e}");
    }
    *state.tray.lock().unwrap() = Some(tray);
    state.tray_ready.store(true, Ordering::SeqCst);
    Ok(())
}
pub fn startup(app: &AppHandle) {
    if is_background_launch(std::env::args())
        && !app
            .state::<Arc<Runtime>>()
            .foreground_requested
            .load(Ordering::SeqCst)
    {
        #[cfg(target_os = "macos")]
        if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
            tracing::warn!("background launch policy: {e}");
        }
        if let Err(e) = hide(app) {
            tracing::warn!("Background launch: {e}");
            show(app);
        }
    } else {
        show(app);
    }
}
#[derive(Debug, PartialEq, Eq)]
enum CloseAction {
    Hide,
    KeepOpen,
    Quit,
}
fn close_action(keep_running: bool, tray_ready: bool) -> CloseAction {
    if !keep_running {
        CloseAction::Quit
    } else if tray_ready {
        CloseAction::Hide
    } else {
        CloseAction::KeepOpen
    }
}
pub fn handle_close(app: &AppHandle, api: &tauri::CloseRequestApi) {
    let state = app.state::<Arc<Runtime>>();
    let keep_running = state
        .preferences
        .lock()
        .map(|p| p.keep_running_on_close)
        .unwrap_or(false);
    match close_action(keep_running, state.tray_ready.load(Ordering::SeqCst)) {
        CloseAction::Hide => {
            api.prevent_close();
            if let Err(e) = hide(app) {
                tracing::warn!("Could not keep window in menu bar: {e}");
            }
        }
        CloseAction::KeepOpen => {
            api.prevent_close();
            tracing::warn!("Keeping window visible because the menu bar is unavailable");
        }
        CloseAction::Quit => app.exit(0),
    }
}
#[cfg(target_os = "macos")]
fn configure_menu_bar_position(tray: &tauri::tray::TrayIcon) -> tauri::Result<()> {
    tray.with_inner_tray_icon(|tray| {
        use objc2_foundation::{NSString, NSUserDefaults};
        let name = NSString::from_str("AK820ProModder");
        let key = NSString::from_str("NSStatusItem Preferred Position AK820ProModder");
        let defaults = NSUserDefaults::standardUserDefaults();
        // Seed only our first placement, at the right edge. Thereafter AppKit
        // restores the user's Command-dragged position using this stable name.
        // Without a seed a crowded notched display can hide a new item entirely.
        if defaults.objectForKey(&key).is_none() {
            defaults.setInteger_forKey(0, &key);
        }
        if let Some(item) = tray.ns_status_item() {
            item.setAutosaveName(Some(&name));
            item.setVisible(true);
        }
    })
}

pub fn is_background_launch(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().skip(1).any(|arg| arg == "--background")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ak820-lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn closing_cannot_hide_the_only_way_back_into_the_app() {
        assert_eq!(close_action(true, true), CloseAction::Hide);
        assert_eq!(close_action(true, false), CloseAction::KeepOpen);
        assert_eq!(close_action(false, true), CloseAction::Quit);
        assert_eq!(close_action(false, false), CloseAction::Quit);
    }
    #[test]
    fn background_running_is_opt_in_and_existing_preferences_survive() {
        assert!(!Preferences::default().keep_running_on_close);
        assert!(
            !serde_json::from_str::<Preferences>("{}")
                .unwrap()
                .keep_running_on_close
        );
        assert!(
            serde_json::from_str::<Preferences>(r#"{"keep_running_on_close":true}"#)
                .unwrap()
                .keep_running_on_close
        );
        assert!(serde_json::from_str::<Preferences>(r#"{"start_at_login":true}"#).is_err());
    }
    #[test]
    fn preference_survives_reload_and_failed_replacement_preserves_runtime() {
        let dir = temp_dir();
        let mut prefs = Preferences::default();
        persist_preferences(&dir, &mut prefs, true).unwrap();
        let loaded: Preferences =
            serde_json::from_slice(&std::fs::read(dir.join("app-preferences.json")).unwrap())
                .unwrap();
        assert!(loaded.keep_running_on_close && prefs.keep_running_on_close);
        std::fs::remove_file(dir.join("app-preferences.json")).unwrap();
        // Simulate a destination that cannot be atomically replaced.
        std::fs::create_dir(dir.join("app-preferences.json")).unwrap();
        assert!(persist_preferences(&dir, &mut prefs, false).is_err());
        assert!(prefs.keep_running_on_close);
        assert!(!dir.join("app-preferences.json.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn only_explicit_background_argument_keeps_launch_quiet() {
        let background = |args: &[&str]| is_background_launch(args.iter().map(|s| s.to_string()));
        assert!(!background(&["app"]));
        assert!(background(&["app", "--background"]));
        assert!(!background(&["app", "--background=false"]));
        assert!(!background(&["--background"]));
    }
}
