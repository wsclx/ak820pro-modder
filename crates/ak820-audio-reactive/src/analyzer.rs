//! FFT pipeline: PCM samples → band magnitudes → smoothed [`Frame`].
//!
//! Design notes (read these before tweaking):
//!
//! * **FFT length 1024 at 48 kHz** = 21 ms window. Enough resolution
//!   (~47 Hz / bin) to separate low bass from kick drums, short enough
//!   that lighting feels reactive instead of laggy. Power of 2 keeps
//!   realfft on a fast path.
//! * **Hann window** to suppress spectral leakage at band edges. Pre-
//!   computed in `new()` so the hot path is just a multiply.
//! * **dB scale, –60 dB floor**: human hearing is logarithmic. Without
//!   this conversion, a vocal track at –20 dB and a passing bass note
//!   at –10 dB end up looking the same on the keyboard.
//! * **Asymmetric EMA smoothing**: fast attack (rise) keeps beats
//!   visible; slow release (decay) prevents the LEDs from flickering
//!   like a Christmas tree on percussive content.
//! * **Bands chosen for keyboard zones**: bass ≤ 250 Hz (kick/sub),
//!   mids 250 Hz – 2 kHz (vocals/snare), highs 2 kHz – 12 kHz (hat/air).
//!   Above 12 kHz the FFT magnitudes are mostly noise and don't add
//!   musical information.

use crate::Frame;
use realfft::{num_complex::Complex32, RealFftPlanner, RealToComplex};
use std::f32::consts::PI;
use std::sync::Arc;

/// FFT window size. Must be a power of 2 for realfft's fast path.
pub const FFT_LEN: usize = 1024;

/// Asymmetric EMA: bigger = faster rise / fall.
const ATTACK: f32 = 0.6;
const RELEASE: f32 = 0.15;

/// dB threshold below which we clamp to zero. Anything quieter than
/// this is treated as silence for visual purposes.
const FLOOR_DB: f32 = -60.0;

pub struct Analyzer {
    sample_rate: f32,
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    /// Pre-computed Hann window coefficients of length [`FFT_LEN`].
    window: Vec<f32>,
    smoothed: Frame,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        assert!(sample_rate > 0, "sample_rate must be > 0");
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_LEN);
        let input = fft.make_input_vec();
        let output = fft.make_output_vec();
        let window: Vec<f32> = (0..FFT_LEN)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (FFT_LEN as f32 - 1.0)).cos())
            .collect();
        Self {
            sample_rate: sample_rate as f32,
            fft,
            input,
            output,
            window,
            smoothed: Frame::ZERO,
        }
    }

    /// Push the most recent `samples` into the FFT, return the updated
    /// smoothed [`Frame`].
    ///
    /// Uses the **trailing** `FFT_LEN` samples of the slice — anything
    /// older is ignored. If fewer than `FFT_LEN` samples are supplied,
    /// the leading slots are zero-padded. Passing a longer slice is the
    /// normal case (consumers typically batch several SCStream callbacks
    /// before analyzing); pass exactly `FFT_LEN` for tight latency.
    pub fn analyze(&mut self, samples: &[f32]) -> Frame {
        let n = samples.len().min(FFT_LEN);
        let pad = FFT_LEN - n;
        self.input[..pad].fill(0.0);
        self.input[pad..].copy_from_slice(&samples[samples.len() - n..]);

        // Hann-window in place
        for (x, w) in self.input.iter_mut().zip(self.window.iter()) {
            *x *= *w;
        }

        // Process is infallible for buffers we allocated via make_*_vec.
        // Any error here is a programmer bug, so panic — the alternative
        // is silently returning the previous frame which would mask it.
        self.fft
            .process(&mut self.input, &mut self.output)
            .expect("realfft process: input/output vec mismatch");

        let bin_hz = self.sample_rate / FFT_LEN as f32;
        // realfft returns *unnormalised* magnitudes — for a unity sine
        // input over `FFT_LEN` samples the peak bin has |c| ≈ N/2. We
        // need a normalisation factor so the numbers we feed into the
        // dB scale below land in the 0..=1 band the FLOOR_DB / 0 dB
        // endpoints assume. Without it the dB scale saturates at +40dB
        // for any real audio and all three lights peg to 1.0 — that's
        // what was happening on Mario's 0.6.0-beta build of this code.
        //
        // Factor choice: 4/N rather than the textbook 2/N. The Hann
        // window has coherent gain 0.5, so multiplying by 2 recovers a
        // unity-sine to ~unity amplitude. The dB scale then has the
        // full -60..0 dB dynamic range to work with for real audio.
        let amplitude_norm = 4.0 / FFT_LEN as f32;
        let band = |from_hz: f32, to_hz: f32| -> f32 {
            // bin 0 is DC; skip it.
            let from = ((from_hz / bin_hz) as usize).max(1);
            let to = ((to_hz / bin_hz) as usize + 1).min(self.output.len());
            if to <= from {
                return 0.0;
            }
            // RMS magnitude across the band, then normalise.
            let sum: f32 = self.output[from..to].iter().map(|c| c.norm_sqr()).sum();
            let rms = (sum / (to - from) as f32).sqrt();
            rms * amplitude_norm
        };

        let scale = |raw: f32| -> f32 {
            // Avoid log10(0) → -inf. 1e-6 maps to exactly FLOOR_DB at the
            // chosen floor.
            let db = 20.0 * raw.max(1e-6).log10();
            ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
        };

        let target = Frame {
            bass: scale(band(40.0, 250.0)),
            mids: scale(band(250.0, 2_000.0)),
            highs: scale(band(2_000.0, 12_000.0)),
        };

        self.smoothed.bass = ema(self.smoothed.bass, target.bass);
        self.smoothed.mids = ema(self.smoothed.mids, target.mids);
        self.smoothed.highs = ema(self.smoothed.highs, target.highs);
        self.smoothed
    }

    /// Last computed frame without running the FFT again.
    pub fn current(&self) -> Frame {
        self.smoothed
    }
}

fn ema(prev: f32, target: f32) -> f32 {
    let alpha = if target > prev { ATTACK } else { RELEASE };
    prev + alpha * (target - prev)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq_hz: f32, sample_rate: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * PI * freq_hz * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn silence_returns_zero_frame() {
        let mut a = Analyzer::new(48_000);
        let frame = a.analyze(&[0.0; FFT_LEN]);
        assert_eq!(frame, Frame::ZERO);
    }

    #[test]
    fn bass_band_lights_up_on_100hz_sine() {
        let mut a = Analyzer::new(48_000);
        let samples = sine(100.0, 48_000, FFT_LEN);
        // EMA settles over a few calls; give it 5 frames.
        let mut frame = Frame::ZERO;
        for _ in 0..5 {
            frame = a.analyze(&samples);
        }
        assert!(
            frame.bass > 0.5,
            "bass={:.3} should be loud on 100 Hz sine",
            frame.bass
        );
        assert!(
            frame.bass > frame.mids,
            "bass={:.3} should beat mids={:.3} at 100 Hz",
            frame.bass,
            frame.mids
        );
        assert!(
            frame.bass > frame.highs,
            "bass={:.3} should beat highs={:.3} at 100 Hz",
            frame.bass,
            frame.highs
        );
    }

    #[test]
    fn highs_band_lights_up_on_8khz_sine() {
        let mut a = Analyzer::new(48_000);
        let samples = sine(8_000.0, 48_000, FFT_LEN);
        let mut frame = Frame::ZERO;
        for _ in 0..5 {
            frame = a.analyze(&samples);
        }
        assert!(
            frame.highs > 0.5,
            "highs={:.3} should be loud on 8 kHz sine",
            frame.highs
        );
        assert!(frame.highs > frame.bass, "highs should beat bass at 8 kHz");
    }

    #[test]
    fn mids_band_lights_up_on_1khz_sine() {
        let mut a = Analyzer::new(48_000);
        let samples = sine(1_000.0, 48_000, FFT_LEN);
        let mut frame = Frame::ZERO;
        for _ in 0..5 {
            frame = a.analyze(&samples);
        }
        assert!(
            frame.mids > 0.5,
            "mids={:.3} should be loud on 1 kHz sine",
            frame.mids
        );
        assert!(frame.mids > frame.bass, "mids should beat bass at 1 kHz");
        assert!(frame.mids > frame.highs, "mids should beat highs at 1 kHz");
    }

    #[test]
    fn release_is_slower_than_attack() {
        // Verify that after a loud transient the meter falls more slowly
        // than it rose — the asymmetric EMA is what makes lighting "feel"
        // musical instead of flickery.
        let mut a = Analyzer::new(48_000);
        let loud = sine(100.0, 48_000, FFT_LEN);
        let silent = vec![0.0_f32; FFT_LEN];

        // Rise: one analyze of loud audio should already push bass well up.
        let after_one_loud = a.analyze(&loud).bass;
        assert!(after_one_loud > 0.3);

        // Push it to near-steady state, then go silent.
        for _ in 0..5 {
            a.analyze(&loud);
        }
        let steady = a.analyze(&loud).bass;
        let after_one_silent = a.analyze(&silent).bass;

        let rise = after_one_loud; // from 0
        let fall = steady - after_one_silent; // toward 0
        assert!(
            rise > fall * 1.5,
            "rise={rise:.3} should be noticeably faster than fall={fall:.3}"
        );
    }
}
