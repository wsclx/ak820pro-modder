//! Macro programming. Phase 4 — decoded from the official AJAZZ online driver
//! (see `docs/PROTOCOL.md` § GET_MACRO / SET_MACRO for the byte layout).
//!
//! Storage layout (firmware view):
//! ```text
//! +----------------+ addr = 0
//! | index page     |   400 bytes = 100 slots × LE u32
//! |                |   each slot = byte offset of that macro's data block
//! |                |   (0 = slot empty)
//! +----------------+ addr = 400
//! | data block 0   |   per-macro: 4-byte header + N × 4-byte actions
//! | data block 1   |
//! |    ...         |
//! +----------------+ addr ≤ macroSpaceSize
//! ```
//!
//! Capacity:
//! - up to 100 macros (`MACRO_SLOT_COUNT`)
//! - per-macro practical max ≈ 320 bytes (`MACRO_BYTE_LIMIT`) ≈ 79 actions
//! - total budget = `deviceInfo.macroSpaceSize` (AK820 Pro firmware 1.07: 3072 B)

use serde::{Deserialize, Serialize};

use crate::error::*;

/// Number of macro slots exposed by the firmware.
pub const MACRO_SLOT_COUNT: usize = 100;

/// Size of the index page in bytes (`MACRO_SLOT_COUNT × 4`).
pub const MACRO_INDEX_BYTES: usize = MACRO_SLOT_COUNT * 4;

/// Address inside macro storage where the data area starts (right after the index).
pub const MACRO_DATA_ADDR: u16 = MACRO_INDEX_BYTES as u16;

/// Bytes per action entry (delay LE u16 + keyCode u8 + flags u8).
pub const ACTION_BYTES: usize = 4;

/// AJAZZ spec hard limit per macro slot (320 B = 4-byte header + 79 × 4 actions).
pub const MACRO_BYTE_LIMIT: usize = 320;

/// Maximum actions per macro consistent with `MACRO_BYTE_LIMIT`.
pub const MAX_ACTIONS_PER_MACRO: usize = (MACRO_BYTE_LIMIT - 4) / ACTION_BYTES;

/// What kind of input the action drives. The wire only distinguishes two
/// families (keyboard-ish vs mouse), even though the AJAZZ recorder UI shows
/// more — consumer keys get folded onto the keyboard family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroActionKind {
    /// HID Keyboard Usage Page (0x07) or Consumer Page (0x0C). On the wire
    /// these share `actionType = 1` and only `key_code` distinguishes them.
    Keyboard,
    /// Mouse button or wheel direction (`actionType = 3` on the wire).
    Mouse,
}

impl MacroActionKind {
    /// Decode the 3-bit `actionType` field from a wire flags byte.
    ///
    /// The AJAZZ recorder uses these values (confirmed against
    /// `layout-default-DElMT--A.js`, function `Qe()`):
    /// - `actionType: 3` for **keyboard** events (`D.value === "keyboard"`)
    /// - `actionType: 1` for **mouse** events (`D.value === "mouse"` etc.)
    ///
    /// The protocol-bundle encoder folds these onto two wire-flag pairs:
    /// - `actionType in {1, 2}` → press `0x90`, release `0x10`
    /// - `actionType == 3`      → press `0xB0`, release `0x30`
    ///
    /// **NB.** Earlier drafts of this match had the mapping inverted — keyboard
    /// macros came out as mouse-button presses (a value-11 macro = HID "H" =
    /// 0b1011 → bits 1+2+8 = left+right+button-4 → right-click context menu
    /// flickering on press). Hardware test on AK820 Pro firmware 1.07
    /// confirmed the table above.
    fn from_wire_action_type(bits: u8) -> Self {
        match bits {
            3 => Self::Keyboard,
            // 1 and 2 are both mouse-family in the recorder (the encoder
            // collapses 2 onto 1 on the wire). Treat anything not 3 as Mouse.
            _ => Self::Mouse,
        }
    }

    fn wire_flags(self, is_press: bool) -> u8 {
        match (self, is_press) {
            // actionType 3 → flags 0x30 (release) / 0xB0 (press) → KEYBOARD
            (Self::Keyboard, true) => 0xB0,
            (Self::Keyboard, false) => 0x30,
            // actionType 1 → flags 0x10 (release) / 0x90 (press) → MOUSE
            (Self::Mouse, true) => 0x90,
            (Self::Mouse, false) => 0x10,
        }
    }
}

/// One event in a macro: press or release of a key/button after a delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroAction {
    /// Milliseconds to wait *before* the next event after this one fires.
    /// Encoded as LE u16, max 65 535 ms.
    pub delay_ms: u16,
    /// HID usage code for the key (or mouse button bitmask).
    pub key_code: u8,
    /// Whether this is a press (`true`) or a release (`false`).
    pub is_press: bool,
    /// Keyboard vs mouse family.
    pub kind: MacroActionKind,
}

/// One macro slot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Macro {
    /// Slot id 0..99. The firmware uses this as the storage key.
    pub macro_id: u8,
    /// Display name. Stored host-side only — the firmware does not persist names.
    #[serde(default)]
    pub name: Option<String>,
    /// Ordered events.
    pub actions: Vec<MacroAction>,
}

impl Macro {
    /// Number of bytes this macro occupies on-device (header + actions).
    /// Empty macros report `0` because they're skipped on write.
    pub fn encoded_len(&self) -> usize {
        if self.actions.is_empty() {
            0
        } else {
            4 + self.actions.len() * ACTION_BYTES
        }
    }
}

/// Encode a list of macros into (index page, packed data area).
///
/// - `index` is always exactly `MACRO_INDEX_BYTES` bytes. Empty slots are zero.
/// - `data` may be empty if every macro has zero actions. Caller is responsible
///   for transmitting it at `addr = MACRO_DATA_ADDR` after the index write.
///
/// Errors:
/// - `OutOfRange` if any macro's id is ≥ `MACRO_SLOT_COUNT`.
/// - `MacroTooLarge` if any single macro exceeds `MACRO_BYTE_LIMIT`.
pub fn encode_macros(macros: &[Macro]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut index = vec![0u8; MACRO_INDEX_BYTES];
    let mut data = Vec::new();
    let mut cursor: u32 = MACRO_DATA_ADDR as u32;

    for m in macros {
        if (m.macro_id as usize) >= MACRO_SLOT_COUNT {
            return Err(Error::OutOfRange {
                field: "macro_id",
                value: m.macro_id as i64,
                max: (MACRO_SLOT_COUNT - 1) as i64,
            });
        }
        if m.actions.is_empty() {
            continue;
        }
        let block_size = m.encoded_len();
        if block_size > MACRO_BYTE_LIMIT {
            return Err(Error::MacroTooLarge {
                macro_id: m.macro_id,
                size: block_size,
                limit: MACRO_BYTE_LIMIT,
            });
        }

        // Header (4 B): doubled action count + 2 reserved.
        let n = m.actions.len();
        let doubled = (n as u16).checked_mul(2).ok_or(Error::MacroTooLarge {
            macro_id: m.macro_id,
            size: block_size,
            limit: MACRO_BYTE_LIMIT,
        })?;
        data.extend_from_slice(&doubled.to_le_bytes());
        data.push(0);
        data.push(0);

        // Actions.
        for act in &m.actions {
            data.extend_from_slice(&act.delay_ms.to_le_bytes());
            data.push(act.key_code);
            data.push(act.kind.wire_flags(act.is_press));
        }

        // Index slot (LE u32 byte offset).
        let i = (m.macro_id as usize) * 4;
        index[i..i + 4].copy_from_slice(&cursor.to_le_bytes());
        cursor = cursor.saturating_add(block_size as u32);
    }

    Ok((index, data))
}

/// Decoded view of one slot's metadata extracted from the index page.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IndexEntry {
    pub macro_id: u8,
    pub addr: u32,
}

pub(crate) fn parse_index(index: &[u8]) -> Vec<IndexEntry> {
    let mut out = Vec::new();
    let slots = (index.len() / 4).min(MACRO_SLOT_COUNT);
    for slot in 0..slots {
        let off = slot * 4;
        let addr = u32::from_le_bytes(index[off..off + 4].try_into().unwrap());
        if addr != 0 {
            out.push(IndexEntry {
                macro_id: slot as u8,
                addr,
            });
        }
    }
    out
}

/// Parse a 4-byte data-block header → (action_count, _reserved).
pub(crate) fn parse_block_header(header: &[u8]) -> usize {
    let raw = u16::from_le_bytes([header[0], header[1]]);
    (raw / 2) as usize
}

/// Parse `count` action entries from a contiguous buffer.
pub(crate) fn parse_actions(buf: &[u8], count: usize) -> Vec<MacroAction> {
    let mut actions = Vec::with_capacity(count);
    for i in 0..count {
        let off = i * ACTION_BYTES;
        if off + ACTION_BYTES > buf.len() {
            break;
        }
        let delay = u16::from_le_bytes([buf[off], buf[off + 1]]);
        let key_code = buf[off + 2];
        let flags = buf[off + 3];
        let is_press = flags & 0x80 != 0;
        let action_type = (flags >> 4) & 0x07;
        actions.push(MacroAction {
            delay_ms: delay,
            key_code,
            is_press,
            kind: MacroActionKind::from_wire_action_type(action_type),
        });
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb(delay: u16, code: u8, press: bool) -> MacroAction {
        MacroAction {
            delay_ms: delay,
            key_code: code,
            is_press: press,
            kind: MacroActionKind::Keyboard,
        }
    }

    #[test]
    fn encode_single_keyboard_macro() {
        let m = Macro {
            macro_id: 0,
            name: Some("hi".into()),
            actions: vec![
                kb(0, 11, true),   // H press, no delay
                kb(20, 11, false), // H release after 20 ms
                kb(0, 12, true),   // I press
                kb(20, 12, false), // I release
            ],
        };
        let (index, data) = encode_macros(&[m]).unwrap();

        // Index slot 0 → addr 400 (LE)
        assert_eq!(&index[0..4], &400u32.to_le_bytes());
        // All other slots empty.
        assert!(index[4..].iter().all(|&b| b == 0));

        // Block header: doubled count = 8 → 0x08 0x00 0x00 0x00
        assert_eq!(&data[0..4], &[0x08, 0x00, 0x00, 0x00]);
        // First action: delay 0, code 11, KEYBOARD press → 0xB0 (actionType 3)
        assert_eq!(&data[4..8], &[0x00, 0x00, 11, 0xB0]);
        // Second: delay 20 (0x14), code 11, KEYBOARD release → 0x30
        assert_eq!(&data[8..12], &[0x14, 0x00, 11, 0x30]);
        assert_eq!(data.len(), 4 + 4 * 4);
    }

    #[test]
    fn encode_skips_empty_macros() {
        let m = Macro {
            macro_id: 5,
            name: None,
            actions: vec![],
        };
        let (index, data) = encode_macros(&[m]).unwrap();
        assert!(index.iter().all(|&b| b == 0));
        assert!(data.is_empty());
    }

    #[test]
    fn round_trip_keyboard_and_mouse() {
        let macros = vec![
            Macro {
                macro_id: 0,
                name: None,
                actions: vec![kb(100, 4, true), kb(50, 4, false)],
            },
            Macro {
                macro_id: 7,
                name: None,
                actions: vec![MacroAction {
                    delay_ms: 0,
                    key_code: 0x01, // left mouse button
                    is_press: true,
                    kind: MacroActionKind::Mouse,
                }],
            },
        ];
        let (index, data) = encode_macros(&macros).unwrap();

        // Slot 0 → 400
        assert_eq!(u32::from_le_bytes(index[0..4].try_into().unwrap()), 400);
        // Slot 7 → 400 + size_of(slot 0) = 400 + (4 + 2 * 4) = 412
        assert_eq!(u32::from_le_bytes(index[28..32].try_into().unwrap()), 412);

        let entries = parse_index(&index);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].macro_id, 0);
        assert_eq!(entries[0].addr, 400);
        assert_eq!(entries[1].macro_id, 7);
        assert_eq!(entries[1].addr, 412);

        // Decode block 0
        let off = (entries[0].addr - MACRO_DATA_ADDR as u32) as usize;
        let header_count = parse_block_header(&data[off..off + 4]);
        assert_eq!(header_count, 2);
        let actions = parse_actions(&data[off + 4..off + 4 + header_count * 4], header_count);
        assert_eq!(actions[0], kb(100, 4, true));
        assert_eq!(actions[1], kb(50, 4, false));

        // Decode block 7
        let off = (entries[1].addr - MACRO_DATA_ADDR as u32) as usize;
        let header_count = parse_block_header(&data[off..off + 4]);
        assert_eq!(header_count, 1);
        let actions = parse_actions(&data[off + 4..off + 4 + header_count * 4], header_count);
        assert_eq!(actions[0].kind, MacroActionKind::Mouse);
        assert!(actions[0].is_press);
        assert_eq!(actions[0].key_code, 0x01);
    }

    #[test]
    fn reject_out_of_range_id() {
        let m = Macro {
            macro_id: 100,
            name: None,
            actions: vec![kb(0, 4, true)],
        };
        let err = encode_macros(&[m]).unwrap_err();
        match err {
            Error::OutOfRange { field, value, max } => {
                assert_eq!(field, "macro_id");
                assert_eq!(value, 100);
                assert_eq!(max, 99);
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn reject_oversized_macro() {
        let actions = (0..MAX_ACTIONS_PER_MACRO + 1)
            .map(|i| kb(0, (i & 0xFF) as u8, true))
            .collect();
        let m = Macro {
            macro_id: 0,
            name: None,
            actions,
        };
        let err = encode_macros(&[m]).unwrap_err();
        assert!(matches!(err, Error::MacroTooLarge { .. }));
    }
}
