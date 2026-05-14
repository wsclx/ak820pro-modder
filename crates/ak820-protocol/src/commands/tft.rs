//! TFT display upload (`SET_TFT_USER_ANIMATION`, cmd 80).
//!
//! Hardware: NFP085B-10AF panel, 128 × 128 px, GC9107 controller.
//!
//! Wire format decoded from the AJAZZ online driver (function `Rt()` in
//! `index-CGDyjcPg.js`):
//!
//! ```text
//! +-------------------------------------+
//! |  256-byte frame-delay header        |
//! |    byte 0     = frame count N       |
//! |    byte 1..N-1 = delay[i] * 5  (ms) |
//! |    byte N     = 0x00 (terminator)   |
//! |    byte N+1..255 = 0xFF (pad)       |
//! +-------------------------------------+
//! |  Frame 0 RGB565 LE (32 768 bytes)   |
//! |  Frame 1 RGB565 LE (32 768 bytes)   |
//! |  …                                  |
//! +-------------------------------------+
//! ```
//!
//! See `docs/PROTOCOL.md` § SET_TFT_USER_ANIMATION for the full wire layout,
//! including the bespoke 8-byte per-chunk header and the 4096-byte chunk size
//! (which differs from every other command's 56-byte payload).

use serde::{Deserialize, Serialize};

use crate::error::*;

pub const TFT_WIDTH: u32 = 128;
pub const TFT_HEIGHT: u32 = 128;

/// Pixels per frame (128 × 128).
pub const PIXELS_PER_FRAME: usize = (TFT_WIDTH as usize) * (TFT_HEIGHT as usize);

/// Bytes per RGB565 pixel.
pub const BYTES_PER_PIXEL: usize = 2;

/// Bytes per encoded frame on the wire (128 × 128 × 2).
pub const FRAME_BYTES: usize = PIXELS_PER_FRAME * BYTES_PER_PIXEL;

/// Size of the frame-delay header that prefixes the pixel stream.
pub const FRAME_HEADER_BYTES: usize = 256;

/// Hard cap on frames per upload: header field is one byte and slot 0 is the
/// count itself, so at most 255 frames can be encoded. The device's own
/// `tftMaxFrames` in `GET_DEVICE_INFO` further constrains this (typically
/// ~30 on the AK820 Pro — clamp client-side before encode).
pub const MAX_FRAMES: usize = 255;

/// Per-frame delay multiplier. The driver stores delays in 5-ms units, so the
/// real delay is `header[i] * 5` milliseconds; the longest representable
/// per-frame delay is `255 * 5 = 1275 ms`.
pub const DELAY_UNIT_MS: u16 = 5;

/// Bespoke 8-byte per-chunk header used by `SET_TFT_USER_ANIMATION`. This is
/// **not** the same as `build_frame()` — TFT chunks have an explicit chunk
/// index and total count instead of an address + length.
///
/// Layout:
/// ```text
/// [0xAA, cmd, idx_lo, idx_hi, total_lo, total_hi, 0x4F, 0x06]
/// ```
/// The trailing two bytes form `0x064F` — a magic value derived from
/// `6619136 / 4096` in the JS source and appears to identify the payload
/// class (per-pixel vs per-LED frames share the same cmd byte path).
pub fn build_tft_header(cmd: u8, chunk_idx: u16, total_chunks: u16) -> [u8; 8] {
    [
        0xAA,
        cmd,
        (chunk_idx & 0xFF) as u8,
        ((chunk_idx >> 8) & 0xFF) as u8,
        (total_chunks & 0xFF) as u8,
        ((total_chunks >> 8) & 0xFF) as u8,
        0x4F,
        0x06,
    ]
}

/// One animation frame, in display-ready RGB565 LE pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TftFrame {
    /// Pixel scan order: row-major, top-left → bottom-right. Each pixel is two
    /// little-endian bytes (`[lo, hi]`) encoding RGB565.
    pub pixels: Vec<u8>,
    /// How long this frame stays on screen before the next one, in
    /// milliseconds. Rounded to a multiple of `DELAY_UNIT_MS` on encode.
    pub delay_ms: u16,
}

impl TftFrame {
    /// Construct a frame from an RGB888 source slice (length must be exactly
    /// `PIXELS_PER_FRAME * 3`). Performs the RGB888 → RGB565 quantisation
    /// inline.
    pub fn from_rgb888(rgb: &[u8], delay_ms: u16) -> Result<Self> {
        if rgb.len() != PIXELS_PER_FRAME * 3 {
            return Err(Error::FrameTooLong {
                len: rgb.len(),
                max: PIXELS_PER_FRAME * 3,
            });
        }
        let mut pixels = Vec::with_capacity(FRAME_BYTES);
        for chunk in rgb.chunks_exact(3) {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            let v = rgb888_to_rgb565(r, g, b);
            pixels.push((v & 0xFF) as u8);
            pixels.push((v >> 8) as u8);
        }
        Ok(Self { pixels, delay_ms })
    }
}

/// Full animation payload: header + concatenated frame pixel streams. Use
/// `TftAnimation::encode()` to produce the byte buffer that the device
/// chunked-upload path consumes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TftAnimation {
    pub frames: Vec<TftFrame>,
}

impl TftAnimation {
    /// Validate + serialise to a single contiguous buffer ready for chunked
    /// upload.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.frames.is_empty() {
            return Err(Error::NotImplemented("TFT animation must have at least one frame"));
        }
        if self.frames.len() > MAX_FRAMES {
            return Err(Error::OutOfRange {
                field: "tft frame count",
                value: self.frames.len() as i64,
                max: MAX_FRAMES as i64,
            });
        }
        for (i, f) in self.frames.iter().enumerate() {
            if f.pixels.len() != FRAME_BYTES {
                return Err(Error::FrameTooLong {
                    len: f.pixels.len(),
                    max: FRAME_BYTES,
                });
            }
            let _ = i;
        }

        // 256-byte frame-delay header.
        let n = self.frames.len();
        let mut header = [0xFFu8; FRAME_HEADER_BYTES];
        header[0] = n as u8;
        for i in 0..n.saturating_sub(1) {
            let scaled = (self.frames[i].delay_ms / DELAY_UNIT_MS).min(0xFF) as u8;
            header[i + 1] = scaled;
        }
        // Terminator at slot N. (For N=1 this lives at index 1, overwriting
        // the "first delay" slot — which is fine because there's no
        // transition.)
        if n > 0 {
            header[n] = 0x00;
        }

        // Concatenate header + per-frame pixel streams.
        let mut out = Vec::with_capacity(FRAME_HEADER_BYTES + n * FRAME_BYTES);
        out.extend_from_slice(&header);
        for f in &self.frames {
            out.extend_from_slice(&f.pixels);
        }
        Ok(out)
    }
}

/// Quantise one RGB888 sample into RGB565 (16-bit).
pub fn rgb888_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    let r5 = ((r as u16) >> 3) & 0x1F;
    let g6 = ((g as u16) >> 2) & 0x3F;
    let b5 = ((b as u16) >> 3) & 0x1F;
    (r5 << 11) | (g6 << 5) | b5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_color_frame(r: u8, g: u8, b: u8, delay_ms: u16) -> TftFrame {
        let mut rgb = Vec::with_capacity(PIXELS_PER_FRAME * 3);
        for _ in 0..PIXELS_PER_FRAME {
            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
        TftFrame::from_rgb888(&rgb, delay_ms).expect("solid frame")
    }

    #[test]
    fn rgb_quantisation_known_values() {
        assert_eq!(rgb888_to_rgb565(0, 0, 0), 0x0000);
        assert_eq!(rgb888_to_rgb565(0xFF, 0xFF, 0xFF), 0xFFFF);
        assert_eq!(rgb888_to_rgb565(0xFF, 0, 0), 0xF800);
        assert_eq!(rgb888_to_rgb565(0, 0xFF, 0), 0x07E0);
        assert_eq!(rgb888_to_rgb565(0, 0, 0xFF), 0x001F);
    }

    #[test]
    fn encode_single_frame_size_and_header() {
        let frame = solid_color_frame(0xFF, 0, 0, 0);
        let anim = TftAnimation { frames: vec![frame] };
        let buf = anim.encode().expect("encode");
        // 256 header + 1 frame
        assert_eq!(buf.len(), FRAME_HEADER_BYTES + FRAME_BYTES);
        assert_eq!(buf[0], 1, "header byte 0 = frame count");
        assert_eq!(buf[1], 0x00, "single-frame terminator");
        // The first pixel should be RGB565 LE for solid red = 0xF800 = [0x00, 0xF8].
        assert_eq!(&buf[FRAME_HEADER_BYTES..FRAME_HEADER_BYTES + 2], &[0x00, 0xF8]);
        // Trailing pad of header is 0xFF.
        assert_eq!(buf[FRAME_HEADER_BYTES - 1], 0xFF);
    }

    #[test]
    fn encode_three_frame_delays() {
        let frames = vec![
            solid_color_frame(0xFF, 0, 0, 100),
            solid_color_frame(0, 0xFF, 0, 200),
            solid_color_frame(0, 0, 0xFF, 1275),
        ];
        let anim = TftAnimation { frames };
        let buf = anim.encode().expect("encode");
        assert_eq!(buf[0], 3, "frame count");
        // delays[0] = 100 / 5 = 20
        assert_eq!(buf[1], 20);
        // delays[1] = 200 / 5 = 40
        assert_eq!(buf[2], 40);
        // 3rd frame has no "next" transition → terminator at slot 3.
        // (Source supplied 1275 ms but it isn't emitted; that's how AJAZZ does it.)
        assert_eq!(buf[3], 0x00);
        // Pad at slot 4..255 = 0xFF
        assert_eq!(buf[4], 0xFF);
        assert_eq!(buf[255], 0xFF);
        // Body length = 3 × FRAME_BYTES
        assert_eq!(buf.len(), FRAME_HEADER_BYTES + 3 * FRAME_BYTES);
    }

    #[test]
    fn reject_oversize_frame_count() {
        let frames = (0..MAX_FRAMES + 1)
            .map(|_| solid_color_frame(0, 0, 0, 0))
            .collect();
        let anim = TftAnimation { frames };
        let err = anim.encode().unwrap_err();
        assert!(matches!(err, Error::OutOfRange { .. }));
    }

    #[test]
    fn reject_wrong_pixel_length() {
        let frame = TftFrame { pixels: vec![0; 10], delay_ms: 0 };
        let anim = TftAnimation { frames: vec![frame] };
        let err = anim.encode().unwrap_err();
        assert!(matches!(err, Error::FrameTooLong { .. }));
    }

    #[test]
    fn chunk_header_layout() {
        let h = build_tft_header(80, 0, 1);
        assert_eq!(h, [0xAA, 80, 0, 0, 1, 0, 0x4F, 0x06]);
        let h2 = build_tft_header(80, 0x1234, 0x05);
        assert_eq!(h2, [0xAA, 80, 0x34, 0x12, 0x05, 0, 0x4F, 0x06]);
    }
}
