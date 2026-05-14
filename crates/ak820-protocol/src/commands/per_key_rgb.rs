//! Per-key RGB lighting via `GET_CUSTOM_LED_DATA` (cmd 20) and
//! `SET_CUSTOM_LED_DATA` (cmd 36). Decoded from the AJAZZ online driver
//! (`index-CGDyjcPg.js`, functions `Ce()` for read and `dt()` for write).
//!
//! Wire format:
//! - 512-byte payload, 128 LEDs × 4 bytes each.
//! - Per-LED layout: `[ledId, red, green, blue]`.
//! - `ledId` mirrors the slot index (0..127); the encoder writes `e[l] = s`
//!   where `s` is the running loop counter, so the index is implicit and
//!   we don't need to send arbitrary `ledId` values.
//!
//! These per-key colours only show up if the **lighting mode** is one that
//! reads from custom LED data (e.g. "Static custom" / "Per-key static" — TBD).
//! The standard 20 lighting modes ignore this buffer.

use serde::{Deserialize, Serialize};

/// 128-LED matrix (one entry per keymap slot).
pub const LED_COUNT: usize = 128;

/// Bytes per LED on the wire.
pub const LED_BYTES: usize = 4;

/// Total payload bytes transferred over `GET_CUSTOM_LED_DATA` /
/// `SET_CUSTOM_LED_DATA` (128 × 4).
pub const CUSTOM_LED_BYTES: usize = LED_COUNT * LED_BYTES;

/// A single LED colour. `ledId` is implicit (= position in the array) on
/// the wire, but we keep it as a struct field for ergonomics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedColor {
    pub led_id: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl LedColor {
    pub const BLACK: Self = Self { led_id: 0, red: 0, green: 0, blue: 0 };

    pub fn rgb(led_id: u8, red: u8, green: u8, blue: u8) -> Self {
        Self { led_id, red, green, blue }
    }
}

/// Snapshot of every LED's colour. Always exactly `LED_COUNT` entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomLedMap {
    pub leds: Vec<LedColor>,
}

impl Default for CustomLedMap {
    fn default() -> Self {
        let leds = (0..LED_COUNT)
            .map(|i| LedColor { led_id: i as u8, ..LedColor::BLACK })
            .collect();
        Self { leds }
    }
}

impl CustomLedMap {
    /// Decode a 512-byte read response into the typed map.
    /// Short payloads are zero-padded (treated as off LEDs).
    pub fn decode(payload: &[u8]) -> Self {
        let mut leds = Vec::with_capacity(LED_COUNT);
        for i in 0..LED_COUNT {
            let base = i * LED_BYTES;
            if base + LED_BYTES <= payload.len() {
                leds.push(LedColor {
                    led_id: payload[base],
                    red: payload[base + 1],
                    green: payload[base + 2],
                    blue: payload[base + 3],
                });
            } else {
                leds.push(LedColor { led_id: i as u8, ..LedColor::BLACK });
            }
        }
        Self { leds }
    }

    /// Encode to the 512-byte payload the firmware expects.
    /// Slot indices are written verbatim (`byte 0 = s`) per the official
    /// encoder's `e[l] = s` line — extra entries past `LED_COUNT` are ignored.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; CUSTOM_LED_BYTES];
        for (i, led) in self.leds.iter().enumerate().take(LED_COUNT) {
            let base = i * LED_BYTES;
            out[base] = i as u8;
            out[base + 1] = led.red;
            out[base + 2] = led.green;
            out[base + 3] = led.blue;
        }
        out
    }

    /// Set a single LED's colour, preserving everything else.
    pub fn set(&mut self, slot: usize, red: u8, green: u8, blue: u8) {
        if let Some(entry) = self.leds.get_mut(slot) {
            entry.led_id = slot as u8;
            entry.red = red;
            entry.green = green;
            entry.blue = blue;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_off() {
        let m = CustomLedMap::default();
        assert_eq!(m.leds.len(), LED_COUNT);
        assert!(m.leds.iter().all(|l| l.red == 0 && l.green == 0 && l.blue == 0));
        // led_id mirrors slot index
        for (i, l) in m.leds.iter().enumerate() {
            assert_eq!(l.led_id as usize, i);
        }
    }

    #[test]
    fn round_trip_known_colours() {
        let mut m = CustomLedMap::default();
        m.set(0, 0xFF, 0, 0);   // slot 0 red
        m.set(12, 0, 0xFF, 0);  // F12 green
        m.set(83, 0, 0, 0xFF);  // spacebar blue
        let encoded = m.encode();
        assert_eq!(encoded.len(), CUSTOM_LED_BYTES);
        // verify the slot-0 layout
        assert_eq!(&encoded[0..4], &[0, 0xFF, 0, 0]);
        // F12 (slot 12) → byte offset 48
        assert_eq!(&encoded[48..52], &[12, 0, 0xFF, 0]);
        // Spacebar (slot 83) → byte offset 332
        assert_eq!(&encoded[332..336], &[83, 0, 0, 0xFF]);

        let decoded = CustomLedMap::decode(&encoded);
        assert_eq!(decoded.leds[0], LedColor::rgb(0, 0xFF, 0, 0));
        assert_eq!(decoded.leds[12], LedColor::rgb(12, 0, 0xFF, 0));
        assert_eq!(decoded.leds[83], LedColor::rgb(83, 0, 0, 0xFF));
        // untouched slots stay black, ledId mirrors index
        assert_eq!(decoded.leds[100], LedColor { led_id: 100, red: 0, green: 0, blue: 0 });
    }

    #[test]
    fn decode_handles_short_payload() {
        // Half the expected length
        let buf = vec![1u8; CUSTOM_LED_BYTES / 2];
        let m = CustomLedMap::decode(&buf);
        assert_eq!(m.leds.len(), LED_COUNT);
        // First half should mostly carry the 1s
        assert_eq!(m.leds[0].red, 1);
        // Second half should be padded to off
        assert_eq!(m.leds[100], LedColor { led_id: 100, ..LedColor::BLACK });
    }
}
