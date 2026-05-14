//! macOS Now-Playing detection.
//!
//! Pragmatic first cut: shell out to `osascript -l JavaScript` (JXA) and
//! probe **Music.app** + **Spotify** in turn. Both expose a stable
//! `currentTrack` AppleScript dictionary, so we get title / artist /
//! album without touching the `MediaRemote` private framework (which
//! Apple has started restricting in recent macOS releases anyway).
//!
//! What this gives us:
//! - Music.app (system AVPlayer)
//! - Spotify desktop client
//!
//! What this misses (acceptable for v0.6 — upgrade path is MediaRemote):
//! - Browser-tab media (Safari, Chrome, Firefox YouTube/Netflix/Spotify Web)
//! - Apple Podcasts, TV
//! - Any non-AppleScript-aware app
//!
//! Threading: `osascript` is invoked synchronously and finishes in
//! ~50-200 ms. The Tauri command wrapping this in `lib.rs` is `async fn`
//! and runs the call inside the tokio runtime, so it does not block the
//! UI thread.

use serde::{Deserialize, Serialize};

/// Snapshot of the currently playing track, normalised across sources.
///
/// `source` is the string `"Music"`, `"Spotify"`, or `"none"` when nothing is
/// playing. Renderers should treat `"none"` as a hint to dim or hide rather
/// than blank out — the user may briefly pause.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NowPlaying {
    pub source: String,
    pub is_playing: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

impl NowPlaying {
    /// "Nothing playing" sentinel used by the non-macOS stub + on parse error.
    pub fn none() -> Self {
        Self {
            source: "none".to_string(),
            ..Self::default()
        }
    }
}

#[cfg(target_os = "macos")]
const JXA_SCRIPT: &str = r#"
const out = { source: "none", is_playing: false, title: null, artist: null, album: null };

function probe(appName) {
  try {
    const app = Application(appName);
    if (!app.running()) return false;
    const state = String(app.playerState());
    if (state !== "playing") return false;
    const t = app.currentTrack();
    out.source = appName;
    out.is_playing = true;
    out.title = String(t.name());
    out.artist = String(t.artist());
    try { out.album = String(t.album()); } catch (e) {}
    return true;
  } catch (e) {
    return false;
  }
}

// Prefer Music; fall through to Spotify; both can be installed.
probe("Music") || probe("Spotify");

JSON.stringify(out);
"#;

/// Fetch the current Now-Playing snapshot.
///
/// Returns `None` only on infrastructure failure (osascript missing, exit
/// non-zero). A successful call with nothing playing yields
/// `Some(NowPlaying::none())`, distinguishing "we couldn't ask" from "we
/// asked and nothing's playing".
#[cfg(target_os = "macos")]
pub fn fetch() -> Option<NowPlaying> {
    use std::process::Command;
    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", JXA_SCRIPT])
        .output()
        .ok()?;
    if !output.status.success() {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&output.stderr),
            "osascript failed"
        );
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<NowPlaying>(raw.trim()).ok()
}

#[cfg(not(target_os = "macos"))]
pub fn fetch() -> Option<NowPlaying> {
    // Other platforms have their own Now-Playing surfaces (MPRIS on Linux,
    // Windows.Media.Control on Windows). Wire them in when we cross-build.
    None
}
