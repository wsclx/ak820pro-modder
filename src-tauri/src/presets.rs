//! Curated cross-cutting **Presets** — pre-configured bundles that touch
//! lighting, keymap overrides, and the automations library in one move.
//!
//! A preset is an *additive* recipe by default: applying it patches the
//! current state, it doesn't wipe everything first. That way a user who
//! likes their custom keymap can still pull a Gaming-FPS lighting profile
//! without losing their work. The UI lets the user opt into each
//! component (lighting / keymap / automations) per-preset.
//!
//! Adding new presets: add an entry to `library()`. Keep them safe
//! (no destructive shell commands as automation seeds, no irreversible
//! firmware writes) and keep them opinionated (a preset that does
//! nothing teaches the user nothing).

use ak820_protocol::commands::{
    keymap::KeyAction,
    lighting::{Direction, LightingConfig},
};
use serde::Serialize;

use crate::starter_library; // we may re-use safe starter automations as preset seeds

/// One curated preset bundle.
#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// User-facing grouping in the picker UI.
    pub category: &'static str,
    /// Short emoji or single char shown next to the name. Pure cosmetic.
    pub icon: &'static str,
    pub description: &'static str,

    /// If present, applied via `apply_lighting`.
    pub lighting: Option<LightingConfig>,
    /// Sparse base-layer keymap overrides — `(slot, action)` pairs.
    pub keymap_overrides: Vec<(u8, KeyAction)>,
    /// Sparse Fn-layer keymap overrides.
    pub fn_keymap_overrides: Vec<(u8, KeyAction)>,
    /// Names of `starter_library` entries to add to the user's library on apply.
    /// Lets us reference safe, already-curated automations without duplicating
    /// the payload here.
    pub automation_seeds: Vec<&'static str>,
}

/// Convenience: a single LightingConfig with the common defaults filled in.
fn lighting(mode: &'static str, color: &'static str, brightness: u8) -> LightingConfig {
    LightingConfig {
        mode: parse_mode(mode),
        color: color.into(),
        secondary: None,
        color_mode: 0,
        effect_mode_type: 0,
        brightness,
        speed: 3,
        direction: Direction::Left,
    }
}

fn parse_mode(name: &str) -> ak820_protocol::commands::lighting::Mode {
    ak820_protocol::commands::lighting::Mode::from_name(name)
        .unwrap_or(ak820_protocol::commands::lighting::Mode::Static)
}

/// Curated set. 10 entries today; trivially extendable.
pub fn library() -> Vec<Preset> {
    vec![
        // ===== Gaming =================================================
        Preset {
            id: "gaming-fps",
            name: "Gaming — FPS",
            category: "Gaming",
            icon: "🎯",
            description: "Static red lighting for the focused-fingers vibe. Caps Lock disabled \
                 to avoid accidental triggers during play.",
            lighting: Some(lighting("static", "FF0F0F", 5)),
            keymap_overrides: vec![
                // slot 48 = Caps Lock — neutralise to avoid mid-firefight grief
                (48, KeyAction::Default),
            ],
            fn_keymap_overrides: vec![],
            automation_seeds: vec![],
        },
        Preset {
            id: "gaming-mmo",
            name: "Gaming — MMO",
            category: "Gaming",
            icon: "⚔️",
            description: "Breathing magenta to keep the F-row visible at low light. \
                 Adds a screenshot-to-Desktop automation so you can clip kill cams quickly.",
            lighting: Some(lighting("breath", "FF00FF", 4)),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Screenshot to Desktop"],
        },
        // ===== Dev ====================================================
        Preset {
            id: "dev-linux",
            name: "Dev — Linux Terminal",
            category: "Dev",
            icon: "🐧",
            description:
                "Monochrome white static lighting, low brightness for late-night terminal \
                 sessions. Caps Lock is remapped to Left Ctrl (Emacs-style ergonomics). \
                 Seeds: spawn a fresh Terminal at $HOME.",
            lighting: Some(lighting("static", "FFFFFF", 2)),
            keymap_overrides: vec![
                // Caps Lock slot (48) → Left Ctrl (HID 224)
                (48, KeyAction::Keyboard { usage: 224 }),
            ],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["New Terminal at home"],
        },
        Preset {
            id: "dev-vibe-coder",
            name: "Dev — Vibe Coder",
            category: "Dev",
            icon: "✨",
            description:
                "Rainbow spectrum animation, full brightness. The 'I-pair-program-with-LLMs' \
                 aesthetic. Seeds: open the repo on GitHub for quick navigation.",
            lighting: Some(LightingConfig {
                mode: parse_mode("spectrum"),
                color: "FFFFFF".into(),
                secondary: None,
                color_mode: 1,
                effect_mode_type: 0,
                brightness: 5,
                speed: 4,
                direction: Direction::Right,
            }),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Open this app on GitHub"],
        },
        Preset {
            id: "dev-white-hacker",
            name: "Dev — White Hat",
            category: "Dev",
            icon: "🛡️",
            description: "Matrix-green static lighting, mid brightness. Caps Lock → Esc \
                 for vi-heavy editors. Seeds: copy your local IP address.",
            lighting: Some(lighting("static", "00FF40", 4)),
            keymap_overrides: vec![
                // Caps Lock (48) → Escape (HID 41)
                (48, KeyAction::Keyboard { usage: 41 }),
            ],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Copy local IP address"],
        },
        // ===== Office =================================================
        Preset {
            id: "office-ms365",
            name: "Office — MS365",
            category: "Office",
            icon: "📧",
            description: "Calm static blue lighting for long meeting days. Seeds: dark-mode \
                 toggle and ISO timestamp helper for note-taking.",
            lighting: Some(lighting("static", "0078D4", 3)),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Toggle dark / light mode", "Copy ISO timestamp"],
        },
        // ===== Creative ===============================================
        Preset {
            id: "music-production",
            name: "Music Production",
            category: "Creative",
            icon: "🎹",
            description: "Flowing rainbow gradient, full brightness. Seeds: Music play/pause for \
                 quick reference-track scrubbing while you're in your DAW.",
            lighting: Some(LightingConfig {
                mode: parse_mode("flowing"),
                color: "FFFFFF".into(),
                secondary: None,
                color_mode: 1,
                effect_mode_type: 0,
                brightness: 5,
                speed: 3,
                direction: Direction::Right,
            }),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Music: play / pause"],
        },
        Preset {
            id: "writing-focus",
            name: "Writing — Focus",
            category: "Creative",
            icon: "✍️",
            description: "Minimal lighting (off), zero distractions. Seeds: speak-clipboard \
                 for proof-reading, sleep-display for stepping away.",
            lighting: Some(lighting("off", "000000", 0)),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Speak clipboard", "Sleep display"],
        },
        // ===== Lifestyle ==============================================
        Preset {
            id: "streaming",
            name: "Streaming",
            category: "Lifestyle",
            icon: "🔴",
            description: "Pulsating red to remind you 'on air'. Seeds: timestamped screenshot \
                 (for clip thumbnails) and play/pause for stream BGM.",
            lighting: Some(LightingConfig {
                mode: parse_mode("pulsating"),
                color: "FF0000".into(),
                secondary: None,
                color_mode: 0,
                effect_mode_type: 0,
                brightness: 5,
                speed: 2,
                direction: Direction::Left,
            }),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec!["Screenshot to Desktop", "Spotify: play / pause"],
        },
        Preset {
            id: "travel-battery-saver",
            name: "Travel — Battery saver",
            category: "Lifestyle",
            icon: "🔋",
            description: "Lighting off, lowest brightness anywhere it leaks through. For long \
                 flights / coffee-shop sessions where you want every percent.",
            lighting: Some(lighting("off", "000000", 0)),
            keymap_overrides: vec![],
            fn_keymap_overrides: vec![],
            automation_seeds: vec![],
        },
    ]
}

/// Resolve a preset by id.
pub fn find(id: &str) -> Option<Preset> {
    library().into_iter().find(|p| p.id == id)
}

/// Resolve `automation_seeds` (by name) against the starter library.
/// Skips silently if a name no longer exists (library version drift).
pub fn seeds_for(preset: &Preset) -> Vec<starter_library::StarterAutomation> {
    let lib = starter_library::library();
    preset
        .automation_seeds
        .iter()
        .filter_map(|name| lib.iter().find(|s| &s.name == name).cloned())
        .collect()
}
