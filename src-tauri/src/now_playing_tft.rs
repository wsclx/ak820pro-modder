//! Now-Playing TFT preset (Phase 5e part 1).
//!
//! Polls macOS's Music.app + Spotify ~every 2 s, rasterises the current
//! track/artist/source onto a 128 × 128 RGB565 frame via
//! [`crate::tft_text`], and uploads to the TFT panel. Skips the upload
//! when nothing has changed since the last tick so the firmware's
//! upload pipeline isn't constantly busy.
//!
//! Lifecycle mirrors [`crate::audio_reactive::AudioReactiveState`]: one
//! background task at a time, started/stopped via Tauri commands. The
//! task self-clears its slot when it exits (capture error, panic, …)
//! so the UI's periodic status poll converges to "stopped" without a
//! dedicated cleanup path.
//!
//! ## Interaction with `TftMemory`
//!
//! When Now-Playing is active, it owns the panel. We call
//! `memory.forget()` on start so that side-effecting commands
//! (`apply_lighting`, `set_keymap`, …) don't re-apply a stale preset
//! between the firmware's TFT-reset and our next poll tick. The reset
//! still flashes briefly, but within 2 s the panel is back to the
//! current track.

use ak820_protocol::commands::tft::{TftAnimation, TftFrame};
use ak820_protocol::Connection;
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_5X8, FONT_6X10, FONT_8X13_BOLD},
        MonoTextStyle,
    },
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor},
    text::{Baseline, Text},
    Drawable,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::now_playing::{self, NowPlaying};
use crate::tft_memory::TftMemory;
use crate::tft_text::{fit_glyphs, Framebuffer128};
use crate::AppError;

/// How often to refresh. 2 s reads as "responsive enough" for track
/// changes (typical track length 3-5 minutes) while keeping the
/// osascript subprocess + HID upload cost negligible.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

struct Inner {
    stop_tx: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub struct NowPlayingTftState(Mutex<Option<Inner>>);

impl NowPlayingTftState {
    pub async fn is_running(&self) -> bool {
        self.0.lock().await.is_some()
    }

    /// Start the background polling task. Silent no-op if one is
    /// already running so a double-click in the UI doesn't error.
    pub async fn start(self: &Arc<Self>, memory: Arc<TftMemory>) -> Result<(), AppError> {
        let mut guard = self.0.lock().await;
        if guard.is_some() {
            tracing::debug!("now_playing_tft: already running, ignoring start");
            return Ok(());
        }

        // Clear any stored preset so side-effecting commands don't
        // restore the old content under our feet. The poll loop owns
        // the panel from here on.
        memory.forget().await;

        let (stop_tx, stop_rx) = watch::channel(false);
        let state = self.clone();
        let handle = tokio::spawn(async move {
            run_loop(stop_rx).await;
            *state.0.lock().await = None;
            tracing::info!("now_playing_tft: loop exited, state cleared");
        });

        *guard = Some(Inner { stop_tx, handle });
        tracing::info!("now_playing_tft: started");
        Ok(())
    }

    pub async fn stop(&self) {
        let Some(inner) = self.0.lock().await.take() else {
            return;
        };
        let _ = inner.stop_tx.send(true);
        inner.handle.abort();
        tracing::info!("now_playing_tft: stop requested");
    }
}

/// Stable signature for "should we re-upload?" — strips out fields
/// that don't materially change the rendered frame (timestamps,
/// position, etc., which `NowPlaying` doesn't carry today but might
/// later).
fn signature(np: &NowPlaying) -> (String, bool, Option<String>, Option<String>) {
    (
        np.source.clone(),
        np.is_playing,
        np.title.clone(),
        np.artist.clone(),
    )
}

async fn run_loop(mut stop_rx: watch::Receiver<bool>) {
    let mut last_sig: Option<(String, bool, Option<String>, Option<String>)> = None;
    // First iteration: force a render so the panel gets the initial
    // "nothing playing" frame instead of holding the previous content.
    let mut force_next = true;

    loop {
        if *stop_rx.borrow_and_update() {
            break;
        }

        let np = match tokio::task::spawn_blocking(now_playing::fetch).await {
            Ok(Some(v)) => v,
            Ok(None) | Err(_) => NowPlaying::none(),
        };

        let sig = signature(&np);
        if force_next || Some(&sig) != last_sig.as_ref() {
            let frame = render_frame(&np);
            match upload_single_frame(frame).await {
                Ok(()) => {
                    last_sig = Some(sig);
                    force_next = false;
                }
                Err(e) => {
                    // Log + retry on next tick. Keep `last_sig` unset
                    // so we re-attempt rather than wedging on a single
                    // failed HID write.
                    tracing::debug!(?e, "now_playing_tft: upload failed");
                }
            }
        }

        // Sleep responsive to stop signal — break out within
        // POLL_INTERVAL after stop instead of always waiting the full
        // interval.
        tokio::select! {
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
            _ = stop_rx.changed() => {}
        }
    }
}

/// Encode + upload a single static frame (no animation). spawn_blocking
/// because hidapi is synchronous and a 9-chunk transfer is ~30 ms.
async fn upload_single_frame(frame: TftFrame) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let anim = TftAnimation {
            frames: vec![frame],
        };
        let tft = Connection::open_tft().map_err(AppError::from)?;
        tft.upload_tft_animation(&anim).map_err(AppError::from)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join now_playing upload: {e}")))?
}

/// Compose the 128×128 frame for the given Now-Playing snapshot.
///
/// Layout:
/// ```text
///   ┌──────────────────────────────┐ y=0
///   │  Title line 1 (8×13 bold)    │ y=4
///   │  Title line 2 if needed      │ y=20
///   ├──────────────────────────────┤
///   │  Artist (6×10)               │ y=44
///   ├──────────────────────────────┤
///   │                              │
///   │  Album (5×8, dim)            │ y=66
///   │                              │
///   ├──────────────────────────────┤
///   │  Source name (5×8)           │ y=116
///   └──────────────────────────────┘ y=127
/// ```
fn render_frame(np: &NowPlaying) -> TftFrame {
    let mut fb = Framebuffer128::new();
    fb.clear_rgb(0, 0, 0);

    if !np.is_playing || np.title.is_none() {
        render_nothing_playing(&mut fb);
    } else {
        render_track(&mut fb, np);
    }

    // The encode path expects exactly FRAME_BYTES of RGB565 LE pixels.
    // delay_ms doesn't matter for a single-frame animation; the device
    // just holds the frame.
    TftFrame {
        pixels: fb.into_pixels(),
        delay_ms: 0,
    }
}

fn render_nothing_playing(fb: &mut Framebuffer128) {
    // Centred "♪" plus "Nothing playing" subtitle.
    let big = MonoTextStyle::new(&FONT_8X13_BOLD, Rgb565::new(15, 30, 15));
    let small = MonoTextStyle::new(&FONT_6X10, Rgb565::new(10, 20, 10));
    let _ = Text::with_baseline("Nothing", Point::new(28, 48), big, Baseline::Top).draw(fb);
    let _ = Text::with_baseline("playing", Point::new(32, 64), big, Baseline::Top).draw(fb);
    let _ = Text::with_baseline("AK820 Pro", Point::new(34, 110), small, Baseline::Top).draw(fb);
}

fn render_track(fb: &mut Framebuffer128, np: &NowPlaying) {
    let title = np.title.as_deref().unwrap_or("");
    let artist = np.artist.as_deref().unwrap_or("");
    let album = np.album.as_deref().unwrap_or("");

    // FONT_8X13_BOLD = 16 chars per line @ 128 px wide.
    // FONT_6X10     = 21 chars per line
    // FONT_5X8      = 25 chars per line
    let title_style = MonoTextStyle::new(&FONT_8X13_BOLD, Rgb565::WHITE);
    let artist_style = MonoTextStyle::new(&FONT_6X10, Rgb565::new(31, 50, 31));
    let album_style = MonoTextStyle::new(&FONT_5X8, Rgb565::new(15, 30, 15));
    let source_style = MonoTextStyle::new(&FONT_5X8, Rgb565::new(10, 25, 25));

    // Title across two lines if it's longer than 16 chars. Word-wrap
    // would be nicer but split-mid-word is fine for a glance at the
    // panel — the user already knows what's playing, this is a hint.
    let (line1, line2) = split_for_two_lines(title, 16);
    let _ = Text::with_baseline(&line1, Point::new(2, 4), title_style, Baseline::Top).draw(fb);
    if !line2.is_empty() {
        let _ = Text::with_baseline(
            &fit_glyphs(&line2, 16),
            Point::new(2, 20),
            title_style,
            Baseline::Top,
        )
        .draw(fb);
    }

    // Artist on a single 6×10 line at y=44.
    let _ = Text::with_baseline(
        &fit_glyphs(artist, 21),
        Point::new(2, 44),
        artist_style,
        Baseline::Top,
    )
    .draw(fb);

    // Album on a single 5×8 line at y=66, only if non-empty.
    if !album.is_empty() {
        let _ = Text::with_baseline(
            &fit_glyphs(album, 25),
            Point::new(2, 66),
            album_style,
            Baseline::Top,
        )
        .draw(fb);
    }

    // Footer: "▶ Music" / "▶ Spotify" / "‖ Music" if paused.
    let prefix = if np.is_playing { "> " } else { "|| " };
    let footer = format!("{}{}", prefix, np.source);
    let _ = Text::with_baseline(
        &fit_glyphs(&footer, 25),
        Point::new(2, 116),
        source_style,
        Baseline::Top,
    )
    .draw(fb);
}

/// Split a string at a glyph boundary closest to `max` chars, preferring
/// a space if one falls in the last 5 chars before `max`. Returns
/// `(line1, line2)`. If `s` is `<= max` chars, `line2` is empty.
fn split_for_two_lines(s: &str, max: usize) -> (String, String) {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return (s.to_string(), String::new());
    }
    // Look for a space in the last 5 chars before `max`.
    let lo = max.saturating_sub(5);
    let split_at = (lo..max)
        .rev()
        .find(|&i| chars.get(i) == Some(&' '))
        .unwrap_or(max);
    let line1: String = chars[..split_at].iter().collect();
    let line2: String = chars[split_at..].iter().collect();
    (line1.trim_end().to_string(), line2.trim_start().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_string_returns_single_line() {
        let (a, b) = split_for_two_lines("Short", 16);
        assert_eq!(a, "Short");
        assert_eq!(b, "");
    }

    #[test]
    fn split_at_space_prefers_word_boundary() {
        // "A long song title here" — 22 chars total. The algorithm
        // scans indices 11..16 in reverse looking for a space. The
        // only space in that range is at index 11 (between "song" and
        // "title"), so split happens there → "A long song" + "title
        // here". This is the desired word-boundary behaviour.
        let (a, b) = split_for_two_lines("A long song title here", 16);
        assert_eq!(a, "A long song");
        assert_eq!(b, "title here");
    }

    #[test]
    fn split_no_space_falls_back_to_hard_cut() {
        let (a, b) = split_for_two_lines("verylongunbrokenstring", 10);
        assert_eq!(a, "verylongun");
        assert_eq!(b, "brokenstring");
    }

    #[test]
    fn render_frame_produces_correct_pixel_count() {
        let np = NowPlaying {
            source: "Music".into(),
            is_playing: true,
            title: Some("Hello".into()),
            artist: Some("World".into()),
            album: None,
        };
        let frame = render_frame(&np);
        // 128 × 128 × 2 = 32768
        assert_eq!(frame.pixels.len(), 32768);
    }

    #[test]
    fn render_frame_nothing_playing_also_emits_full_frame() {
        let np = NowPlaying::none();
        let frame = render_frame(&np);
        assert_eq!(frame.pixels.len(), 32768);
    }
}
