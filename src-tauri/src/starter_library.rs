//! Curated starter library for the Automations tab.
//!
//! 15 hand-picked examples covering AppleScript + Shell that **work
//! out of the box** on stock macOS 11+ with no sudo, no external
//! dependencies, no user-created Shortcuts. They're meant to seed the
//! library on first launch so a new user has something to click and
//! adapt rather than staring at an empty list.
//!
//! Each entry is a template — when the user "adopts" one, the
//! frontend instantiates it with a fresh id + timestamps and persists
//! it through the normal `save_automations` path. The backend never
//! mutates the user's library on its own.
//!
//! Adding entries: keep them safe, useful, and self-contained.
//! Avoid anything that needs network, sudo, third-party CLIs, or
//! user configuration. Anything more elaborate belongs in CONTRIBUTING.md
//! as a "recipe" rather than baked into the library here.

use serde::Serialize;

use crate::automations::AutomationKind;

#[derive(Debug, Clone, Serialize)]
pub struct StarterAutomation {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: AutomationKind,
    pub payload: &'static str,
    pub category: &'static str,
}

pub fn library() -> Vec<StarterAutomation> {
    vec![
        // ----- System / Display ----------------------------------------
        StarterAutomation {
            name: "Sleep display",
            description: "Turn the display off without locking — handy for stepping away briefly.",
            kind: AutomationKind::Shell,
            payload: "pmset displaysleepnow",
            category: "System",
        },
        StarterAutomation {
            name: "Toggle dark / light mode",
            description: "Flip macOS Appearance between Dark and Light. Effect is system-wide.",
            kind: AutomationKind::AppleScript,
            payload: "tell application \"System Events\" to tell appearance preferences to set dark mode to not dark mode",
            category: "System",
        },
        StarterAutomation {
            name: "Restart Finder",
            description: "Useful when Finder gets stuck or sidebar items disappear. Reopens any open windows automatically.",
            kind: AutomationKind::Shell,
            payload: "killall Finder",
            category: "System",
        },

        // ----- Files / Folders -----------------------------------------
        StarterAutomation {
            name: "Open Downloads folder",
            description: "Pop the Downloads folder open in Finder. Great as a keyboard shortcut.",
            kind: AutomationKind::AppleScript,
            payload: "tell application \"Finder\" to open folder \"Downloads\" of home",
            category: "Files",
        },
        StarterAutomation {
            name: "Empty Trash",
            description: "Empty the trash. Finder still prompts for confirmation if you have items locked.",
            kind: AutomationKind::AppleScript,
            payload: "tell application \"Finder\" to empty trash",
            category: "Files",
        },
        StarterAutomation {
            name: "Reveal current iCloud Drive in Finder",
            description: "Open ~/Library/Mobile Documents/com~apple~CloudDocs in a new Finder window.",
            kind: AutomationKind::Shell,
            payload: "open \"$HOME/Library/Mobile Documents/com~apple~CloudDocs\"",
            category: "Files",
        },

        // ----- Clipboard / Capture -------------------------------------
        StarterAutomation {
            name: "Copy ISO timestamp",
            description: "Puts the current date+time in ISO-8601 UTC on the clipboard (good for filenames or log entries).",
            kind: AutomationKind::Shell,
            payload: "date -u +\"%Y-%m-%dT%H:%M:%SZ\" | pbcopy",
            category: "Clipboard",
        },
        StarterAutomation {
            name: "Speak clipboard",
            description: "Reads the current clipboard contents aloud through the system voice. Great for proof-reading.",
            kind: AutomationKind::Shell,
            payload: "pbpaste | say",
            category: "Clipboard",
        },
        StarterAutomation {
            name: "Copy local IP address",
            description: "Copies the Wi-Fi (en0) IP address to the clipboard. Edit `en0` → `en1` if you use Ethernet.",
            kind: AutomationKind::Shell,
            payload: "ipconfig getifaddr en0 | pbcopy",
            category: "Clipboard",
        },
        StarterAutomation {
            name: "Screenshot to Desktop",
            description: "Interactive screenshot — same as ⌘⇧4 — saved to your Desktop with a timestamp.",
            kind: AutomationKind::Shell,
            payload: "screencapture -i \"$HOME/Desktop/screenshot-$(date +%Y%m%d-%H%M%S).png\"",
            category: "Clipboard",
        },

        // ----- Media ---------------------------------------------------
        StarterAutomation {
            name: "Music: play / pause",
            description: "Toggles Music.app playback. No effect if Music isn't running.",
            kind: AutomationKind::AppleScript,
            payload: "tell application \"Music\" to playpause",
            category: "Media",
        },
        StarterAutomation {
            name: "Music: next track",
            description: "Skip to the next track in Music.app.",
            kind: AutomationKind::AppleScript,
            payload: "tell application \"Music\" to next track",
            category: "Media",
        },
        StarterAutomation {
            name: "Spotify: play / pause",
            description: "Toggles Spotify playback. No effect if Spotify isn't running.",
            kind: AutomationKind::AppleScript,
            payload: "tell application \"Spotify\" to playpause",
            category: "Media",
        },

        // ----- Web -----------------------------------------------------
        StarterAutomation {
            name: "Open this app on GitHub",
            description: "Pop the AK820 Pro Modder GitHub repo open in your default browser.",
            kind: AutomationKind::Shell,
            payload: "open \"https://github.com/wsclx/ak820pro-modder\"",
            category: "Web",
        },

        // ----- Dev -----------------------------------------------------
        StarterAutomation {
            name: "New Terminal at home",
            description: "Spawn a fresh Terminal window starting in your home directory.",
            kind: AutomationKind::Shell,
            payload: "open -a Terminal \"$HOME\"",
            category: "Dev",
        },
    ]
}
