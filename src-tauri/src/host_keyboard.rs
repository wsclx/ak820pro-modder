//! Explicit macOS fallback for legacy boards. Never writes keyboard firmware.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
const PAGE: u64 = 7 << 32;
static REMAP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Remap {
    pub source: u16,
    pub destination: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub code: String,
    pub down: bool,
    pub delay_ms: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostMacro {
    pub id: u64,
    pub name: String,
    pub shortcut: String,
    pub steps: Vec<Step>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub macros: Vec<HostMacro>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct NativeMapping {
    #[serde(
        rename = "HIDKeyboardModifierMappingSrc",
        deserialize_with = "mapping_number"
    )]
    source: u64,
    #[serde(
        rename = "HIDKeyboardModifierMappingDst",
        deserialize_with = "mapping_number"
    )]
    destination: u64,
}
fn mapping_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid mapping number")),
        serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("invalid mapping number")),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemapBackup {
    baseline: Vec<NativeMapping>,
    applied: Vec<NativeMapping>,
}
#[derive(Serialize)]
pub struct Status {
    pub accessibility: bool,
    pub running: bool,
}

pub fn status() -> Status {
    Status {
        accessibility: native::trusted(),
        running: RUNNING.load(Ordering::SeqCst),
    }
}
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}
pub fn load(dir: &Path) -> Result<Config, String> {
    let path = dir.join("host-macros.json");
    if !path.exists() {
        return Ok(Config::default());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
pub fn save(dir: &Path, config: &Config) -> Result<(), String> {
    validate(config)?;
    write_json(dir, "host-macros.json", config)
}
fn write_json(dir: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let temp = dir.join(format!("{name}.tmp"));
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temp, dir.join(name)).map_err(|e| e.to_string())
}
pub fn validate(config: &Config) -> Result<(), String> {
    if config.macros.len() > 32 {
        return Err("At most 32 Mac macros".into());
    }
    let mut ids = BTreeSet::new();
    let mut shortcuts = BTreeSet::new();
    for m in &config.macros {
        if !ids.insert(m.id) || m.name.trim().is_empty() || m.name.len() > 120 {
            return Err("Macro names and IDs must be valid and unique".into());
        }
        if !m.shortcut.is_empty() {
            let shortcut: tauri_plugin_global_shortcut::Shortcut = m
                .shortcut
                .parse()
                .map_err(|e| format!("Invalid shortcut: {e}"))?;
            if !shortcut.mods.intersects(
                tauri_plugin_global_shortcut::Modifiers::CONTROL
                    | tauri_plugin_global_shortcut::Modifiers::ALT
                    | tauri_plugin_global_shortcut::Modifiers::SUPER,
            ) {
                return Err("Mac macro shortcuts need Control, Option or Command; ordinary typing keys cannot be global triggers".into());
            }
        }
        if !m.shortcut.is_empty() && !shortcuts.insert(m.shortcut.to_lowercase()) {
            return Err("Duplicate macro shortcut".into());
        }
        validate_steps(&m.steps)?;
    }
    Ok(())
}
fn validate_steps(steps: &[Step]) -> Result<(), String> {
    if steps.is_empty()
        || steps.len() > 256
        || steps.iter().map(|s| u64::from(s.delay_ms)).sum::<u64>() > 60_000
    {
        return Err("Use 1–256 key events and at most 60 seconds".into());
    }
    let mut held = BTreeSet::new();
    for s in steps {
        if virtual_key(&s.code).is_none() || s.delay_ms > 10_000 {
            return Err(format!("Unsupported key or delay: {}", s.code));
        }
        if s.down {
            if !held.insert(&s.code) {
                return Err("Repeated key-down; record without auto-repeat".into());
            }
        } else if !held.remove(&s.code) {
            return Err("Key-up without matching key-down".into());
        }
    }
    if !held.is_empty() {
        return Err("Release all recorded keys before saving".into());
    }
    Ok(())
}

pub fn get_remaps() -> Result<Vec<Remap>, String> {
    native::read_map()?
        .into_iter()
        .map(|m| {
            if m.source >> 32 != 7 || m.destination >> 32 != 7 {
                return Err(
                    "Existing mappings use a different usage page; leave them unchanged".into(),
                );
            }
            Ok(Remap {
                source: (m.source & 0xffff) as u16,
                destination: (m.destination & 0xffff) as u16,
            })
        })
        .collect()
}
pub fn saved_remaps(dir: &Path) -> Result<Vec<Remap>, String> {
    let bytes =
        std::fs::read(dir.join("host-remap-backup.json")).map_err(|_| "No saved Mac mappings")?;
    let backup: RemapBackup = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(backup
        .applied
        .into_iter()
        .map(|m| Remap {
            source: (m.source & 0xffff) as u16,
            destination: (m.destination & 0xffff) as u16,
        })
        .collect())
}
pub fn apply_remaps(dir: &Path, remaps: Vec<Remap>, restore: bool) -> Result<Vec<Remap>, String> {
    let _lock = REMAP_LOCK
        .lock()
        .map_err(|_| "Mac mapping lock unavailable")?;
    let mut sources = BTreeSet::new();
    if remaps.len() > 128
        || remaps.iter().any(|r| {
            !(4..=231).contains(&r.source)
                || !(4..=231).contains(&r.destination)
                || !sources.insert(r.source)
        })
    {
        return Err("Use unique keyboard usages from 4 to 231".into());
    }
    let current = native::read_map()?;
    let path = dir.join("host-remap-backup.json");
    let old: Option<RemapBackup> = if path.exists() {
        Some(
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    if let Some(old) = &old {
        // Empty is also allowed after a device re-plug (macOS loses hidutil mappings).
        if current != old.applied && !current.is_empty() {
            return Err("Mappings were changed by another app. Leave them unchanged and review the conflict first".into());
        }
    }
    if restore {
        let old = old.ok_or("No mappings applied by this app to undo")?;
        native::write_map(&old.baseline)?;
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    } else {
        let applied = remaps
            .iter()
            .map(|r| NativeMapping {
                source: PAGE | u64::from(r.source),
                destination: PAGE | u64::from(r.destination),
            })
            .collect();
        let backup = RemapBackup {
            baseline: old
                .as_ref()
                .map(|b| b.baseline.clone())
                .unwrap_or_else(|| current.clone()),
            applied,
        };
        // Save the undo record BEFORE touching macOS.
        write_json(dir, "host-remap-backup.json", &backup)?;
        if let Err(error) = native::write_map(&backup.applied) {
            // A denied/no-op write must not leave a misleading ownership record.
            if native::read_map().ok().as_ref() == Some(&current) {
                if let Some(old) = old {
                    let _ = write_json(dir, "host-remap-backup.json", &old);
                } else {
                    let _ = std::fs::remove_file(path);
                }
            }
            return Err(error);
        }
    }
    get_remaps()
}

pub fn play(m: HostMacro, countdown: bool) -> Result<(), String> {
    validate_steps(&m.steps)?;
    if !native::trusted() {
        return Err("Enable Accessibility for AK820 Pro Modder in System Settings → Privacy & Security, then try again".into());
    }
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Another Mac macro is running".into());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _guard = Guard;
    CANCEL.store(false, Ordering::SeqCst);
    let mut held = BTreeSet::new();
    let result = (|| {
        if countdown {
            wait(3000)?;
        } else {
            wait(300)?;
        }
        for s in m.steps {
            wait(s.delay_ms)?;
            if s.down {
                held.insert(s.code.clone());
            } else {
                held.remove(&s.code);
            }
            native::post(
                virtual_key(&s.code).ok_or("Unknown key")?,
                s.down,
                flags(&held),
            )?;
        }
        Ok(())
    })();
    // Always release keys injected by us, including on cancellation/error.
    for code in held.into_iter().rev() {
        if let Some(key) = virtual_key(&code) {
            let _ = native::post(key, false, 0);
        }
    }
    result
}
fn wait(ms: u32) -> Result<(), String> {
    let mut left = ms;
    loop {
        if CANCEL.load(Ordering::SeqCst) {
            return Err("Macro stopped".into());
        }
        if left == 0 {
            return Ok(());
        }
        let tick = left.min(20);
        std::thread::sleep(std::time::Duration::from_millis(u64::from(tick)));
        left -= tick;
    }
}
fn flags(held: &BTreeSet<String>) -> u64 {
    let mut flags = 0;
    for code in held {
        flags |= match code.as_str() {
            "ShiftLeft" | "ShiftRight" => 1 << 17,
            "ControlLeft" | "ControlRight" => 1 << 18,
            "AltLeft" | "AltRight" => 1 << 19,
            "MetaLeft" | "MetaRight" => 1 << 20,
            _ => 0,
        };
    }
    flags
}
fn virtual_key(code: &str) -> Option<u16> {
    // Apple Events.h virtual key codes; independent of the current input language.
    Some(match code {
        "KeyA" => 0,
        "KeyS" => 1,
        "KeyD" => 2,
        "KeyF" => 3,
        "KeyH" => 4,
        "KeyG" => 5,
        "KeyZ" => 6,
        "KeyX" => 7,
        "KeyC" => 8,
        "KeyV" => 9,
        "KeyB" => 11,
        "KeyQ" => 12,
        "KeyW" => 13,
        "KeyE" => 14,
        "KeyR" => 15,
        "KeyY" => 16,
        "KeyT" => 17,
        "Digit1" => 18,
        "Digit2" => 19,
        "Digit3" => 20,
        "Digit4" => 21,
        "Digit6" => 22,
        "Digit5" => 23,
        "Equal" => 24,
        "Digit9" => 25,
        "Digit7" => 26,
        "Minus" => 27,
        "Digit8" => 28,
        "Digit0" => 29,
        "BracketRight" => 30,
        "KeyO" => 31,
        "KeyU" => 32,
        "BracketLeft" => 33,
        "KeyI" => 34,
        "KeyP" => 35,
        "Enter" => 36,
        "KeyL" => 37,
        "KeyJ" => 38,
        "Quote" => 39,
        "KeyK" => 40,
        "Semicolon" => 41,
        "Backslash" => 42,
        "Comma" => 43,
        "Slash" => 44,
        "KeyN" => 45,
        "KeyM" => 46,
        "Period" => 47,
        "Tab" => 48,
        "Space" => 49,
        "Backquote" => 50,
        "Backspace" => 51,
        "Escape" => 53,
        "MetaRight" => 54,
        "MetaLeft" => 55,
        "ShiftLeft" => 56,
        "AltLeft" => 58,
        "ControlLeft" => 59,
        "ShiftRight" => 60,
        "AltRight" => 61,
        "ControlRight" => 62,
        "IntlBackslash" => 10,
        "F1" => 122,
        "F2" => 120,
        "F3" => 99,
        "F4" => 118,
        "F5" => 96,
        "F6" => 97,
        "F7" => 98,
        "F8" => 100,
        "F9" => 101,
        "F10" => 109,
        "F11" => 103,
        "F12" => 111,
        "Home" => 115,
        "PageUp" => 116,
        "Delete" => 117,
        "End" => 119,
        "PageDown" => 121,
        "ArrowLeft" => 123,
        "ArrowRight" => 124,
        "ArrowDown" => 125,
        "ArrowUp" => 126,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
mod native {
    use super::NativeMapping;
    use std::{
        ffi::c_void,
        io::Write,
        process::{Command, Stdio},
    };
    const MATCH: &str =
        r#"{"VendorID":3141,"ProductID":32778,"PrimaryUsagePage":1,"PrimaryUsage":6}"#;
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn CGPreflightPostEventAccess() -> bool;
        fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool)
            -> *const c_void;
        fn CGEventSetFlags(event: *const c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *const c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: *const c_void);
    }
    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() && CGPreflightPostEventAccess() }
    }
    pub fn post(key: u16, down: bool, flags: u64) -> Result<(), String> {
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), key, down);
            if event.is_null() {
                return Err("macOS could not create the keyboard event".into());
            }
            CGEventSetFlags(event, flags);
            CGEventPost(1, event);
            CFRelease(event);
        }
        Ok(())
    }
    pub fn read_map() -> Result<Vec<NativeMapping>, String> {
        let out = Command::new("/usr/bin/hidutil")
            .args(["property", "--matching", MATCH, "--get", "UserKeyMapping"])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into());
        }
        parse_map(&String::from_utf8_lossy(&out.stdout))
    }
    pub(super) fn parse_map(output: &str) -> Result<Vec<NativeMapping>, String> {
        let mut lines = output
            .lines()
            .filter(|line| line.contains("UserKeyMapping"));
        let first = lines
            .next()
            .ok_or("AJAZZ USB keyboard not found. Connect it by USB first")?;
        if lines.next().is_some() {
            return Err("Multiple matching keyboard services; refusing an ambiguous remap".into());
        }
        let start = output.find(first).ok_or("Missing keyboard property")?
            + first.find("UserKeyMapping").ok_or("Missing property")?
            + "UserKeyMapping".len();
        let value = output[start..].trim();
        if value == "(null)" {
            return Ok(Vec::new());
        }
        let mut child = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child
            .stdin
            .take()
            .ok_or("Missing plist input")?
            .write_all(value.as_bytes())
            .map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("Could not safely read existing macOS mappings; no changes made".into());
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("Invalid macOS mapping: {e}"))
    }
    pub fn write_map(map: &[NativeMapping]) -> Result<(), String> {
        let json = serde_json::json!({"UserKeyMapping":map}).to_string();
        let out = Command::new("/usr/bin/hidutil")
            .args(["property", "--matching", MATCH, "--set", &json])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() || read_map()? != map {
            return Err(format!(
                "macOS did not apply the mapping: {} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }
}
#[cfg(not(target_os = "macos"))]
mod native {
    use super::NativeMapping;
    pub fn trusted() -> bool {
        false
    }
    pub fn post(_: u16, _: bool, _: u64) -> Result<(), String> {
        Err("Mac macros require macOS".into())
    }
    pub fn read_map() -> Result<Vec<NativeMapping>, String> {
        Err("Mac remapping requires macOS".into())
    }
    pub fn write_map(_: &[NativeMapping]) -> Result<(), String> {
        Err("Mac remapping requires macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn step(code: &str, down: bool) -> Step {
        Step {
            code: code.into(),
            down,
            delay_ms: 10,
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires connected USB AK820 Pro; temporary A and F24 identity mappings, then restores baseline"]
    fn usb_remap_round_trip_and_undo() {
        let dir =
            std::env::temp_dir().join(format!("ak820-host-remap-test-{}", std::process::id()));
        let before = get_remaps().unwrap();
        let mut remaps = before.clone();
        for source in [4, 115] {
            if !remaps.iter().any(|r| r.source == source) {
                remaps.push(Remap {
                    source,
                    destination: source,
                });
            }
        }
        let applied = apply_remaps(&dir, remaps.clone(), false);
        if applied.is_ok() {
            let restored = apply_remaps(&dir, vec![], true).unwrap();
            assert_eq!(restored, before);
        }
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(applied.unwrap(), remaps);
    }
    #[test]
    fn macro_validation_rejects_stuck_keys_unknown_keys_and_long_runs() {
        assert!(validate_steps(&[step("MetaLeft", true)]).is_err());
        assert!(validate_steps(&[step("Unknown", true)]).is_err());
        assert!(validate_steps(&[step("KeyA", false)]).is_err());
        assert!(
            validate_steps(&[step("KeyA", true), step("KeyA", true), step("KeyA", false)]).is_err()
        );
        assert!(validate_steps(&[
            step("MetaLeft", true),
            step("KeyA", true),
            step("KeyA", false),
            step("MetaLeft", false)
        ])
        .is_ok());
    }
    #[test]
    fn global_triggers_cannot_capture_ordinary_typing() {
        let mut config = Config {
            macros: vec![HostMacro {
                id: 1,
                name: "Test".into(),
                shortcut: "A".into(),
                steps: vec![step("KeyA", true), step("KeyA", false)],
            }],
        };
        assert!(validate(&config).is_err());
        config.macros[0].shortcut = "Shift+A".into();
        assert!(validate(&config).is_err());
        config.macros[0].shortcut = "Control+Alt+M".into();
        assert!(validate(&config).is_ok());
    }
    #[test]
    fn modifiers_are_released_and_letters_match_apple_keycodes() {
        assert_eq!(virtual_key("KeyA"), Some(0));
        assert_eq!(virtual_key("KeyZ"), Some(6));
        assert_eq!(
            flags(&BTreeSet::from(["MetaLeft".into(), "ShiftRight".into()])),
            (1 << 20) | (1 << 17)
        );
        assert_eq!(flags(&BTreeSet::new()), 0);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn parses_hidutil_without_accepting_multiple_services() {
        assert!(
            native::parse_map("RegistryID Key Value\n123 UserKeyMapping (null)\n")
                .unwrap()
                .is_empty()
        );
        let map = native::parse_map("RegistryID Key Value\n123 UserKeyMapping (\n{ HIDKeyboardModifierMappingDst = 30064771077; HIDKeyboardModifierMappingSrc = 30064771076; }\n)\n").unwrap();
        assert_eq!(
            map[0],
            NativeMapping {
                source: PAGE | 4,
                destination: PAGE | 5
            }
        );
        assert!(native::parse_map("123 UserKeyMapping (null)\n456 UserKeyMapping (null)").is_err());
        assert!(native::parse_map("RegistryID Key Value\n").is_err());
    }
}
