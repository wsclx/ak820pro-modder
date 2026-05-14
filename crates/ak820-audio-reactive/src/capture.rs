//! macOS system-audio capture via ScreenCaptureKit.
//!
//! # Permissions
//!
//! SCStream piggybacks on the **Screen Recording** TCC bucket — even for
//! audio-only capture, macOS requires the parent process (Terminal,
//! tauri-app .app bundle, etc.) to be approved. First call to
//! [`Capture::start`] triggers the system prompt; subsequent calls are
//! silent once granted. macOS 15.2+ introduced an audio-only API that
//! avoids this prompt but requires the user to pick a source via
//! `SCContentSharingPicker` — useful eventually, but out of scope for the
//! first cut.
//!
//! # Architecture
//!
//! ```text
//!  SCStream callback ──► std::mpsc::Sender ──► Receiver ◄── consumer
//!  (Apple dispatch queue,   (Send + Sync,           (CLI / Tauri
//!   foreign thread)          back-pressure free)     task)
//! ```
//!
//! Apple delivers audio buffers on a dispatch queue, **not** on a Tokio
//! worker. We push them through a sync `mpsc` channel and let the
//! consumer pull at its own cadence. No locks held across HID I/O.

use crate::{Error, Result};
use screencapturekit::cm::AudioBufferList;
use screencapturekit::prelude::{
    CMSampleBuffer, SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamOutputTrait, SCStreamOutputType,
};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Sample rate we ask SCStream to deliver. 48 kHz is the macOS-default
/// system audio rate; any other value forces a sample-rate-convert on the
/// Apple side that we don't need.
pub const SAMPLE_RATE: u32 = 48_000;

/// We request stereo and mix down to mono inside the callback. SCStream
/// delivers stereo regardless on most outputs; explicit `2` avoids
/// ambiguity in the audio_buffer_list layout.
const CHANNELS: u32 = 2;

pub struct Capture {
    /// Owning handle. Dropped via [`Drop`] which calls `stop_capture()`.
    _stream: SCStream,
    /// PCM samples (mono f32) pushed from the SCStream output handler.
    rx: Receiver<Vec<f32>>,
}

struct AudioHandler {
    tx: Sender<Vec<f32>>,
}

impl SCStreamOutputTrait for AudioHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }
        let Some(abl) = sample.audio_buffer_list() else {
            return;
        };
        if let Some(samples) = extract_mono_f32(&abl) {
            // If the receiver was dropped (Capture went away), this fails
            // silently — that's fine, we're racing with shutdown.
            let _ = self.tx.send(samples);
        }
    }
}

/// Dummy screen handler so SCStream is happy. Audio-only would be nicer
/// but the framework still emits screen frames as long as the filter
/// contains a display source; we just throw them away.
struct DiscardScreen;
impl SCStreamOutputTrait for DiscardScreen {
    fn did_output_sample_buffer(&self, _: CMSampleBuffer, _: SCStreamOutputType) {}
}

/// Pull PCM samples out of a CMSampleBuffer's AudioBufferList.
///
/// SCStream delivers **interleaved 32-bit float** by default (Float32 per
/// Apple's docs, little-endian on every Apple Silicon and Intel Mac).
/// For stereo input the first buffer in the list holds `[L,R,L,R,...]`;
/// we mix it down to mono by averaging channels — that's the right move
/// for a simple energy meter, and the analyzer doesn't care about stereo
/// imaging.
fn extract_mono_f32(abl: &AudioBufferList) -> Option<Vec<f32>> {
    let buf = abl.get(0)?;
    let bytes = buf.data();
    if bytes.is_empty() {
        return None;
    }
    // bytes.len() should always be a multiple of 4 (sizeof::<f32>()),
    // but stay defensive — a truncated final sample is harmless to drop.
    let mut frames: Vec<f32> = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        frames.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    if CHANNELS == 1 {
        return Some(frames);
    }
    let n = CHANNELS as usize;
    let mono: Vec<f32> = frames
        .chunks_exact(n)
        .map(|frame| frame.iter().sum::<f32>() / n as f32)
        .collect();
    Some(mono)
}

impl Capture {
    /// Start a system-audio capture stream. May block briefly while
    /// SCStream initialises; the TCC prompt (if not yet granted) is
    /// modal and may take arbitrarily long the first time.
    pub fn start() -> Result<Self> {
        let content = SCShareableContent::get()
            .map_err(|e| Error::Capture(format!("SCShareableContent::get: {e:?}")))?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| Error::Capture("no displays available".into()))?;

        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();

        let config = SCStreamConfiguration::new()
            // Tiny capture area — we throw the frames away, but SCStream
            // still allocates buffers for them. 64×64 keeps RAM trivial.
            .with_width(64)
            .with_height(64)
            .with_captures_audio(true)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(SAMPLE_RATE as i32)
            .with_channel_count(CHANNELS as i32);

        let (tx, rx) = channel();
        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(DiscardScreen, SCStreamOutputType::Screen);
        stream.add_output_handler(AudioHandler { tx }, SCStreamOutputType::Audio);
        stream
            .start_capture()
            .map_err(|e| Error::Capture(format!("start_capture: {e:?}")))?;

        tracing::info!(SAMPLE_RATE, CHANNELS, "audio capture started");
        Ok(Capture {
            _stream: stream,
            rx,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    /// Non-blocking pull. Returns `None` if no buffer is currently
    /// queued — caller should sleep briefly and try again.
    pub fn try_recv(&self) -> Option<Vec<f32>> {
        self.rx.try_recv().ok()
    }

    /// Block until the next audio buffer arrives or the capture is
    /// dropped. Useful for the smoke-test CLI; the Tauri app uses
    /// [`try_recv`] in a paced loop instead.
    pub fn recv_blocking(&self) -> Option<Vec<f32>> {
        self.rx.recv().ok()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Best-effort: log but don't panic. The stream's own Drop will
        // also release its Swift-side resources.
        if let Err(e) = self._stream.stop_capture() {
            tracing::warn!(?e, "SCStream stop_capture failed during drop");
        }
    }
}
