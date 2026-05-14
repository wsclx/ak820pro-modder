//! Audio-reactive lighting driver (macOS only).
//!
//! Owns a single background task that ties together:
//!
//! ```text
//!   ScreenCaptureKit → ak820-audio-reactive::Capture     (PCM, mpsc)
//!                                ↓
//!                  ak820-audio-reactive::Analyzer        (Frame {bass, mids, highs})
//!                                ↓
//!                       preset map (Frame → CustomLedMap)
//!                                ↓
//!                  Connection::set_custom_led + apply_lighting(Custom)
//! ```
//!
//! Only one such task can run at a time — `start()` returns silently if
//! a previous task is already running, `stop()` is a no-op if none is.
//! State transitions go through a `tokio::sync::Mutex<Inner>` so the
//! Tauri command handlers never race against each other or against the
//! task's own teardown.
//!
//! # Why this isn't `spawn_blocking`
//!
//! The inner loop is mostly waiting — `try_recv` on the SCStream pipe,
//! `tokio::time::sleep(33ms)`, then a short HID write. Spawning a
//! regular `tokio::spawn` task keeps the loop integrated with the
//! existing async commands (which use the same `ConnState` mutex) and
//! lets us cancel via a `watch::Sender<bool>` stop signal instead of
//! poll-on-AtomicBool.
//!
//! Capture creation itself *does* briefly block (SCStream init + first-
//! run TCC prompt). We accept the worker-thread block: the runtime has
//! several workers, and the prompt only happens once per macOS install.

use ak820_audio_reactive::{analyzer::FFT_LEN, Analyzer, Capture, Frame};
use ak820_protocol::commands::lighting::{Direction, LightingConfig, Mode};
use ak820_protocol::commands::per_key_rgb::{CustomLedMap, LedColor, LED_COUNT};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::AppError;

/// Target output framerate.
///
/// One `SET_CUSTOM_LED_DATA` is `ceil(128 LEDs × 4 B / 56 B per packet) = 10`
/// HID output reports. At ~3 ms per chunk (write + ack roundtrip), a full
/// LED update takes ~30 ms. At 30 fps that's a 33 ms window — essentially
/// 100% mutex contention against the rest of the app, which on Mario's
/// box manifested as visible flicker. 15 fps doubles the headroom and
/// reads smoothly to the eye. If we ever want >15 fps we'd need to skip
/// the per-chunk ack read in `set_many_at`, which is a protocol-layer
/// change.
const TARGET_FPS: u32 = 15;

/// Brightness ceiling for any single channel inside a band-coloured
/// pixel. 220 (instead of 255) leaves the keyboard's overall mode-light
/// looking colour-correct against its case rather than over-saturated.
const CHANNEL_MAX: f32 = 220.0;

/// Minimum brightness (as a fraction of `CHANNEL_MAX`) below which the
/// LEDs never go in a band's zone, even when the analyzer reads zero.
/// Without this, quiet moments between beats blank the keyboard fully —
/// which reads to the eye as the LEDs trying to "switch off and back
/// on" rather than as a musical decay. The floor keeps the structure
/// visible at all times and makes loud passages feel additive on top.
const BRIGHTNESS_FLOOR: f32 = 0.08;

/// Gamma curve applied to each band magnitude before the LED brightness
/// scale. Human perception of brightness is roughly `display ^ 2.2`, so
/// linear magnitudes look squashed in the middle. `0.7` is the inverse
/// curve (≈ 1/1.4) that brings mid-loudness signals back to "fully
/// visible" without over-saturating the loud peaks.
const BAND_GAMMA: f32 = 0.7;

/// Inner state behind the public `AudioReactiveState`. Held under a
/// tokio mutex so `start` / `stop` / `status` from Tauri commands
/// serialise cleanly.
struct Inner {
    stop_tx: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub struct AudioReactiveState(Mutex<Option<Inner>>);

impl AudioReactiveState {
    pub async fn is_running(&self) -> bool {
        self.0.lock().await.is_some()
    }

    /// Start the background task. If one is already running this is a
    /// silent no-op (the second `start` press in the UI shouldn't error).
    /// Errors only on the SCStream open path — Screen-Recording TCC denied,
    /// no displays, etc.
    pub async fn start(self: &Arc<Self>, conn: Arc<crate::ConnState>) -> Result<(), AppError> {
        let mut guard = self.0.lock().await;
        if guard.is_some() {
            tracing::debug!("audio_reactive: already running, ignoring start");
            return Ok(());
        }

        // Open the capture *before* spawning the task so the SCStream
        // error (Screen Recording permission denied, no display, …)
        // bubbles back to the caller and the UI shows a real message
        // rather than a silent "started but instantly stopped" state.
        let capture =
            Capture::start().map_err(|e| AppError::Protocol(format!("audio capture: {e}")))?;

        let (stop_tx, stop_rx) = watch::channel(false);
        let state = self.clone();
        let conn_for_task = conn.clone();
        let handle = tokio::spawn(async move {
            run_loop(capture, conn_for_task, stop_rx).await;
            // Self-clear when the loop exits on its own (capture error,
            // panic in send, …). The UI's periodic `status` poll will
            // then notice we're back to "stopped".
            *state.0.lock().await = None;
            tracing::info!("audio_reactive: loop exited, state cleared");
        });

        *guard = Some(Inner { stop_tx, handle });
        tracing::info!("audio_reactive: started");
        Ok(())
    }

    pub async fn stop(&self) {
        let Some(inner) = self.0.lock().await.take() else {
            return;
        };
        let _ = inner.stop_tx.send(true);
        // Don't block the Tauri command on `await`ing the task; the
        // loop will see the stop signal on its next tick (≤33ms) and
        // clean up. UI gets immediate `false` from `status`.
        inner.handle.abort();
        tracing::info!("audio_reactive: stop requested");
    }
}

async fn run_loop(
    capture: Capture,
    conn: Arc<crate::ConnState>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut analyzer = Analyzer::new(capture.sample_rate());
    let mut pcm: Vec<f32> = Vec::with_capacity(FFT_LEN * 4);
    let frame_period = Duration::from_secs_f32(1.0 / TARGET_FPS as f32);

    // Switch the keyboard into `custom` lighting mode once at the top.
    // Without this the per-key buffer we write below is accepted but
    // never rendered — the firmware keeps showing whatever effect was
    // active before audio-reactive was toggled on.
    if let Err(e) = enable_custom_mode(&conn).await {
        tracing::warn!(?e, "audio_reactive: failed to switch into custom mode");
    }

    loop {
        // Check stop signal up-front so a long-running TCC dialog that
        // resolved into a working capture but then got cancelled doesn't
        // run forever.
        if *stop_rx.borrow_and_update() {
            break;
        }

        // Drain everything the SCStream callback has queued since our
        // last tick. Each callback carries ~10ms of audio; capping the
        // rolling buffer at FFT_LEN×4 keeps memory steady even if the
        // streamer falls behind for a moment.
        while let Some(chunk) = capture.try_recv() {
            pcm.extend_from_slice(&chunk);
            if pcm.len() > FFT_LEN * 4 {
                let drop_n = pcm.len() - FFT_LEN * 2;
                pcm.drain(..drop_n);
            }
        }

        if pcm.len() >= FFT_LEN {
            let frame = analyzer.analyze(&pcm);
            let map = build_spectrum_map(frame);
            if let Err(e) = write_custom_led(&conn, map).await {
                // Don't bail on a single HID error — the auto-reconnect
                // logic in `ConnState::with` already cleared the cached
                // handle, and the next tick will re-open. Just log.
                tracing::debug!(?e, "audio_reactive: set_custom_led failed (will retry)");
            }
        }

        // `tokio::time::sleep` yields cleanly so a concurrent UI command
        // grabs the conn mutex between our HID writes.
        tokio::select! {
            _ = tokio::time::sleep(frame_period) => {}
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break;
                }
            }
        }
    }

    tracing::info!("audio_reactive: loop teardown");
}

/// Switch the keyboard into per-key Custom mode so the `SET_CUSTOM_LED_DATA`
/// buffer we stream below is actually rendered. We push a Custom-mode
/// lighting config once; the firmware remembers it until we send another
/// mode change.
async fn enable_custom_mode(conn: &crate::ConnState) -> Result<(), AppError> {
    // `LightingConfig` has no Default impl (mode/color/brightness are
    // mandatory), so spell out a minimal Custom-mode payload here.
    // The colour fields are placeholders — Custom mode renders from the
    // per-LED buffer we stream below, not from this single RGB triple.
    let cfg = LightingConfig {
        mode: Mode::Custom,
        color: "FFFFFF".into(),
        secondary: None,
        color_mode: 0,
        effect_mode_type: 0,
        brightness: 5,
        speed: 3,
        direction: Direction::Left,
    };
    conn.with(|slot| {
        let c = crate::ensure_open(slot)?;
        c.set_lighting(&cfg)?;
        Ok(())
    })
    .await
}

async fn write_custom_led(conn: &crate::ConnState, map: CustomLedMap) -> Result<(), AppError> {
    conn.with(|slot| {
        let c = crate::ensure_open(slot)?;
        c.set_custom_led(&map)?;
        Ok(())
    })
    .await
}

/// **Spectrum** preset — divides the keyboard into three vertical zones
/// based on the (col = slot % 16) coordinate of each LED:
///
/// * cols  0..= 4 → red, brightness = `frame.bass`
/// * cols  5..=10 → green, brightness = `frame.mids`
/// * cols 11..=15 → blue, brightness = `frame.highs`
///
/// The slot/16 modulo is a rough approximation of physical column on
/// the AK820 Pro's ISO layout — special keys (Enter, Backspace, arrows)
/// land at higher slot indices and pick up the rightmost (highs) band
/// as a side effect. That's intentional: it puts the navigation cluster
/// in the same colour zone visually.
fn build_spectrum_map(frame: Frame) -> CustomLedMap {
    let leds = (0..LED_COUNT as u8)
        .map(|id| {
            let col = id % 16;
            let (band, channel) = match col {
                0..=4 => (frame.bass, Channel::Red),
                5..=10 => (frame.mids, Channel::Green),
                _ => (frame.highs, Channel::Blue),
            };
            // Gamma-correct so mid-loudness reads as a proper mid-brightness
            // rather than the squashed-middle look that linear scaling
            // produces against a CRT-style display response. Then clamp into
            // [BRIGHTNESS_FLOOR..=1] so quiet moments never blank a LED
            // fully — the keyboard keeps its structure visible at all times
            // and loud passages feel additive.
            let shaped = band.clamp(0.0, 1.0).powf(BAND_GAMMA);
            let scaled = BRIGHTNESS_FLOOR + shaped * (1.0 - BRIGHTNESS_FLOOR);
            let v = (scaled * CHANNEL_MAX) as u8;
            let (red, green, blue) = match channel {
                Channel::Red => (v, 0, 0),
                Channel::Green => (0, v, 0),
                Channel::Blue => (0, 0, v),
            };
            LedColor {
                led_id: id,
                red,
                green,
                blue,
            }
        })
        .collect();
    CustomLedMap { leds }
}

enum Channel {
    Red,
    Green,
    Blue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_map_assigns_each_band_to_distinct_columns() {
        // Push all three bands to unity, then verify column 0 lights red,
        // column 7 lights green, column 15 lights blue with no mixing.
        let frame = Frame {
            bass: 1.0,
            mids: 1.0,
            highs: 1.0,
        };
        let map = build_spectrum_map(frame);

        let max = CHANNEL_MAX as u8;
        // led_id 0 → col 0 → bass → red only
        assert_eq!(map.leds[0].red, max);
        assert_eq!(map.leds[0].green, 0);
        assert_eq!(map.leds[0].blue, 0);
        // led_id 7 → col 7 → mids → green only
        assert_eq!(map.leds[7].green, max);
        assert_eq!(map.leds[7].red, 0);
        assert_eq!(map.leds[7].blue, 0);
        // led_id 15 → col 15 → highs → blue only
        assert_eq!(map.leds[15].blue, max);
        assert_eq!(map.leds[15].red, 0);
        assert_eq!(map.leds[15].green, 0);
    }

    #[test]
    fn spectrum_map_floors_silent_bands_above_zero() {
        // Floor exists so the keyboard stays visible during quiet
        // moments instead of blanking and reading as "switching off".
        let frame = Frame::ZERO;
        let map = build_spectrum_map(frame);
        let floor = (BRIGHTNESS_FLOOR * CHANNEL_MAX) as u8;
        assert_eq!(map.leds[0].red, floor, "bass column should sit at floor");
        assert_eq!(map.leds[7].green, floor, "mids column should sit at floor");
        assert_eq!(map.leds[15].blue, floor, "highs column should sit at floor");
        // Off-channel pixels of those columns must stay fully dark.
        assert_eq!(map.leds[0].green, 0);
        assert_eq!(map.leds[7].red, 0);
        assert_eq!(map.leds[15].green, 0);
    }

    #[test]
    fn spectrum_map_is_monotone_in_band_magnitude() {
        // Brightness must rise monotonically with band magnitude — the
        // exact curve is gamma-shaped (mid-values are pulled *up*), but
        // 0 ≤ 0.5 ≤ 1.0 always holds. This locks the contract without
        // pinning the curve, so we can re-tune BAND_GAMMA without test
        // churn.
        let zero = build_spectrum_map(Frame::ZERO).leds[0].red;
        let half = build_spectrum_map(Frame {
            bass: 0.5,
            mids: 0.0,
            highs: 0.0,
        })
        .leds[0]
            .red;
        let full = build_spectrum_map(Frame {
            bass: 1.0,
            mids: 0.0,
            highs: 0.0,
        })
        .leds[0]
            .red;
        assert!(zero < half, "zero={zero} should be below half={half}");
        assert!(half < full, "half={half} should be below full={full}");
        assert_eq!(full, CHANNEL_MAX as u8, "unity band should hit ceiling");
        // Gamma curve specifically lifts mid-loudness above the linear
        // midpoint — sanity-check that's working.
        let linear_mid = ((CHANNEL_MAX + BRIGHTNESS_FLOOR * CHANNEL_MAX) * 0.5) as u8;
        assert!(
            half > linear_mid,
            "gamma should lift half-band brightness above linear midpoint \
             (half={half}, linear_mid={linear_mid})"
        );
    }
}
