//! Command modules. Each module owns one decoded feature family.
//!
//! Phase 0: scaffolds only.
//! Phase 1: lighting.
//! Phase 2: sleep, clock, battery, profiles.
//! Phase 3: keymap.
//! Phase 4: macros.
//! Phase 5: tft.

pub mod clock;
pub mod keymap;
pub mod lighting;
pub mod macros;
pub mod per_key_rgb;
pub mod sleep;
pub mod system;
pub mod tft;

pub use keymap::{KeyAction, Keymap, Page as KeyPage, KEYMAP_BYTES};
pub use macros::{
    Macro, MacroAction, MacroActionKind, MACRO_BYTE_LIMIT, MACRO_SLOT_COUNT,
    MAX_ACTIONS_PER_MACRO,
};
pub use per_key_rgb::{CustomLedMap, LedColor, CUSTOM_LED_BYTES, LED_COUNT};
