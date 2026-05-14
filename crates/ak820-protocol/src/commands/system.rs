//! System-level reads (device info) and writes (game-mode struct,
//! which carries sleep timer + several other ergonomics flags).
//!
//! Layouts ported from the official AJAZZ online driver — see
//! `docs/PROTOCOL.md`.

use serde::{Deserialize, Serialize};

/// Parsed `GET_DEVICE_INFO` (48 bytes). Same field set as the JSON export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfoReport {
    pub rom_size: u8,
    pub macro_space_size: u16,
    pub vid: u16,
    pub pid: u16,
    /// e.g. `1.07`
    pub firmware_version: f32,
    pub sensor: u16,
    pub manufacturer_id: u16,
    pub product_id: u16,
    pub work_mode: u8,
    pub battery_level: u8,
    /// 0 = discharging, 1 = charging, etc.
    pub charge_status: u8,
    /// 0-based slot index of the active onboard profile.
    pub current_profile: u8,
    pub axis_info: u16,
    pub tft_max_frames: u16,
    pub gif_max_frames: u16,
    pub led_max_frames: u16,
    pub tft_direction: u8,
    pub rt_precision: u8,
    pub frame_version: u8,
    pub lighting_version: u8,
}

impl DeviceInfoReport {
    pub fn parse(b: &[u8]) -> Self {
        // The slice may be shorter than 48 if the device truncated; pad with 0.
        let g = |i: usize| b.get(i).copied().unwrap_or(0);
        let w = |lo: usize, hi: usize| (g(lo) as u16) | ((g(hi) as u16) << 8);
        // Firmware version encoding from the JS source:
        //   v = ((e[8] & 0x0F) + ((e[8] & 0xF0) >> 4) * 10 + e[9] * 100) / 100
        let v8 = g(8) as u32;
        let v9 = g(9) as u32;
        let version_raw = (v8 & 0x0F) + ((v8 & 0xF0) >> 4) * 10 + v9 * 100;
        let firmware_version = (version_raw as f32) / 100.0;

        Self {
            rom_size: g(0),
            macro_space_size: w(2, 3),
            vid: w(4, 5),
            pid: w(6, 7),
            firmware_version,
            sensor: w(10, 11),
            manufacturer_id: w(12, 13),
            product_id: w(14, 15),
            work_mode: g(16),
            battery_level: g(17),
            charge_status: g(18),
            current_profile: g(19),
            axis_info: w(20, 21),
            tft_max_frames: w(22, 23),
            gif_max_frames: w(24, 25),
            led_max_frames: w(26, 27),
            tft_direction: g(28),
            rt_precision: g(29),
            frame_version: g(30),
            lighting_version: g(31),
        }
    }
}

/// Parsed `GET_GAME_MODE` / `SET_GAME_MODE` (56 bytes).
/// Byte 0 of the wire payload is unused / reserved by the firmware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameMode {
    pub game_mode: u8,
    pub fn_switch: u8,
    /// Idle-time before sleep. The firmware enumerates these as small
    /// integers (0 = never, 1 = 1 min, 2 = 5 min, 3 = 10 min, 4 = 15 min,
    /// 5 = 30 min) — exact mapping confirmed against `mario.json`
    /// (`sleepTime: 5` corresponds to "30 minutes").
    pub sleep_time: u8,
    pub key_delay: u8,
    pub report_rate: u8,
    pub system_mode: u8,
    pub tft_display_time: u8,
    /// Stored on the wire as `dead_zone * 100`. Surface as a float here.
    pub top_dead_zone: f32,
    pub bottom_dead_zone: f32,
    pub stability_mode: u8,
    pub auto_calibration: u8,
    pub single_key_wakeup: u8,
}

impl GameMode {
    pub fn parse(b: &[u8]) -> Self {
        let g = |i: usize| b.get(i).copied().unwrap_or(0);
        Self {
            game_mode: g(1),
            fn_switch: g(2),
            sleep_time: g(3),
            key_delay: g(4),
            report_rate: g(5),
            system_mode: g(6),
            tft_display_time: g(7),
            top_dead_zone: (g(8) as f32) / 100.0,
            bottom_dead_zone: (g(9) as f32) / 100.0,
            stability_mode: g(11),
            auto_calibration: g(14),
            single_key_wakeup: g(15),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut p = vec![0u8; 56];
        p[1] = self.game_mode;
        p[2] = self.fn_switch;
        p[3] = self.sleep_time;
        p[4] = self.key_delay;
        p[5] = self.report_rate;
        p[6] = self.system_mode;
        p[7] = self.tft_display_time;
        p[8] = (self.top_dead_zone * 100.0).clamp(0.0, 255.0) as u8;
        p[9] = (self.bottom_dead_zone * 100.0).clamp(0.0, 255.0) as u8;
        p[11] = self.stability_mode;
        p[14] = self.auto_calibration;
        p[15] = self.single_key_wakeup;
        p
    }
}

/// Sleep-timer preset values the official tool exposes. The wire byte is just
/// an index — these labels are the ones rendered in the AJAZZ UI.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SleepPreset {
    pub value: u8,
    pub label: &'static str,
}

impl SleepPreset {
    pub fn label_for(value: u8) -> &'static str {
        SLEEP_PRESETS
            .iter()
            .find(|p| p.value == value)
            .map(|p| p.label)
            .unwrap_or("unknown")
    }
}

pub const SLEEP_PRESETS: &[SleepPreset] = &[
    SleepPreset {
        value: 0,
        label: "never",
    },
    SleepPreset {
        value: 1,
        label: "1 minute",
    },
    SleepPreset {
        value: 2,
        label: "5 minutes",
    },
    SleepPreset {
        value: 3,
        label: "10 minutes",
    },
    SleepPreset {
        value: 4,
        label: "15 minutes",
    },
    SleepPreset {
        value: 5,
        label: "30 minutes",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_info_parses_known_layout() {
        // Synthetic 48-byte buffer matching the JS layout.
        let mut b = [0u8; 48];
        b[0] = 64; // romSize
        b[2] = 0x00;
        b[3] = 0x02; // macroSpaceSize = 512
        b[4] = 0x45;
        b[5] = 0x0C; // vid = 0x0C45
        b[6] = 0x09;
        b[7] = 0x80; // pid = 0x8009
        b[8] = 0x07;
        b[9] = 0x01; // version: (7) + (0*10) + 1*100 = 107 → 1.07
        b[17] = 100; // batteryLevel
        b[18] = 1; // chargeStatus
        b[19] = 0; // currentProfile

        let info = DeviceInfoReport::parse(&b);
        assert_eq!(info.rom_size, 64);
        assert_eq!(info.macro_space_size, 512);
        assert_eq!(info.vid, 0x0C45);
        assert_eq!(info.pid, 0x8009);
        assert!((info.firmware_version - 1.07).abs() < 0.001);
        assert_eq!(info.battery_level, 100);
        assert_eq!(info.charge_status, 1);
        assert_eq!(info.current_profile, 0);
    }

    #[test]
    fn game_mode_roundtrips() {
        let gm = GameMode {
            game_mode: 0,
            fn_switch: 1,
            sleep_time: 5,
            key_delay: 5,
            report_rate: 0,
            system_mode: 0,
            tft_display_time: 0,
            top_dead_zone: 0.15,
            bottom_dead_zone: 0.25,
            stability_mode: 1,
            auto_calibration: 1,
            single_key_wakeup: 0,
        };
        let bytes = gm.serialize();
        assert_eq!(bytes.len(), 56);
        let parsed = GameMode::parse(&bytes);
        assert_eq!(parsed.game_mode, 0);
        assert_eq!(parsed.fn_switch, 1);
        assert_eq!(parsed.sleep_time, 5);
        assert_eq!(parsed.key_delay, 5);
        assert!((parsed.top_dead_zone - 0.15).abs() < 0.001);
        assert!((parsed.bottom_dead_zone - 0.25).abs() < 0.001);
        assert_eq!(parsed.stability_mode, 1);
    }
}
