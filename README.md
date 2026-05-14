<div align="center">

# AK820 Pro Modder

**Open-source, macOS-first control software for the Epomaker / Ajazz AK820 Pro mechanical keyboard.**

A clean replacement for the Windows-only AJAZZ tool — native, transparent, scriptable.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Status: Beta](https://img.shields.io/badge/status-beta-orange.svg)](#status)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB)](https://tauri.app)
[![Rust](https://img.shields.io/badge/rust-1.82%2B-orange)](https://www.rust-lang.org)
[![Made for macOS](https://img.shields.io/badge/macOS-11%2B-black)](#install)

</div>

---

## Why

The official Epomaker / Ajazz driver is Windows-only and limited. macOS users get nothing — no lighting tweaks, no key remapping, no macros, no TFT customisation. This project changes that, by reverse-engineering the wire protocol against the official online driver and re-implementing the full feature surface in a clean, native, scriptable stack.

| | Official AJAZZ tool | **AK820 Pro Modder** |
|---|---|---|
| macOS support | ❌ Windows only | ✅ Native, signed-build target |
| CLI / scripting | ❌ | ✅ `ak820` headless binary |
| Transparent protocol | ❌ closed | ✅ [`docs/PROTOCOL.md`](docs/PROTOCOL.md) — every byte documented |
| 20 lighting modes | ✅ | ✅ |
| Per-key RGB | ✅ | ✅ click-to-paint surface |
| Keymap remap | ✅ | ✅ base + Fn layer + factory-default reset |
| Macro recorder | ✅ | ✅ live capture in the app |
| TFT image upload | ✅ | 🚧 wire-format decoded, visibility verification in progress |
| **AppleScript / Shortcuts library** | ❌ | ✅ host-side runner with 15 starter examples |
| **Keyboard-triggered automations** | ❌ | ✅ bind AppleScripts to physical keys via F13–F24 markers |
| **Now-Playing reader (Music + Spotify)** | ❌ | ✅ live in the System view |
| **Cross-cutting presets** | ❌ | ✅ 10 curated profiles (Gaming / Dev / Office / Creative / Lifestyle) |
| Audio-reactive lighting | ❌ | 🧪 alpha — pipeline works, real-music smoothness still WIP |
| Now-playing on TFT | ❌ | 🛣 planned (gated on TFT activation RE) |
| Profile sync across machines | ❌ | 🧪 beta (iCloud Drive · automations library, more files coming) |
| Open source | ❌ | ✅ MIT |

## Status

**Version 0.6.0-beta.** Lighting (incl. per-key paint surface), keymap (with factory-default reset), macros, system info, and the host-side automations engine — including a curated 15-entry starter library and one-click cross-cutting presets — are all hardware-verified on the AK820 Pro running firmware 1.07 (**ISO-DE** German QWERTZ layout). TFT upload is protocol-complete and ships a CLI smoke test; the in-app TFT UI is gated on one final reverse-engineering step. See the [Roadmap](#roadmap) section for details.

> ⚠️ **Layout scope.** `v0.6.0-beta` is built **only** for the AK820 Pro **ISO-DE** variant. The wire protocol itself is layout-agnostic, so the lighting / system / per-key-RGB / TFT paths work on every AK820 Pro variant — but the on-screen keyboard surface in the Keymap view will mislabel keys on ANSI, ISO-FR, ISO-ES, ISO-UK, or JIS hardware. **Multi-layout support is a planned roadmap item** ([see below](#roadmap)) — once available, layouts will be cleanly separated under `src/data/layouts/`, never mixed.

> ⚠️ Beta software. Read-only paths are safe; write paths have been used extensively on a single physical device without bricking, but there are no warranties. The keyboard's hidden bootloader (under the spacebar, see hardware notes) is your rescue path if anything ever goes sideways.

## Quick start

### Install (macOS, build from source for now)

```bash
# Prerequisites: Rust 1.82+, Node.js 20+, pnpm
git clone https://github.com/wsclx/ak820pro-modder.git
cd ak820pro-modder
pnpm install
pnpm tauri:build
open src-tauri/target/release/bundle/dmg/*.dmg
```

Drag **AK820 Pro Modder.app** into Applications and launch.

Signed `.dmg` releases will land on the [Releases](https://github.com/wsclx/ak820pro-modder/releases) page once the codesigning pipeline is wired up.

### CLI usage

```bash
cargo build -p ak820-cli --release
./target/release/ak820 list                  # enumerate every HID interface
./target/release/ak820 probe                 # open control endpoint, sanity check
./target/release/ak820 info                  # firmware + battery + profile
./target/release/ak820 lighting set --mode static --color FF00AA
./target/release/ak820 rgb fill --color 00FF80   # per-key static colour
./target/release/ak820 rgb rainbow               # 128-LED rainbow gradient
./target/release/ak820 macros list           # dump every stored macro
./target/release/ak820 hid-descriptors       # debug: report sizes per interface
```

Full CLI help is `ak820 --help`.

## Features

### Lighting
- All 20 effect modes from the official driver (static, breathing, spectrum, ripples, flowing, …)
- RGB picker with HSL preview
- Dual-tone secondary colour for the modes that support it
- Brightness + speed 0–5
- Direction (left / right / up / down) where the mode honours it
- Live apply with debounce; toggle to manual mode for slower-feedback tweaks

### Keymap & layers
- Visual ISO-DE keyboard surface, resizable via window drag (CSS `transform: scale()` for pixel-perfect re-render)
- Click any key → pick a new action → save writes both base + Fn layers atomically
- 128 slots, every keystroke type covered: HID keyboard, consumer (media), mouse, layer toggle, macro trigger, raw-passthrough for unknown classes
- Heads-up warning when remapping F-row keys in macOS-mode (the firmware preempts those with media keys on the physical Mac/Win switch)

### Macros
- Record macros directly in the app — every browser `keydown` / `keyup` becomes a wire event with millisecond delays
- Up to 100 slots × 320 bytes (~79 actions per macro)
- Two-phase atomic commit so a partial write can't leave the keyboard in a weird state
- Assignable to any key from the **Macros** action group in the keymap picker

### System
- Firmware version + battery level + charge state + active profile slot
- Sleep-timer presets (never / 1m / 5m / 10m / 15m / 30m)
- Live device info read-back after every write to confirm the keyboard actually accepted what we sent
- **Now-Playing card** (macOS): live read of Music.app + Spotify desktop, refreshed every 2 s. Foundation for streaming the track to the TFT display once the activation sequence is unblocked.

### Per-key RGB
- Click-to-paint surface that mirrors the ISO-DE keyboard layout
- Brush palette + Fill-all / Clear-all helpers
- Auto-apply with 120 ms debounce so painting feels real-time on the hardware
- Hidden under the Lighting view's `custom` effect mode (`SET_LED_EFFECT` with mode byte `0x80`)

### Automations
- Host-side library of AppleScripts, macOS Shortcuts, and shell commands
- **15 curated starter examples** out of the box across six categories (System, Files, Clipboard, Media, Web, Dev) — no sudo, no external deps, all safe
- Per-entry editor with type-aware payload (textarea for AppleScript / shell, picker for installed Shortcuts)
- Inline run + stdout/stderr/exit-code panel
- Persisted to `~/Library/Application Support/io.github.wsclx.ak820pro-modder/automations.json`
- **Bind any automation to a physical key** in the Keymap view: pick from the `Automations` action group → backend auto-assigns an F13–F24 marker → registers a global hotkey via Carbon `RegisterEventHotKey` → key press fires the script (up to 12 simultaneous bindings, no Accessibility permission needed)

### Presets
- Cross-cutting profiles bundling lighting + sparse keymap overrides + automation seeds
- **10 starter presets** across 5 categories: Gaming (FPS · MMO), Dev (Linux Terminal · Vibe Coder · White Hat), Office (MS365), Creative (Music Production · Writing Focus), Lifestyle (Streaming · Travel battery saver)
- **Additive apply** — patches your current state, doesn't wipe anything. The user opts into each component (lighting / base keymap / Fn keymap / automations) per preset.
- Inline post-apply report showing exactly what changed

### Theming
- Dark + Light themes, both with foreground contrast above WCAG AA on every text-on-surface combination (primaries hit AAA)
- System-pref aware on first launch (`prefers-color-scheme`)
- Manual toggle in the sidebar footer · persisted to `localStorage`
- Driven by CSS custom properties under `data-theme="dark|light"` on `<html>`, so adding a new theme later (e.g. a high-contrast variant) is one CSS block

### Audio-reactive lighting (Alpha — opt-in)
- macOS-only · macOS 13+ · uses Apple's [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit) for the system-audio tap (Screen-Recording permission required, same TCC bucket as the screen-capture API even for audio-only streams)
- Pure-Rust FFT pipeline (`realfft` crate) in `crates/ak820-audio-reactive/` — splits the spectrum into bass / mids / highs bands, applies a Hann window + dB scaling + asymmetric EMA smoothing
- Spectrum preset maps each band to a vertical zone on the keyboard: cols 0-4 red (bass), cols 5-10 green (mids), cols 11-15 blue (highs); gamma + brightness floor so the keyboard's structure stays visible even between beats
- **Known limitations (why this is alpha)**: the wire-level cadence of `SET_CUSTOM_LED_DATA` (10 chunked HID reports per frame) makes the firmware's per-key RGB pipeline visibly stutter on busy music. Frame deduplication helps but doesn't fully eliminate the flicker. Improving this needs a protocol-layer change (skip the per-chunk ACK in `set_many_at` for write-only payloads).
- **Two-stage UI gate**: Lighting view has a "Locked / Unlocked" toggle for the experimental opt-in, then a "Streaming Off / On" toggle for the actual capture. The unlock persists across launches via `localStorage` so contributors don't re-click it, but the casual user can't trip the feature accidentally.

### Coming next
See the [Roadmap](#roadmap) section and the [`docs/HANDOFF.md`](docs/HANDOFF.md) file for the engineering trail.

## Architecture

```
ak820pro-modder/
├── crates/
│   ├── ak820-protocol/      Pure-Rust library — HID framing, command encoders/decoders
│   │   └── src/commands/    One module per feature family (lighting, keymap, macros, …)
│   └── ak820-cli/           `ak820` headless binary (scripting + RE / smoke tests)
├── src-tauri/               Tauri 2 shell — async commands marshal between UI and protocol crate
├── src/                     React 19 + TypeScript + Tailwind 3 frontend
│   ├── views/               One file per top-level tab (Connect, Lighting, Keymap, Macros, …)
│   ├── components/          Shared UI primitives (Card, Button, Slider, …)
│   └── data/                Static layout descriptors + curated action catalogues
├── docs/
│   ├── PROTOCOL.md          Living byte-level wire documentation
│   ├── ARCHITECTURE.md      High-level design + tech stack rationale
│   ├── HANDOFF.md           Engineering handoff: foot-guns, decisions, debug trail
│   └── reverse-engineering/ Scraped vendor bundles + USB pcap captures (gitignored)
└── tests/                   Hardware-in-the-loop fixtures (gitignored)
```

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) explains why Tauri + Rust over Electron / PyQt / WebHID.

## Hardware reference

| Component | Part | Notes |
|---|---|---|
| Main MCU | HFD80CP100 (Sonix SN32F299 clone) | 6 × 15 key matrix |
| Wireless | WCH CH582F | BLE 5.1 + 2.4 GHz, I²C to MCU |
| Flash | PY25Q128HA | 16 MB SPI — firmware + configs + GIF frames |
| Display | NFP085B-10AF, 0.85″ 128 × 128 | GC9107 controller, SPI |
| Bootloader | Hidden pins under spacebar | ISP-mode VID/PID `0x0C45 / 0x7140` |
| Operating VID/PID | `0x0C45 / 0x8009` (wired + 2.4 GHz) | `0xFEFE` for BT, control on HID interface 2 (`usage_page 0xFF68`) |
| **Tested layout** | **ISO-DE** (German QWERTZ) | The wire protocol is layout-agnostic; only the rendered keyboard surface in the Keymap view is layout-specific. ANSI / ISO-FR / ISO-ES / ISO-UK / JIS variants exist for the AK820 Pro and are on the multi-layout roadmap. |

Credit to the [fpb/ajazz-ak820-pro](https://github.com/fpb/ajazz-ak820-pro) hardware-doc project for the early MCU / wireless / flash identification — see [Acknowledgements](#acknowledgements).

## Roadmap

| Phase | Status | Description |
|---|---|---|
| 0 — Foundation | ✅ | Tauri shell, workspace, HID transport, probe handshake |
| 1 — Lighting | ✅ | 20 modes + secondary colour + brightness / speed / direction |
| 2 — System | ✅ | Device info, battery, sleep timer, profile, game-mode |
| 3 — Keymap | ✅ | Base + Fn layer editor, visual ISO-DE surface, 128-slot round-trip, factory-default reset |
| 4 — Macros | ✅ | Recorder, editor, ActionCatalog integration, hardware-verified |
| 5a — Per-key RGB | ✅ | Protocol + CLI + click-to-paint UI in the Lighting view |
| 5b — TFT display | 🟡 | Protocol + CLI upload verified at wire level; visibility flip pending USB pcap of the official Windows tool |
| 6a — Now-Playing reader | ✅ | macOS Music.app + Spotify desktop, surfaced in System view |
| 6b — Automations engine | ✅ | AppleScript / Shortcut / Shell library, 15 curated starters, keyboard-side triggers via F13–F24 markers |
| 6c — Cross-cutting presets | ✅ | 10 curated profiles across Gaming / Dev / Office / Creative / Lifestyle |
| 6d — Audio-reactive lighting | 🛣 | ScreenCaptureKit system-audio tap + FFT → colour map |
| 6e — Now-playing on TFT | 🛣 | Gated on Phase 5b — stream the Now-Playing reader's snapshot to the keyboard's display |
| 6f — iCloud profile sync | 🛣 | Plist + macros + automations roundtripped through `~/Library/Mobile Documents` so multiple Macs share one config |
| 7 — Cross-platform | 🛣 | Windows + Linux builds via GitHub Actions |
| 8 — Multi-layout | 🛣 | ANSI, ISO-FR, ISO-ES, ISO-UK, JIS variants. Cleanly isolated under `src/data/layouts/` with a layout-picker UI. The wire protocol already works for every variant; this is purely about the on-screen keyboard surface. |

## Contributing

This is an open project. We need:

- 🧪 **Hardware testers** — `v0.6.0-beta` is built for **ISO-DE only**. If you own an ANSI / ISO-FR / ISO-ES / ISO-UK / JIS AK820 Pro, the wire protocol still works (lighting, system, per-key RGB, TFT), but the Keymap surface will be mislabelled. Capture your physical layout into a new file under `src/data/layouts/` — see the layouts directory's `index.ts` for the three-step add-a-layout recipe.
- 🕵️ **Reverse engineers** — USB pcap captures of the official Windows tool doing specific actions (especially TFT upload and the per-key RGB enable path).
- 🦀 **Rust developers** — protocol modules, error handling, additional decoders.
- ⚛️ **Frontend developers** — UI for TFT image upload, per-key RGB paint mode, audio-reactive visualisation.
- 📝 **Docs writers** — better install instructions per platform.

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) before opening a PR. Issues are open — see the [issue templates](.github/ISSUE_TEMPLATE) for the kind of structured info that makes a bug report or protocol finding actually actionable.

By contributing you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Hardware control surfaces are inherently risky. Read [`SECURITY.md`](SECURITY.md) for our disclosure policy and the rescue procedures (ISP bootloader, factory reset over HID).

## Acknowledgements

- [@gohv](https://github.com/gohv) — [EPOMAKER-Ajazz-AK820-Pro](https://github.com/gohv/EPOMAKER-Ajazz-AK820-Pro) for the original Linux Rust port. Inspired this project's architecture; its lighting findings turned out to be wrong on macOS (the official online driver uses a different wire format) but the encoder skeleton was a useful starting point.
- [@TaxMachine](https://github.com/TaxMachine) — [ajazz-keyboard-software-linux](https://github.com/TaxMachine/ajazz-keyboard-software-linux) for the C++ pcap-parser approach and continued protocol RE.
- [@fpb](https://github.com/fpb) — [ajazz-ak820-pro](https://github.com/fpb/ajazz-ak820-pro) for hardware identification (MCU, wireless chip, flash, display).
- The [SonixQMK](https://github.com/SonixQMK) project — keeping a path open to eventual QMK firmware support for the SN32F299 family.

## License

[MIT](LICENSE) © wsclx

---

<sub>v0.6.0-beta · made with ❤️ for the macOS mechanical-keyboard community · open issues / PRs at [github.com/wsclx/ak820pro-modder](https://github.com/wsclx/ak820pro-modder)</sub>
