//! Feature-report control for FF13/FF68 firmware. Packet fixtures follow
//! gohv/EPOMAKER-Ajazz-AK820-Pro and gusleig/ajazz-ak820-pro-mac; battery
//! query follows Aiacos/ajazz-control-center. See docs/MACOS-800A.md.
//! Unknown key-table/macro formats are never substituted with web commands.

use crate::commands::clock::TftDateTime;
use crate::commands::lighting::{LightingConfig, Mode};
use crate::{Error, Result};
use hidapi::HidDevice;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::time::Duration;

/// Serialize complete feature transactions with the TFT worker, including
/// across CLI and GUI processes. Dropping/terminating a process releases it.
pub(crate) struct TransactionLock {
    _file: File,
}

impl TransactionLock {
    pub(crate) fn acquire(pid: u16) -> Result<Self> {
        let path = std::env::temp_dir().join(format!("ak820-0c45-{pid:04x}.lock"));
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| Error::UnexpectedResponse(format!("open transaction lock: {e}")))?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
            Error::UnexpectedResponse(
                "Another keyboard operation is running; wait for it to finish".into(),
            )
        })?;
        Ok(Self { _file: file })
    }
}

/// Legacy control packets include 04 as data. HID report ID 0 is added by
/// the transport, never substituted for that first payload byte.
pub fn control_packet(command: u8, subcommand: u8, value: u16) -> [u8; 64] {
    let mut packet = [0u8; 64];
    packet[0] = 4;
    packet[1] = command;
    packet[2] = subcommand;
    packet[8..10].copy_from_slice(&value.to_le_bytes());
    packet
}

/// Build the older mode payload. Off uses brightness 0; static uses the
/// breathing engine at speed 0. Neither can use the modern Custom=80 mode.
pub fn lighting_packet(config: &LightingConfig) -> Result<[u8; 64]> {
    if config.secondary.is_some() || config.effect_mode_type != 0 {
        return Err(Error::NotImplemented(
            "Secondary color and effect type are not encoded by feature-report firmware",
        ));
    }
    if config.mode == Mode::Custom {
        return Err(Error::NotImplemented(
            "Per-key RGB format is not verified for feature-report firmware",
        ));
    }
    let (mode, speed, brightness) = match config.mode {
        Mode::Off => (Mode::SingleOn, 0, 0),
        Mode::Static => (Mode::Breath, 0, config.brightness.min(5)),
        mode => (mode, config.speed.min(5), config.brightness.min(5)),
    };
    let mut packet = [0u8; 64];
    let (r, g, b) = config.rgb();
    packet[0] = mode as u8;
    packet[1..4].copy_from_slice(&[r, g, b]);
    packet[8] = u8::from(config.color_mode != 0);
    packet[9] = brightness;
    packet[10] = speed;
    packet[11] = config.direction as u8;
    packet[14..16].copy_from_slice(&[0x55, 0xAA]);
    Ok(packet)
}

/// Four legacy sleep steps; these are minutes, not web-profile enum values.
pub fn sleep_packet(minutes: u8) -> Result<[u8; 64]> {
    let code = match minutes {
        0 => 0,
        1 => 1,
        5 => 2,
        30 => 3,
        _ => {
            return Err(Error::UnexpectedResponse(
                "Sleep minutes must be 0, 1, 5 or 30 on this keyboard".into(),
            ))
        }
    };
    let mut packet = [0u8; 64];
    packet[8] = code;
    packet[62..].copy_from_slice(&[0xAA, 0x55]);
    Ok(packet)
}

/// Encode a validated host-local date and time in binary, not BCD. Same
/// `01 5A YY MM DD hh mm ss` core as family A's cmd 52 payload.
pub fn clock_packet(now: &TftDateTime) -> Result<[u8; 64]> {
    // Range checks shared with family A; byte 0 differs (5A there, 00 here).
    let web = now.encode()?;
    let mut packet = [0u8; 64];
    packet[1..9].copy_from_slice(&web[1..9]);
    packet[10] = 4;
    packet[62..].copy_from_slice(&[0xAA, 0x55]);
    Ok(packet)
}

pub(crate) fn send(device: &HidDevice, packet: &[u8; 64]) -> Result<[u8; 65]> {
    let mut wire = [0u8; 65];
    wire[1..].copy_from_slice(packet);
    device.send_feature_report(&wire)?;
    std::thread::sleep(Duration::from_millis(50));
    let mut response = [0u8; 65];
    let n = device.get_feature_report(&mut response)?;
    if n != response.len() {
        return Err(Error::UnexpectedResponse(
            "Short feature-report handshake".into(),
        ));
    }
    if packet[0] == 4 && response[1..3] != packet[..2] {
        return Err(Error::UnexpectedResponse(format!(
            "Wrong feature handshake for command {:02x}",
            packet[1]
        )));
    }
    Ok(response)
}

pub(crate) fn set_lighting(device: &HidDevice, pid: u16, config: &LightingConfig) -> Result<()> {
    let packet = lighting_packet(config)?;
    let _guard = TransactionLock::acquire(pid)?;
    send(device, &control_packet(0x18, 0, 1))?;
    send(device, &control_packet(0x13, 0, 1))?;
    send(device, &packet)?;
    send(device, &control_packet(0xF0, 0, 1))?;
    Ok(())
}

pub(crate) fn set_sleep(device: &HidDevice, pid: u16, minutes: u8) -> Result<()> {
    let packet = sleep_packet(minutes)?;
    let _guard = TransactionLock::acquire(pid)?;
    send(device, &control_packet(0x18, 0, 1))?;
    send(device, &control_packet(0x17, 1, 1))?;
    send(device, &packet)?;
    Ok(())
}

/// Resolve the host-local date and time without changing system timezone.
/// macOS date provides the effective offset including daylight saving.
pub fn local_now() -> Result<time::OffsetDateTime> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("/bin/date")
            .arg("+%z")
            .output()
            .map_err(|e| Error::UnexpectedResponse(format!("read local UTC offset: {e}")))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.trim();
        if !output.status.success()
            || text.len() != 5
            || !matches!(text.as_bytes()[0], b'+' | b'-')
            || !text.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        {
            return Err(Error::UnexpectedResponse("Invalid host UTC offset".into()));
        }
        let hours: i32 = text[1..3]
            .parse()
            .map_err(|_| Error::UnexpectedResponse("Invalid UTC hours".into()))?;
        let minutes: i32 = text[3..5]
            .parse()
            .map_err(|_| Error::UnexpectedResponse("Invalid UTC minutes".into()))?;
        if minutes > 59 {
            return Err(Error::UnexpectedResponse("Invalid UTC minutes".into()));
        }
        let sign = if text.starts_with('-') { -1 } else { 1 };
        let offset = time::UtcOffset::from_whole_seconds(sign * (hours * 3600 + minutes * 60))
            .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
        Ok(time::OffsetDateTime::now_utc().to_offset(offset))
    }
    #[cfg(not(unix))]
    time::OffsetDateTime::now_local().map_err(|e| Error::UnexpectedResponse(e.to_string()))
}

pub(crate) fn sync_clock(device: &HidDevice, pid: u16, now: &TftDateTime) -> Result<()> {
    let packet = clock_packet(now)?;
    let _guard = TransactionLock::acquire(pid)?;
    send(device, &control_packet(0x18, 0, 1))?;
    send(device, &control_packet(0x28, 0, 1))?;
    send(device, &packet)?;
    send(device, &control_packet(0xF0, 0, 1))?;
    Ok(())
}

/// An absent reading stays absent. FF is the wired/full sentinel, not a
/// measured percentage or proof that the battery is charging.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BatteryStatus {
    pub percent: Option<u8>,
    pub wired_full: bool,
}

fn parse_battery(response: &[u8]) -> BatteryStatus {
    let raw = (response.len() >= 5 && response[1..3] == [0x20, 1]).then(|| response[4]);
    BatteryStatus {
        percent: raw.filter(|&v| v <= 100),
        wired_full: raw == Some(255),
    }
}

pub(crate) fn battery(device: &HidDevice, pid: u16) -> Result<BatteryStatus> {
    let _guard = TransactionLock::acquire(pid)?;
    // This query has no 04 control prefix: report ID 0, then 20 01.
    let mut wire = [0u8; 65];
    wire[1..3].copy_from_slice(&[0x20, 1]);
    device.send_feature_report(&wire)?;
    std::thread::sleep(Duration::from_millis(50));
    let mut response = [0u8; 65];
    let n = device.get_feature_report(&mut response)?;
    Ok(parse_battery(&response[..n]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::lighting::Direction;

    #[test]
    fn static_and_off_match_legacy_fixtures() {
        let mut config = LightingConfig {
            mode: Mode::Static,
            color: "123456".into(),
            secondary: None,
            color_mode: 0,
            effect_mode_type: 0,
            brightness: 3,
            speed: 5,
            direction: Direction::Left,
        };
        let packet = lighting_packet(&config).unwrap();
        assert_eq!(&packet[..4], &[7, 0x12, 0x34, 0x56]);
        assert_eq!(&packet[8..16], &[0, 3, 0, 0, 0, 0, 0x55, 0xaa]);
        config.mode = Mode::Off;
        assert_eq!(&lighting_packet(&config).unwrap()[8..11], &[0, 0, 0]);
        config.mode = Mode::Custom;
        assert!(lighting_packet(&config).is_err());
    }

    #[test]
    fn clock_fixture_is_local_binary_time() {
        let now = time::macros::datetime!(2026-10-20 15:26:37 +05:00);
        let packet = clock_packet(&now.into()).unwrap();
        assert_eq!(&packet[..11], &[0, 1, 0x5a, 26, 10, 20, 15, 26, 37, 0, 4]);
        assert_eq!(&packet[62..], &[0xaa, 0x55]);
        assert!(clock_packet(&time::macros::datetime!(2100-01-01 00:00 UTC).into()).is_err());
    }

    #[test]
    fn sleep_minutes_are_not_web_enum_values() {
        assert_eq!(sleep_packet(30).unwrap()[8], 3);
        assert_eq!(sleep_packet(5).unwrap()[8], 2);
        assert!(sleep_packet(10).is_err());
    }

    #[test]
    fn battery_rejects_echo_and_preserves_wired_sentinel() {
        assert_eq!(
            parse_battery(&[0, 0x20, 1, 0, 255]),
            BatteryStatus {
                percent: None,
                wired_full: true
            }
        );
        assert_eq!(parse_battery(&[0, 0x20, 1, 0, 0]).percent, Some(0));
        assert_eq!(parse_battery(&[0, 0x20, 1, 0, 75]).percent, Some(75));
        assert_eq!(parse_battery(&[0, 4, 0x20, 1, 100]).percent, None);
        assert_eq!(parse_battery(&[]).percent, None);
    }
}
