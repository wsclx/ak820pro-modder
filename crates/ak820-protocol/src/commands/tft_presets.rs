//! Curated catalogue of 128 × 128 TFT animations, generated programmatically.
//!
//! Every preset returns a fully-encoded [`TftAnimation`] ready to hand to
//! `Connection::set_tft_user_animation()`. Frames are computed on demand
//! (no embedded asset bytes) so the binary stays small and contributors
//! can tweak a single function to redesign a preset without re-rolling
//! any image pipelines.
//!
//! ## Adding a new preset
//!
//! 1. Implement a `fn name_of_preset() -> TftAnimation` that builds one or
//!    more frames via [`solid_frame`] / [`gradient_frame`] / [`pattern_frame`]
//!    or a custom inline pixel generator.
//! 2. Add an entry to [`ALL_PRESETS`] with id, display name, description,
//!    and a constructor pointer.
//! 3. Cover it with at least one assert in the unit tests (frame count,
//!    first-pixel sanity, etc.). The frame *content* is hard to assert
//!    cheaply — a visual check on hardware is the real gate.

use super::tft::{TftAnimation, TftFrame, FRAME_BYTES, PIXELS_PER_FRAME, TFT_HEIGHT, TFT_WIDTH};
use serde::Serialize;

/// Lightweight catalogue entry. The frontend lists these in the TFT view's
/// picker. The `frame_count` + `total_ms` fields are pre-computed so the
/// UI can render duration hints without instantiating the animation.
#[derive(Debug, Clone, Serialize)]
pub struct TftPresetInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub frame_count: usize,
    pub total_ms: u32,
}

/// Build the animation for `id`. Returns `None` for unknown ids.
pub fn build(id: &str) -> Option<TftAnimation> {
    let entry = ALL_PRESETS.iter().find(|p| p.id == id)?;
    Some((entry.build)())
}

/// Metadata snapshot for every shipping preset. Hand-written so the list
/// stays in roadmap-curation order (test colours first, then static
/// gradients, then animations).
pub fn catalogue() -> Vec<TftPresetInfo> {
    ALL_PRESETS
        .iter()
        .map(|p| {
            let anim = (p.build)();
            let frame_count = anim.frames.len();
            let total_ms: u32 = anim.frames.iter().map(|f| f.delay_ms as u32).sum();
            TftPresetInfo {
                id: p.id,
                display_name: p.display_name,
                description: p.description,
                frame_count,
                total_ms,
            }
        })
        .collect()
}

struct PresetEntry {
    id: &'static str,
    display_name: &'static str,
    description: &'static str,
    build: fn() -> TftAnimation,
}

/// The shipping preset catalogue. Order is the UI display order — diagnostic
/// presets first (so a contributor verifying a fresh AK820 Pro sees them
/// before the decorative animations), then test colours, then static
/// decoration, then animations from simple → busy.
static ALL_PRESETS: &[PresetEntry] = &[
    PresetEntry {
        id: "diagnostic-quadrants",
        display_name: "Diagnostic · Quadrants",
        description: "4 corners coloured red/green/blue/yellow with a 1 px black grid \
             every 16 px and a centred 8 px white cross. Tells you visually \
             whether the display renders the full 128 × 128 frame, which way \
             is 'up', and roughly where the centre of the visible area sits. \
             First preset to try on a new hardware build.",
        build: diagnostic_quadrants,
    },
    PresetEntry {
        id: "diagnostic-border",
        display_name: "Diagnostic · Border",
        description: "4 px white border around the 128 × 128 perimeter, otherwise pitch \
             black. If any edge of the border is missing or clipped, our pixel \
             stream isn't reaching that part of the panel.",
        build: diagnostic_border,
    },
    PresetEntry {
        id: "magenta-solid",
        display_name: "Magenta",
        description: "Single full-screen magenta frame. Use this to verify the display \
             accepts your upload — if the TFT goes pink the wire-protocol is fine.",
        build: magenta_solid,
    },
    PresetEntry {
        id: "cyan-solid",
        display_name: "Cyan",
        description: "Single full-screen cyan frame. Companion test colour to Magenta.",
        build: cyan_solid,
    },
    PresetEntry {
        id: "rainbow-horizontal",
        display_name: "Rainbow",
        description: "Static horizontal rainbow gradient (red → magenta) across the \
             full width. Good first 'pretty' preset and exercises the full \
             RGB565 quantisation path.",
        build: rainbow_horizontal,
    },
    PresetEntry {
        id: "sunset-vertical",
        display_name: "Sunset",
        description: "Top-down vertical gradient from deep blue through magenta to warm \
             orange. Calmer than the rainbow, looks like a sunset reflection.",
        build: sunset_vertical,
    },
    PresetEntry {
        id: "color-cycle-slow",
        display_name: "Color Cycle (slow)",
        description: "6-frame ROYGBIV-ish cycle at 800 ms per frame. Solid colours \
             with a soft fade implied by the firmware's frame interpolation.",
        build: color_cycle_slow,
    },
    PresetEntry {
        id: "color-cycle-fast",
        display_name: "Color Cycle (fast)",
        description: "12-frame finer-grained cycle at 250 ms per frame. More flicker, more energy.",
        build: color_cycle_fast,
    },
    PresetEntry {
        id: "pulse-cyan",
        display_name: "Pulse · Cyan",
        description: "8-frame brightness-pulsing cyan, 150 ms per frame. Reads as a \
             gentle breathe at full saturation.",
        build: pulse_cyan,
    },
    PresetEntry {
        id: "pulse-magenta",
        display_name: "Pulse · Magenta",
        description: "Companion to the cyan pulse in the opposite half of the spectrum.",
        build: pulse_magenta,
    },
    PresetEntry {
        id: "scanline-vertical",
        display_name: "Scanline",
        description: "10-frame vertical scanline sweeping top to bottom over a deep-purple \
             background. Reads like a retro CRT loading screen.",
        build: scanline_vertical,
    },
    PresetEntry {
        id: "checkerboard-strobe",
        display_name: "Checkerboard Strobe",
        description: "4-frame inverted checkerboard at 300 ms per frame. Useful for \
             checking refresh rate + pixel-perfect alignment on the GC9107 \
             controller.",
        build: checkerboard_strobe,
    },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert an RGB888 triple to a 2-byte little-endian RGB565 pixel.
#[inline]
fn pixel_rgb565(r: u8, g: u8, b: u8) -> [u8; 2] {
    let r5 = (r >> 3) as u16;
    let g6 = (g >> 2) as u16;
    let b5 = (b >> 3) as u16;
    let v = (r5 << 11) | (g6 << 5) | b5;
    v.to_le_bytes()
}

/// Build a single solid-colour frame with the given delay.
fn solid_frame(r: u8, g: u8, b: u8, delay_ms: u16) -> TftFrame {
    let pixel = pixel_rgb565(r, g, b);
    let mut pixels = Vec::with_capacity(FRAME_BYTES);
    for _ in 0..PIXELS_PER_FRAME {
        pixels.extend_from_slice(&pixel);
    }
    TftFrame { pixels, delay_ms }
}

/// Build a frame by calling `gen(x, y)` for each pixel — returns `(r, g, b)`.
fn build_frame<F: Fn(u32, u32) -> (u8, u8, u8)>(gen: F, delay_ms: u16) -> TftFrame {
    let mut pixels = Vec::with_capacity(FRAME_BYTES);
    for y in 0..TFT_HEIGHT {
        for x in 0..TFT_WIDTH {
            let (r, g, b) = gen(x, y);
            pixels.extend_from_slice(&pixel_rgb565(r, g, b));
        }
    }
    TftFrame { pixels, delay_ms }
}

/// Saturated HSV → RGB. `h` in degrees [0..360), `s` and `v` in [0..1].
/// We hand-roll instead of pulling a colour-conversion crate — this is
/// the only place RGB565 generation needs colour-space math.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (rp, gp, bp) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_byte = |f: f32| -> u8 { ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8 };
    (to_byte(rp), to_byte(gp), to_byte(bp))
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

fn diagnostic_quadrants() -> TftAnimation {
    // Four colour quadrants with 1 px black grid every 16 px and an 8 px
    // white plus sign at the centre. Picking 16 px = a power of 2 makes
    // it easy to count divisions if the display turns out to be a
    // different resolution than expected.
    let frame = build_frame(
        |x, y| {
            // Grid lines first — supersede everything else.
            if x % 16 == 0 || y % 16 == 0 {
                return (0x10, 0x10, 0x10);
            }
            // Centre cross: 8 px wide bar at x or y middle ±4 px.
            let cx = TFT_WIDTH as i32 / 2;
            let cy = TFT_HEIGHT as i32 / 2;
            let dx = (x as i32 - cx).abs();
            let dy = (y as i32 - cy).abs();
            if (dx < 4 && dy < (TFT_HEIGHT as i32 / 2 - 4))
                || (dy < 4 && dx < (TFT_WIDTH as i32 / 2 - 4))
            {
                return (0xFF, 0xFF, 0xFF);
            }
            // Quadrant colour. (0,0) is top-left.
            let right = x >= TFT_WIDTH / 2;
            let bottom = y >= TFT_HEIGHT / 2;
            match (right, bottom) {
                (false, false) => (0xC0, 0x20, 0x20), // TL red
                (true, false) => (0x20, 0xC0, 0x20),  // TR green
                (false, true) => (0x20, 0x40, 0xC0),  // BL blue
                (true, true) => (0xC0, 0xA0, 0x10),   // BR yellow / amber
            }
        },
        200,
    );
    TftAnimation {
        frames: vec![frame],
    }
}

fn diagnostic_border() -> TftAnimation {
    const BORDER: u32 = 4;
    let frame = build_frame(
        |x, y| {
            if x < BORDER || y < BORDER || x >= TFT_WIDTH - BORDER || y >= TFT_HEIGHT - BORDER {
                (0xFF, 0xFF, 0xFF)
            } else {
                (0x00, 0x00, 0x00)
            }
        },
        200,
    );
    TftAnimation {
        frames: vec![frame],
    }
}

fn magenta_solid() -> TftAnimation {
    TftAnimation {
        frames: vec![solid_frame(0xFF, 0x00, 0xFF, 200)],
    }
}

fn cyan_solid() -> TftAnimation {
    TftAnimation {
        frames: vec![solid_frame(0x00, 0xFF, 0xFF, 200)],
    }
}

fn rainbow_horizontal() -> TftAnimation {
    let frame = build_frame(
        |x, _y| {
            let hue = x as f32 * (300.0 / TFT_WIDTH as f32); // 0..300° (red → magenta)
            hsv_to_rgb(hue, 1.0, 1.0)
        },
        200,
    );
    TftAnimation {
        frames: vec![frame],
    }
}

fn sunset_vertical() -> TftAnimation {
    // Hand-picked stops to read as a sunset rather than a hue sweep.
    // [deep navy at top, magenta in the middle, warm orange at the bottom]
    let stops: [(u8, u8, u8); 3] = [(0x10, 0x14, 0x60), (0xC0, 0x40, 0x80), (0xFF, 0xA0, 0x40)];
    let frame = build_frame(
        |_x, y| {
            let t = y as f32 / (TFT_HEIGHT - 1) as f32; // 0..1
                                                        // Two-segment lerp through 3 stops.
            let (a, b, mix) = if t < 0.5 {
                (stops[0], stops[1], t * 2.0)
            } else {
                (stops[1], stops[2], (t - 0.5) * 2.0)
            };
            let lerp = |c0: u8, c1: u8| -> u8 { (c0 as f32 * (1.0 - mix) + c1 as f32 * mix) as u8 };
            (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
        },
        200,
    );
    TftAnimation {
        frames: vec![frame],
    }
}

fn color_cycle_slow() -> TftAnimation {
    let palette: [(u8, u8, u8); 6] = [
        (0xFF, 0x40, 0x40), // red
        (0xFF, 0xA0, 0x30), // orange
        (0xF0, 0xE0, 0x40), // yellow
        (0x40, 0xE0, 0x50), // green
        (0x40, 0xA0, 0xFF), // blue
        (0xB0, 0x40, 0xFF), // violet
    ];
    let frames = palette
        .iter()
        .map(|&(r, g, b)| solid_frame(r, g, b, 800))
        .collect();
    TftAnimation { frames }
}

fn color_cycle_fast() -> TftAnimation {
    // 12 evenly-spaced hues, full saturation.
    let frames = (0..12)
        .map(|i| {
            let hue = i as f32 * (360.0 / 12.0);
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            solid_frame(r, g, b, 250)
        })
        .collect();
    TftAnimation { frames }
}

fn pulse_at_hue(hue: f32) -> TftAnimation {
    // 8 frames: brightness 0.2 → 1.0 → 0.2 (half-cosine).
    let frames = (0..8)
        .map(|i| {
            let phase = i as f32 / 8.0;
            let v = 0.2 + 0.8 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos());
            let (r, g, b) = hsv_to_rgb(hue, 1.0, v);
            solid_frame(r, g, b, 150)
        })
        .collect();
    TftAnimation { frames }
}

fn pulse_cyan() -> TftAnimation {
    pulse_at_hue(180.0)
}

fn pulse_magenta() -> TftAnimation {
    pulse_at_hue(300.0)
}

fn scanline_vertical() -> TftAnimation {
    // Deep purple background with a moving 6-row bright line.
    const BACKGROUND: (u8, u8, u8) = (0x20, 0x08, 0x40);
    const LINE: (u8, u8, u8) = (0xC0, 0xFF, 0xFF);
    const LINE_HEIGHT: u32 = 6;
    let frames = (0..10)
        .map(|i| {
            let scan_y = (i * (TFT_HEIGHT / 10)) as i32; // moves top→bottom
            build_frame(
                |_x, y| {
                    let dy = (y as i32 - scan_y).rem_euclid(TFT_HEIGHT as i32);
                    if dy >= 0 && dy < LINE_HEIGHT as i32 {
                        LINE
                    } else {
                        BACKGROUND
                    }
                },
                160,
            )
        })
        .collect();
    TftAnimation { frames }
}

fn checkerboard_strobe() -> TftAnimation {
    // 4 frames: A, B, A, B. Patterns: 8×8 cells.
    const CELL: u32 = 16;
    let make = |invert: bool| -> TftFrame {
        build_frame(
            |x, y| {
                let cell = ((x / CELL) + (y / CELL)) & 1;
                let lit = (cell == 1) ^ invert;
                if lit {
                    (0xFF, 0xFF, 0xFF)
                } else {
                    (0x00, 0x00, 0x00)
                }
            },
            300,
        )
    };
    TftAnimation {
        frames: vec![make(false), make(true), make(false), make(true)],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_returns_twelve_entries() {
        let c = catalogue();
        assert_eq!(
            c.len(),
            12,
            "12 = 2 diagnostic + 10 decorative; bump this when adding presets"
        );
    }

    #[test]
    fn every_preset_id_is_unique() {
        let c = catalogue();
        let mut seen = std::collections::HashSet::new();
        for p in &c {
            assert!(seen.insert(p.id), "duplicate preset id: {}", p.id);
        }
    }

    #[test]
    fn every_preset_builds_a_valid_animation() {
        for p in catalogue() {
            let anim = build(p.id).unwrap_or_else(|| panic!("no animation for id {}", p.id));
            assert!(!anim.frames.is_empty(), "{} has zero frames", p.id);
            assert_eq!(anim.frames.len(), p.frame_count);
            for (i, f) in anim.frames.iter().enumerate() {
                assert_eq!(
                    f.pixels.len(),
                    FRAME_BYTES,
                    "{} frame {} has wrong byte count",
                    p.id,
                    i
                );
            }
        }
    }

    #[test]
    fn animation_encodes_round_trips_for_every_preset() {
        // The encode path is the only one that actually reaches the wire,
        // so it has to succeed for every preset.
        for p in catalogue() {
            let anim = build(p.id).unwrap();
            let buf = anim.encode().unwrap_or_else(|e| {
                panic!("preset {} failed to encode: {:?}", p.id, e);
            });
            // Header + N frame bodies.
            let expected_len = 256 + anim.frames.len() * FRAME_BYTES;
            assert_eq!(buf.len(), expected_len, "preset {} encoded size", p.id);
        }
    }

    #[test]
    fn solid_frame_has_uniform_pixel_value() {
        // Reach into a known preset and spot-check that solid_frame really
        // outputs the same 2 bytes everywhere.
        let anim = magenta_solid();
        let pixels = &anim.frames[0].pixels;
        let first = [pixels[0], pixels[1]];
        for chunk in pixels.chunks_exact(2) {
            assert_eq!(chunk, &first);
        }
    }
}
