//! Audio-reactive lighting pipeline for the AK820 Pro.
//!
//! Two halves:
//!
//! * [`analyzer`] — pure-Rust FFT + band aggregation. Cross-platform,
//!   unit-testable with synthetic input.
//! * [`capture`] (macOS only) — taps system audio via Apple's
//!   ScreenCaptureKit, hands PCM samples off to a single-consumer
//!   channel.
//!
//! Consumers (CLI smoke probe, Tauri app) own a [`capture::Capture`],
//! periodically pull samples, feed them into [`analyzer::Analyzer`],
//! and turn the returned [`Frame`] into whatever colour scheme they
//! want. The analyzer never touches the device; the capture never
//! touches the LEDs. Each can be swapped in isolation.

pub mod analyzer;

/// Real audio capture only ships when the `capture` feature is on AND
/// the target is macOS. Other platforms (Linux dev box, Windows) get a
/// compile error if they try to use [`Capture`] — that's intentional;
/// we'd rather fail loudly than silently fall back to silence.
#[cfg(all(target_os = "macos", feature = "capture"))]
pub mod capture;

pub use analyzer::Analyzer;

#[cfg(all(target_os = "macos", feature = "capture"))]
pub use capture::Capture;

/// Per-frame normalised band magnitudes, smoothed across calls.
///
/// All three fields are in `0.0..=1.0`. The analyzer applies a log-scale
/// (dB → linear) so the values follow perceived loudness rather than raw
/// FFT amplitudes — that's what makes them useful for visual feedback.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frame {
    pub bass: f32,
    pub mids: f32,
    pub highs: f32,
}

impl Frame {
    pub const ZERO: Self = Self {
        bass: 0.0,
        mids: 0.0,
        highs: 0.0,
    };

    /// Maximum component, handy for global beat / RMS-ish indicators.
    pub fn peak(&self) -> f32 {
        self.bass.max(self.mids).max(self.highs)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("audio capture: {0}")]
    Capture(String),

    #[error("audio backend not available on this platform")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, Error>;
