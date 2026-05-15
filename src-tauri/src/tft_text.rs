//! Tiny text rasteriser for the AK820 Pro TFT panel.
//!
//! Bridges [`embedded-graphics`](https://docs.rs/embedded-graphics) onto
//! our 128 × 128 RGB565-LE wire format. The TFT firmware accepts frames
//! as little-endian RGB565; everything in this module produces that
//! directly so the caller can hand the buffer straight to
//! `TftFrame::pixels`.
//!
//! Used by the Now-Playing TFT preset (`now_playing_tft.rs`) to render
//! track + artist text every poll tick.
//!
//! ## What we don't do
//!
//! - No proportional-width fonts. `embedded-graphics`'s monospace
//!   `MonoFont` lookup is the cheapest path and a 128 × 128 panel reads
//!   fine with it.
//! - No anti-aliasing. The panel is small and the firmware's gamma is
//!   unknown; AA would mostly produce blurry grey edges.

use ak820_protocol::commands::tft::{rgb888_to_rgb565, FRAME_BYTES, PIXELS_PER_FRAME, TFT_WIDTH};
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{DrawTarget, OriginDimensions, Pixel, RgbColor, Size},
    primitives::Rectangle,
};

/// 128 × 128 RGB565-LE framebuffer. Stored as `Vec<u8>` so we can move
/// the inner buffer into a `TftFrame` without copying.
pub struct Framebuffer128 {
    pixels: Vec<u8>,
}

impl Framebuffer128 {
    pub fn new() -> Self {
        Self {
            pixels: vec![0u8; FRAME_BYTES],
        }
    }

    /// Clear the entire surface to an RGB888 colour.
    pub fn clear_rgb(&mut self, r: u8, g: u8, b: u8) {
        let v = rgb888_to_rgb565(r, g, b);
        let lo = (v & 0xFF) as u8;
        let hi = (v >> 8) as u8;
        for i in 0..PIXELS_PER_FRAME {
            self.pixels[i * 2] = lo;
            self.pixels[i * 2 + 1] = hi;
        }
    }

    /// Consume into the raw RGB565-LE pixel buffer suitable for
    /// `ak820_protocol::commands::tft::TftFrame::pixels`.
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Write a single RGB565 (LE) pixel without bounds checking.
    /// Caller must have verified `x < 128 && y < 128`.
    fn set_unchecked(&mut self, x: usize, y: usize, v: u16) {
        let idx = (y * TFT_WIDTH as usize + x) * 2;
        self.pixels[idx] = (v & 0xFF) as u8;
        self.pixels[idx + 1] = (v >> 8) as u8;
    }
}

impl Default for Framebuffer128 {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginDimensions for Framebuffer128 {
    fn size(&self) -> Size {
        Size::new(TFT_WIDTH, TFT_WIDTH)
    }
}

impl DrawTarget for Framebuffer128 {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let w = TFT_WIDTH as i32;
        for Pixel(p, c) in pixels {
            if p.x < 0 || p.y < 0 || p.x >= w || p.y >= w {
                continue;
            }
            let r = c.r() << 3 | c.r() >> 2;
            let g = c.g() << 2 | c.g() >> 4;
            let b = c.b() << 3 | c.b() >> 2;
            let v = rgb888_to_rgb565(r, g, b);
            self.set_unchecked(p.x as usize, p.y as usize, v);
        }
        Ok(())
    }

    /// Specialised fast-path for solid rects — used by background fills.
    fn fill_solid(&mut self, area: &Rectangle, c: Self::Color) -> Result<(), Self::Error> {
        let w = TFT_WIDTH as i32;
        let x0 = area.top_left.x.max(0);
        let y0 = area.top_left.y.max(0);
        let x1 = (area.top_left.x + area.size.width as i32).min(w);
        let y1 = (area.top_left.y + area.size.height as i32).min(w);
        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        let r = c.r() << 3 | c.r() >> 2;
        let g = c.g() << 2 | c.g() >> 4;
        let b = c.b() << 3 | c.b() >> 2;
        let v = rgb888_to_rgb565(r, g, b);
        for y in y0..y1 {
            for x in x0..x1 {
                self.set_unchecked(x as usize, y as usize, v);
            }
        }
        Ok(())
    }
}

/// Truncate a string to `max` glyphs by character count (not bytes), so
/// "Naïve" with a 4-char budget stays "Naïv" not "Na?v". When the string
/// is longer than `max` we append an ellipsis `…` (single codepoint, so
/// the visual width stays `max` glyphs).
pub fn fit_glyphs(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let len = s.chars().count();
    if len <= max {
        return s.to_string();
    }
    if max == 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_glyphs_short_returns_unchanged() {
        assert_eq!(fit_glyphs("Hi", 10), "Hi");
    }

    #[test]
    fn fit_glyphs_long_appends_ellipsis() {
        // 5 chars + ellipsis = 6 total, then truncated to (max=5) means
        // we take 4 chars + ellipsis.
        assert_eq!(fit_glyphs("Hello world", 5), "Hell…");
    }

    #[test]
    fn fit_glyphs_unicode_counts_chars_not_bytes() {
        // "Naïve" is 5 chars but ï is 2 bytes — char-based truncation.
        assert_eq!(fit_glyphs("Naïve", 4), "Naï…");
    }

    #[test]
    fn fit_glyphs_zero_budget_returns_empty() {
        assert_eq!(fit_glyphs("anything", 0), "");
    }

    #[test]
    fn framebuffer_clear_writes_full_buffer() {
        let mut fb = Framebuffer128::new();
        fb.clear_rgb(0xFF, 0, 0); // red = 0xF800 LE = [0x00, 0xF8]
        assert_eq!(fb.pixels[0], 0x00);
        assert_eq!(fb.pixels[1], 0xF8);
        assert_eq!(fb.pixels[FRAME_BYTES - 1], 0xF8);
    }
}
