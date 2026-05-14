//! Image upload pipeline for the TFT panel.
//!
//! Reads a PNG / JPEG / GIF byte buffer, fits it to the AK820 Pro's
//! 128 × 128 panel, quantises to RGB565, and produces a [`TftAnimation`]
//! the device's `SET_TFT_USER_ANIMATION` path can ingest.
//!
//! **MP4 / video** is out of scope for this iteration — pulling in
//! ffmpeg-like deps would more than double the binary, and a user with
//! an mp4 can decimate it to a GIF via ffmpeg / Gifski / native Photos
//! before importing.
//!
//! ## Fit modes
//!
//! See [`FitMode`]. The web driver implicitly uses **Fill** (resize so
//! the shorter side matches, crop centred on the longer side) — pictures
//! always come out edge-to-edge. We expose the three common options so
//! a contributor doesn't have to crop their image first.

use super::tft::{TftAnimation, TftFrame, FRAME_BYTES, TFT_HEIGHT, TFT_WIDTH};
use crate::error::{Error, Result};
use image::imageops::FilterType;
use image::{AnimationDecoder, GenericImageView, Rgba};
use serde::{Deserialize, Serialize};

/// How to fit an arbitrary-aspect source into the 128 × 128 target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    /// Resize so the shorter side matches 128, crop the longer side
    /// equally on both ends to a 128 × 128 square. Picture goes
    /// edge-to-edge. **Default** — matches what the AJAZZ web tool does
    /// for stills.
    Fill,
    /// Resize so the longer side matches 128, letterbox the shorter
    /// side with black bars. Preserves the entire image content.
    Contain,
    /// Stretch each axis to 128 independently. Distorts non-square
    /// sources; only useful when you already cropped to a square.
    Stretch,
}

impl Default for FitMode {
    fn default() -> Self {
        Self::Fill
    }
}

impl FitMode {
    /// Parse from a lowercase string. Named `parse_lenient` rather than
    /// `from_str` so it doesn't clash with `std::str::FromStr` — we want
    /// to forgive unknown values (fall back to `Fill`) rather than error,
    /// and that conflicts with `FromStr`'s `Result` contract.
    pub fn parse_lenient(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "contain" => Self::Contain,
            "stretch" => Self::Stretch,
            _ => Self::Fill,
        }
    }
}

/// Frame-delay caps. The protocol allows up to 255 × 5 ms = 1275 ms per
/// frame slot, and `MAX_FRAMES` total — we mirror those for GIFs but
/// also clamp ridiculously short delays (e.g. 0 ms or 10 ms) to a sane
/// minimum so the panel isn't asked to refresh faster than it can.
const MIN_FRAME_DELAY_MS: u16 = 40; // 25 fps ceiling
const MAX_FRAMES_FOR_GIF: usize = 30; // device-reported `tftMaxFrames` ≈ 30

/// Decode an image byte buffer and turn it into a [`TftAnimation`].
/// Auto-detects format from the byte signature (PNG / JPEG / GIF). For
/// GIFs, every frame is decoded, fitted, and quantised in turn. For
/// stills, the result is a single-frame animation.
pub fn animation_from_bytes(bytes: &[u8], fit: FitMode) -> Result<TftAnimation> {
    if bytes.is_empty() {
        return Err(Error::UnexpectedResponse("empty image buffer".into()));
    }

    // GIF gets a dedicated decoder because we want per-frame delays.
    // PNG / JPEG go through the generic loader.
    if is_gif(bytes) {
        return animation_from_gif(bytes, fit);
    }

    let img = image::load_from_memory(bytes)
        .map_err(|e| Error::UnexpectedResponse(format!("image decode: {e}")))?;
    let frame = fit_and_quantise(&img, fit, 200);
    Ok(TftAnimation {
        frames: vec![frame],
    })
}

fn is_gif(bytes: &[u8]) -> bool {
    bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
}

fn animation_from_gif(bytes: &[u8], fit: FitMode) -> Result<TftAnimation> {
    let cursor = std::io::Cursor::new(bytes);
    let decoder = image::codecs::gif::GifDecoder::new(cursor)
        .map_err(|e| Error::UnexpectedResponse(format!("gif decode: {e}")))?;
    let mut frames = Vec::new();
    for (i, raw) in decoder.into_frames().enumerate() {
        let raw = raw.map_err(|e| Error::UnexpectedResponse(format!("gif frame {i}: {e}")))?;
        let delay_ms = raw.delay().numer_denom_ms();
        // `delay()` is the per-frame display duration as a rational
        // (numer, denom) in milliseconds. Round to nearest whole ms,
        // then clamp to the protocol's representable range.
        let delay_ms = (delay_ms.0 as u64).saturating_div(delay_ms.1.max(1) as u64) as u16;
        let delay_ms = delay_ms.max(MIN_FRAME_DELAY_MS);
        let img = image::DynamicImage::ImageRgba8(raw.into_buffer());
        frames.push(fit_and_quantise(&img, fit, delay_ms));
        if frames.len() >= MAX_FRAMES_FOR_GIF {
            tracing::warn!(
                limit = MAX_FRAMES_FOR_GIF,
                "GIF exceeds device frame budget; truncating"
            );
            break;
        }
    }
    if frames.is_empty() {
        return Err(Error::UnexpectedResponse("gif contained no frames".into()));
    }
    Ok(TftAnimation { frames })
}

/// Apply the chosen fit mode to a source `DynamicImage`, then quantise to
/// RGB565 and pack into a [`TftFrame`] with the supplied delay.
fn fit_and_quantise(img: &image::DynamicImage, fit: FitMode, delay_ms: u16) -> TftFrame {
    let target = match fit {
        FitMode::Fill => fit_fill(img),
        FitMode::Contain => fit_contain(img),
        FitMode::Stretch => fit_stretch(img),
    };
    let rgba = target.to_rgba8();
    debug_assert_eq!(rgba.width(), TFT_WIDTH);
    debug_assert_eq!(rgba.height(), TFT_HEIGHT);
    let mut pixels = Vec::with_capacity(FRAME_BYTES);
    for px in rgba.pixels() {
        let Rgba([r, g, b, _a]) = *px;
        let r5 = (r >> 3) as u16;
        let g6 = (g >> 2) as u16;
        let b5 = (b >> 3) as u16;
        let v = (r5 << 11) | (g6 << 5) | b5;
        pixels.extend_from_slice(&v.to_le_bytes());
    }
    TftFrame { pixels, delay_ms }
}

fn fit_fill(img: &image::DynamicImage) -> image::DynamicImage {
    // Centre-crop to a square, then resize to the panel size.
    let (w, h) = img.dimensions();
    let side = w.min(h);
    let x0 = (w - side) / 2;
    let y0 = (h - side) / 2;
    let cropped = img.crop_imm(x0, y0, side, side);
    cropped.resize_exact(TFT_WIDTH, TFT_HEIGHT, FilterType::Lanczos3)
}

fn fit_contain(img: &image::DynamicImage) -> image::DynamicImage {
    use image::imageops::overlay;
    use image::{ImageBuffer, Rgba};
    let resized = img.resize(TFT_WIDTH, TFT_HEIGHT, FilterType::Lanczos3);
    let (rw, rh) = resized.dimensions();
    // Black canvas underneath, centre the resized image on top.
    let mut canvas: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(TFT_WIDTH, TFT_HEIGHT, Rgba([0, 0, 0, 255]));
    let x = ((TFT_WIDTH.saturating_sub(rw)) / 2) as i64;
    let y = ((TFT_HEIGHT.saturating_sub(rh)) / 2) as i64;
    overlay(&mut canvas, &resized.to_rgba8(), x, y);
    image::DynamicImage::ImageRgba8(canvas)
}

fn fit_stretch(img: &image::DynamicImage) -> image::DynamicImage {
    img.resize_exact(TFT_WIDTH, TFT_HEIGHT, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a 4 × 4 PNG with a known colour pattern, encode it,
    /// then round-trip through `animation_from_bytes`. Mostly a sanity
    /// gate so the dependency wiring doesn't silently regress.
    #[test]
    fn png_round_trip_produces_single_full_frame() {
        let img = image::ImageBuffer::from_fn(4, 4, |x, _y| {
            if x < 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 255, 0, 255])
            }
        });
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let anim = animation_from_bytes(&bytes, FitMode::Fill).unwrap();
        assert_eq!(anim.frames.len(), 1);
        assert_eq!(anim.frames[0].pixels.len(), FRAME_BYTES);
    }

    #[test]
    fn fit_mode_parses_case_insensitively() {
        assert_eq!(FitMode::parse_lenient("fill"), FitMode::Fill);
        assert_eq!(FitMode::parse_lenient("FILL"), FitMode::Fill);
        assert_eq!(FitMode::parse_lenient("Contain"), FitMode::Contain);
        assert_eq!(FitMode::parse_lenient("stretch"), FitMode::Stretch);
        assert_eq!(FitMode::parse_lenient("nonsense"), FitMode::Fill); // safe fallback
    }

    #[test]
    fn empty_buffer_errors_out() {
        assert!(animation_from_bytes(&[], FitMode::Fill).is_err());
    }
}
