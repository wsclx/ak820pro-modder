//! Per-key remapping. Wire format decoded from the official AJAZZ online
//! driver (`Ne`, `Z`, `W`, `ne` functions in `index-CGDyjcPg.js`).
//!
//! The device exposes a flat array of **128 4-byte slots**. The full table is
//! transferred via the multi-packet `GET_KEY` / `SET_KEY` commands with
//! content size 512.
//!
//! Each slot is a tagged 4-byte action:
//! ```text
//! byte 0 = pageType   (action class, see `Page` enum)
//! byte 1 = param1
//! byte 2 = param2
//! byte 3 = param3
//! ```
//! For an ordinary HID keystroke the encoding is `[2, 0, hid_usage, 0]`.

use serde::{Deserialize, Serialize};

pub const TOTAL_SLOTS: usize = 128;
pub const SLOT_BYTES: usize = 4;
pub const KEYMAP_BYTES: usize = TOTAL_SLOTS * SLOT_BYTES;

/// Action page-type. Matches the device's `O` enum exactly. Source: AJAZZ
/// online driver, `layout-default-DElMT--A.js`:
/// ```text
/// O = { DEFAULT:0, MOUSE:1, KEYBOARD:2, CONSUMER_KEY:3, SYSTEM_KEY:4,
///       EXTRA_FUNCTION:5, MACRO:6, CB:7, DKS:8, MT:9, TGL:10, SOCD:11,
///       RS:12, FUNC:13, END:14, MPT:15 }
/// ```
/// **The previous version had this enum wrong** (Macro=4, Cb=5, Mt=11,
/// Socd=12). That's why the first Phase-4 hardware test produced silence
/// when F12 was reassigned to "Macro M1" — we were writing SYSTEM_KEY (4)
/// with system-key value 0, not a macro trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    Default = 0,
    Mouse = 1,
    Keyboard = 2,
    ConsumerKey = 3,
    SystemKey = 4,
    ExtraFunction = 5,
    Macro = 6,
    Cb = 7,
    Dks = 8,
    Mt = 9,
    Tgl = 10,
    Socd = 11,
    Rs = 12,
    Func = 13,
    End = 14,
    Mpt = 15,
}

/// A high-level action assigned to one slot. The encoding/decoding follows
/// `W()` and `Z()` in the online driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyAction {
    /// Slot uses the firmware default mapping (no override).
    Default,
    /// Standard HID keystroke. `usage` is a HID Keyboard Usage Code (e.g. 0x29 = Esc).
    Keyboard { usage: u8 },
    /// Mouse button.
    Mouse { button: u8, value: u8 },
    /// Consumer-control key (volume, media). `value` is a 16-bit usage code.
    ConsumerKey { value: u16 },
    /// Trigger a stored macro by slot id (0..99). `param2`/`param3` carry
    /// firmware-internal trigger flags we haven't fully decoded yet — preserved
    /// verbatim for non-destructive round-trip.
    Macro {
        macro_id: u8,
        param2: u8,
        param3: u8,
    },
    /// Toggle a layer.
    Tgl { value: u8 },
    /// Generic FUNC (24-bit big-endian value).
    Func { value: u32 },
    /// FUNC_V2 (newer namespace, encoded with bit-7 set on byte 0).
    FuncV2 { param1: u16, param2: u16 },
    /// Catch-all for action classes we haven't fully decoded yet — preserved
    /// verbatim so write-back is non-destructive.
    Raw {
        page: u8,
        param1: u8,
        param2: u8,
        param3: u8,
    },
}

impl KeyAction {
    pub fn decode(bytes: [u8; SLOT_BYTES]) -> Self {
        let p = bytes[0];
        let p1 = bytes[1];
        let p2 = bytes[2];
        let p3 = bytes[3];
        if p >= 0x80 {
            // FUNC_V2 with bit-7 set on byte 0.
            let param1 = (((p & 0x7F) as u16) << 8) | p1 as u16;
            let param2 = ((p2 as u16) << 8) | p3 as u16;
            return Self::FuncV2 { param1, param2 };
        }
        match p {
            0 => Self::Default,
            1 => Self::Mouse {
                button: p1,
                value: p2,
            },
            2 => Self::Keyboard { usage: p2 },
            3 => Self::ConsumerKey {
                value: (p1 as u16) | ((p2 as u16) << 8),
            },
            6 => Self::Macro {
                macro_id: p1,
                param2: p2,
                param3: p3,
            },
            10 => Self::Tgl { value: p1 },
            13 => Self::Func {
                value: ((p1 as u32) << 16) | ((p2 as u32) << 8) | p3 as u32,
            },
            other => Self::Raw {
                page: other,
                param1: p1,
                param2: p2,
                param3: p3,
            },
        }
    }

    pub fn encode(&self) -> [u8; SLOT_BYTES] {
        match *self {
            Self::Default => [0, 0, 0, 0],
            Self::Mouse { button, value } => [1, button, value, 0],
            Self::Keyboard { usage } => [2, 0, usage, 0],
            Self::ConsumerKey { value } => [3, (value & 0xFF) as u8, (value >> 8) as u8, 0],
            Self::Macro {
                macro_id,
                param2,
                param3,
            } => [6, macro_id, param2, param3],
            Self::Tgl { value } => [10, value, 0, 0],
            Self::Func { value } => [
                13,
                ((value >> 16) & 0xFF) as u8,
                ((value >> 8) & 0xFF) as u8,
                (value & 0xFF) as u8,
            ],
            Self::FuncV2 { param1, param2 } => [
                0x80 | ((param1 >> 8) & 0x7F) as u8,
                (param1 & 0xFF) as u8,
                ((param2 >> 8) & 0xFF) as u8,
                (param2 & 0xFF) as u8,
            ],
            Self::Raw {
                page,
                param1,
                param2,
                param3,
            } => [page, param1, param2, param3],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keymap {
    pub slots: Vec<KeyAction>,
}

impl Keymap {
    pub fn decode(payload: &[u8]) -> Self {
        let mut slots = Vec::with_capacity(TOTAL_SLOTS);
        for i in 0..TOTAL_SLOTS {
            let base = i * SLOT_BYTES;
            let bytes = if base + SLOT_BYTES <= payload.len() {
                [
                    payload[base],
                    payload[base + 1],
                    payload[base + 2],
                    payload[base + 3],
                ]
            } else {
                [0; SLOT_BYTES]
            };
            slots.push(KeyAction::decode(bytes));
        }
        Self { slots }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; KEYMAP_BYTES];
        for (i, action) in self.slots.iter().enumerate().take(TOTAL_SLOTS) {
            let bytes = action.encode();
            let base = i * SLOT_BYTES;
            out[base..base + SLOT_BYTES].copy_from_slice(&bytes);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_roundtrip() {
        let a = KeyAction::Keyboard { usage: 0x29 };
        assert_eq!(a.encode(), [2, 0, 0x29, 0]);
        assert_eq!(KeyAction::decode([2, 0, 0x29, 0]), a);
    }

    #[test]
    fn func_v2_roundtrip() {
        let a = KeyAction::FuncV2 {
            param1: 0x1234,
            param2: 0x5678,
        };
        let bytes = a.encode();
        assert!(bytes[0] >= 0x80);
        assert_eq!(KeyAction::decode(bytes), a);
    }

    #[test]
    fn consumer_key_little_endian() {
        let a = KeyAction::ConsumerKey { value: 0x00CD };
        assert_eq!(a.encode(), [3, 0xCD, 0, 0]);
    }

    #[test]
    fn macro_action_roundtrip() {
        // Page byte 6 = MACRO (official AJAZZ O enum).
        let a = KeyAction::Macro {
            macro_id: 7,
            param2: 0,
            param3: 0,
        };
        assert_eq!(a.encode(), [6, 7, 0, 0]);
        assert_eq!(KeyAction::decode([6, 7, 0, 0]), a);
        // Preserve unknown flags verbatim. param2/param3 are trigger mode
        // (0/1/2) + repeat count when mode=1; we round-trip whatever the
        // device gave us.
        let b = KeyAction::Macro {
            macro_id: 12,
            param2: 1,
            param3: 5,
        };
        assert_eq!(b.encode(), [6, 12, 1, 5]);
        assert_eq!(KeyAction::decode(b.encode()), b);
    }

    #[test]
    fn keymap_full_roundtrip() {
        let mut slots: Vec<KeyAction> = (0..TOTAL_SLOTS)
            .map(|i| KeyAction::Keyboard {
                usage: (i & 0xFF) as u8,
            })
            .collect();
        slots[100] = KeyAction::Tgl { value: 1 };
        let km = Keymap {
            slots: slots.clone(),
        };
        let bytes = km.encode();
        assert_eq!(bytes.len(), KEYMAP_BYTES);
        assert_eq!(Keymap::decode(&bytes).slots, slots);
    }
}
