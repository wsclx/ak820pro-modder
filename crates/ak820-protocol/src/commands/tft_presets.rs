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
        id: "mandala",
        display_name: "Mandala",
        description: "Static centred 8-fold symmetric radial pattern in deep \
             purple / cyan / magenta. Looks like a kaleidoscope frozen at one \
             rotation; pleasant ambient look.",
        build: mandala,
    },
    PresetEntry {
        id: "matrix-rain",
        display_name: "Matrix Rain",
        description: "Classic green falling-glyph effect on a black background. \
             16 columns of trails with varying speeds, bright-green heads fading \
             to dark-green tails. 16 frames at 90 ms.",
        build: matrix_rain,
    },
    PresetEntry {
        id: "plasma",
        display_name: "Plasma",
        description: "Sum-of-sines plasma effect cycling through the full hue \
             spectrum. 20 frames at 70 ms. Looks like a 1990s demoscene intro.",
        build: plasma,
    },
    PresetEntry {
        id: "starfield",
        display_name: "Starfield",
        description: "Stars warping outward from the centre, hyperspace-style. \
             ~80 stars with radial velocity. 18 frames at 80 ms.",
        build: starfield,
    },
    PresetEntry {
        id: "wave",
        display_name: "Wave",
        description: "Cyan sine wave drifting horizontally across deep navy, with \
             a softly glowing crest. 20 frames at 80 ms — calm, oceanic.",
        build: wave,
    },
    PresetEntry {
        id: "spiral",
        display_name: "Spiral",
        description: "Rotating multi-arm spiral in magenta / cyan / yellow. 18 \
             frames at 80 ms, smooth rotation. Hypnotic.",
        build: spiral,
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

// ---------------------------------------------------------------------------
// Eye-candy presets — replaced the v0.7 Color Cycle / Pulse / Scanline /
// Checkerboard set, which Mario judged "sinnlose Frame-Animationen". The
// new presets target the same 128 × 128 panel but produce content the
// whole frame uses, so the upper-half-rendering bug (still unresolved as
// of v0.8) is observable on every preset, not just one or two.
// ---------------------------------------------------------------------------

/// Centred 8-fold-symmetric radial pattern. Static — one frame, 250 ms.
/// Built by computing polar coordinates (r, θ) for every pixel and
/// mapping `(r, θ × 8)` to HSV → RGB. The 8-fold symmetry makes the
/// pattern read as a kaleidoscope tile rather than a noisy gradient.
fn mandala() -> TftAnimation {
    let cx = TFT_WIDTH as f32 / 2.0 - 0.5;
    let cy = TFT_HEIGHT as f32 / 2.0 - 0.5;
    let max_r = (cx * cx + cy * cy).sqrt();
    let frame = build_frame(
        |x, y| {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let theta = dy.atan2(dx); // -π..π
                                      // 8-fold symmetry: fold θ into [0..π/8) and reflect.
            let folded = (theta * 4.0 / std::f32::consts::PI).rem_euclid(2.0);
            let folded = if folded > 1.0 { 2.0 - folded } else { folded };
            // Hue from radius: cycles roughly 2× across the panel for two
            // colour rings.
            let hue = (r / max_r * 720.0 + folded * 60.0) % 360.0;
            let v = (1.0 - r / max_r).max(0.0).powf(0.6);
            // Soft inner core: bright near the centre.
            let v = (v + 0.15).min(1.0);
            hsv_to_rgb(hue, 0.9, v)
        },
        250,
    );
    TftAnimation {
        frames: vec![frame],
    }
}

/// "Matrix Rain" — green falling glyph trails on a black background.
///
/// 16 columns × 16 frames. Each column has a deterministic offset and
/// speed (seeded from the column index) so the visual is repeatable
/// across runs but every column still looks independent. Trails are 28
/// pixels long, fading from bright cyan-green at the head to dark
/// green at the tail. No glyph rasteriser yet — the "drops" are 8 px
/// wide columns of solid colour. Reads as Matrix-ish at 0.85" viewing.
fn matrix_rain() -> TftAnimation {
    const COLS: u32 = 16;
    const COL_W: u32 = TFT_WIDTH / COLS;
    const FRAMES: usize = 16;
    const TRAIL: u32 = 28;
    const HEAD: (u8, u8, u8) = (0xB0, 0xFF, 0xC0);
    const TAIL: (u8, u8, u8) = (0x00, 0x30, 0x10);

    // Per-column metadata: (initial_offset, pixels-per-frame velocity).
    // Mul-by-prime + modulus keeps the columns visually independent
    // without an RNG dependency.
    let cols: Vec<(u32, u32)> = (0..COLS)
        .map(|c| {
            let off = c.wrapping_mul(73) % (TFT_HEIGHT + TRAIL);
            let speed = 4 + (c.wrapping_mul(11) % 6); // 4..10 px/frame
            (off, speed)
        })
        .collect();

    let frames = (0..FRAMES)
        .map(|f| {
            // Snapshot head-y for each column at this frame.
            let heads: Vec<i32> = cols
                .iter()
                .map(|&(off, sp)| {
                    let raw = off + sp * f as u32;
                    (raw % (TFT_HEIGHT + TRAIL * 2)) as i32 - TRAIL as i32
                })
                .collect();
            build_frame(
                |x, y| {
                    let col = (x / COL_W) as usize;
                    if col >= COLS as usize {
                        return (0, 0, 0);
                    }
                    let head_y = heads[col];
                    let dy = head_y - y as i32; // positive = pixel is above head
                    if dy < 0 || dy >= TRAIL as i32 {
                        return (0, 0, 0);
                    }
                    let t = dy as f32 / TRAIL as f32;
                    lerp_rgb(HEAD, TAIL, t)
                },
                90,
            )
        })
        .collect();
    TftAnimation { frames }
}

/// Classic sum-of-sines plasma. Each pixel gets a hue from the sum of
/// four sinusoids parameterised by position + time; saturation stays
/// at full so the result reads as a morphing colour gradient.
fn plasma() -> TftAnimation {
    const FRAMES: usize = 20;
    let frames = (0..FRAMES)
        .map(|f| {
            let t = f as f32 / FRAMES as f32 * std::f32::consts::TAU;
            build_frame(
                |x, y| {
                    let fx = x as f32 / TFT_WIDTH as f32;
                    let fy = y as f32 / TFT_HEIGHT as f32;
                    // Four sinusoids in different directions, time-shifted.
                    let v = (fx * 8.0 + t).sin()
                        + (fy * 8.0 + t * 1.3).cos()
                        + ((fx + fy) * 6.0 + t * 0.7).sin()
                        + (((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt() * 16.0 + t * 1.7).cos();
                    // Map v ∈ ~[-4, 4] to hue ∈ [0..360).
                    let hue = ((v + 4.0) / 8.0 * 360.0).rem_euclid(360.0);
                    hsv_to_rgb(hue, 0.9, 1.0)
                },
                70,
            )
        })
        .collect();
    TftAnimation { frames }
}

/// Starfield warp — ~80 stars positioned in polar coordinates, each
/// frame advances their radius linearly. Stars wrap around to the
/// centre once they exit the panel. Star brightness fades with radius
/// so the centre stays a bit clearer than the edges.
fn starfield() -> TftAnimation {
    const FRAMES: usize = 18;
    const STAR_COUNT: usize = 80;
    let cx = TFT_WIDTH as f32 / 2.0 - 0.5;
    let cy = TFT_HEIGHT as f32 / 2.0 - 0.5;
    let max_r = (cx * cx + cy * cy).sqrt();

    // Pre-compute star angles + initial radii deterministically.
    let stars: Vec<(f32, f32, f32)> = (0..STAR_COUNT)
        .map(|i| {
            let i = i as u32;
            let theta = (i.wrapping_mul(2654435761).wrapping_rem(1000) as f32 / 1000.0)
                * std::f32::consts::TAU;
            let r0 = (i.wrapping_mul(1597).wrapping_rem(1000) as f32 / 1000.0) * max_r;
            let speed = 1.2 + (i.wrapping_mul(31).wrapping_rem(20) as f32 / 20.0) * 2.5;
            (theta, r0, speed)
        })
        .collect();

    let frames = (0..FRAMES)
        .map(|f| {
            // Each star's current radius this frame.
            let positions: Vec<(f32, f32, f32)> = stars
                .iter()
                .map(|&(theta, r0, speed)| {
                    let r = (r0 + speed * f as f32) % max_r;
                    let px = cx + theta.cos() * r;
                    let py = cy + theta.sin() * r;
                    (px, py, r / max_r)
                })
                .collect();
            build_frame(
                |x, y| {
                    // Pure black background plus per-star contribution. Each
                    // star is a 1-pixel "core" so accumulation is unnecessary
                    // — just paint stars at integer rounded positions.
                    for &(px, py, t) in &positions {
                        if (px.round() as u32 == x) && (py.round() as u32 == y) {
                            let brightness = (1.0 - (t - 0.2).clamp(0.0, 1.0) * 0.6).max(0.4);
                            let v = (brightness * 255.0) as u8;
                            return (v, v, v);
                        }
                    }
                    (0, 0, 0)
                },
                80,
            )
        })
        .collect();
    TftAnimation { frames }
}

/// Sine-wave drifting horizontally across a deep navy background, with
/// a soft cyan glow at the crest. Animation moves the wave one cycle
/// across in 20 frames at 80 ms = 1.6 s loop.
fn wave() -> TftAnimation {
    const FRAMES: usize = 20;
    const BG: (u8, u8, u8) = (0x05, 0x0B, 0x20);
    const CREST: (u8, u8, u8) = (0xA0, 0xF0, 0xFF);
    const GLOW: (u8, u8, u8) = (0x20, 0x60, 0xA0);
    let frames = (0..FRAMES)
        .map(|f| {
            let phase = f as f32 / FRAMES as f32 * std::f32::consts::TAU;
            build_frame(
                |x, y| {
                    let fx = x as f32 / TFT_WIDTH as f32 * std::f32::consts::TAU * 2.0;
                    let crest_y = TFT_HEIGHT as f32 / 2.0 + (fx + phase).sin() * 20.0;
                    let dy = (y as f32 - crest_y).abs();
                    if dy < 1.0 {
                        CREST
                    } else if dy < 6.0 {
                        let t = (dy - 1.0) / 5.0;
                        lerp_rgb(CREST, GLOW, t)
                    } else if dy < 18.0 {
                        let t = (dy - 6.0) / 12.0;
                        lerp_rgb(GLOW, BG, t)
                    } else {
                        BG
                    }
                },
                80,
            )
        })
        .collect();
    TftAnimation { frames }
}

/// Three-arm rotating spiral in magenta / cyan / yellow. Each pixel's
/// colour comes from `((θ - r×k + t) × arms) % 1` which produces
/// continuous rotating arms regardless of resolution.
fn spiral() -> TftAnimation {
    const FRAMES: usize = 18;
    const ARMS: f32 = 3.0;
    const TIGHTNESS: f32 = 8.0;
    let cx = TFT_WIDTH as f32 / 2.0 - 0.5;
    let cy = TFT_HEIGHT as f32 / 2.0 - 0.5;
    let max_r = (cx * cx + cy * cy).sqrt();
    let frames = (0..FRAMES)
        .map(|f| {
            let t = f as f32 / FRAMES as f32 * std::f32::consts::TAU;
            build_frame(
                |x, y| {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let r = (dx * dx + dy * dy).sqrt() / max_r;
                    let theta = dy.atan2(dx);
                    // Arm position cycles 0..1 across each arm.
                    let arm = ((theta + r * TIGHTNESS + t) * ARMS / std::f32::consts::TAU)
                        .rem_euclid(1.0);
                    // Three hues at 0°, 120°, 240° → magenta, cyan, yellow-ish.
                    let hue = arm * 360.0 + 300.0; // start at magenta
                    let v = 1.0 - r.min(1.0) * 0.4; // brighter near centre
                    hsv_to_rgb(hue, 0.9, v)
                },
                80,
            )
        })
        .collect();
    TftAnimation { frames }
}

/// Linear interpolation between two RGB triples. `t = 0` → `a`,
/// `t = 1` → `b`. Used by Matrix Rain head→tail fade and the Wave
/// glow falloff.
fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| ((x as f32) * (1.0 - t) + (y as f32) * t) as u8;
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
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
