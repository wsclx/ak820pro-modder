# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it leaves beta.

## [Unreleased]

### Planned for 0.6.0
- TFT image upload UI (drag-and-drop GIF / PNG → frame extract → resize / dither → upload).
- Per-key RGB paint-mode editor in the Lighting view.
- Audio-reactive lighting via macOS `ScreenCaptureKit` (system-audio tap + FFT → colour map).

## [0.5.0-beta] — 2026-05-14

First public preview. Five feature phases implemented end-to-end on the AK820 Pro (ISO-DE, firmware 1.07).

### Added
- **Phase 0 — Foundation**: Tauri 2 + React 19 + Vite + Tailwind 3 shell. Rust workspace with `ak820-protocol` library, `ak820-cli` binary, and the Tauri shell crate. HID transport on `usage_page 0xFF68` (control endpoint, 64-byte reports).
- **Phase 1 — Lighting**: 20 effect modes, RGB picker with secondary colour, brightness / speed 0–5, direction (where supported), live apply with debounce.
- **Phase 2 — System**: Device info read-out (firmware, battery, profile slot), sleep-timer presets, game-mode round-trip.
- **Phase 3 — Keymap**: Base + Fn layer editor over a responsive ISO-DE visual surface, 128-slot round-trip via multi-chunk `GET_KEY` / `SET_KEY`, action picker grouped by category (letters / digits / editing / nav / F-keys / modifiers / media / special / macros).
- **Phase 4 — Macros**: Live recorder capturing browser `keydown` / `keyup` events with millisecond delays, action-list editor, 100 slots × 320 bytes, two-phase atomic commit, ActionCatalog integration for assignment to any key.
- **Phase 5a — Per-key RGB (protocol)**: `SET_CUSTOM_LED_DATA` (cmd 36) encoder + decoder, 128 LEDs × 4 B wire layout, `Mode::Custom = 0x80` switch, CLI `ak820 rgb fill / rainbow` hardware-verified.
- **Phase 5b — TFT display (protocol)**: `SET_TFT_USER_ANIMATION` (cmd 80) frame encoder (256-byte header + RGB565 LE pixel stream), bespoke 8-byte per-chunk header, 4096-byte chunk payload, dedicated `Connection::open_tft()` path on the `0xFF67` interface with 4104-byte output reports.
- **Hardening**:
  - Switched `ConnState` from `std::sync::Mutex` to `tokio::sync::Mutex`; every HID-touching Tauri command is now `async fn`. Killed the freeze class observed on System, Macros, and Keymap views.
  - Native macOS menu (App / Edit / View / Window) with `⌘+R`, `⌘⇧R`, `⌘⌥I` shortcuts wired through `tauri::menu::PredefinedMenuItem` and a custom View submenu.
  - Probe poll reads `enumerate()` only (no HID-mutex contention).
  - Project-local `.claude/settings.json` restricts MCPs to `playwright` only, sharply reducing sub-agent prompt size.
- **Documentation**:
  - [`docs/PROTOCOL.md`](docs/PROTOCOL.md) — living byte-level wire docs with AJAZZ JS-source references.
  - [`docs/HANDOFF.md`](docs/HANDOFF.md) — engineering trail, foot-guns, decisions.
  - [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — high-level design + tech-stack rationale.
- **CLI** (`ak820`): `list`, `probe`, `info`, `game-mode get / set-sleep`, `lighting modes / set`, `macros list`, `rgb fill / rainbow`, `hid-descriptors`, `tft solid / cycle / select-index`.
- **26 unit tests** across protocol / frame / keymap / macros / lighting / per-key-RGB / TFT modules.

### Known limitations
- TFT upload runs at wire level but the display still shows the default animation — `SET_TFT_BUILT_IN_INDEX` doesn't appear to switch to user mode on the AK820 Pro firmware we tested. Awaiting USB pcap of the official Windows tool to ground-truth the final activation sequence.
- Per-key RGB has CLI control but no in-app paint UI yet.
- ISO-Enter L-shape renders as a regular rectangle (visual polish only).
- Only the AK820 Pro is supported. Other AJAZZ keyboard models will require their own per-model config.
- Knob (rotary encoder) is firmware-fixed to Volume ± / Mute. Not remappable through the standard protocol — see `docs/HANDOFF.md` § 6.9a for details.

### Documented foot-guns (don't re-discover these)
- Upstream Linux ports (`gohv`, `TaxMachine`) use a wire format the firmware silently ignores on macOS. We RE'd against the official AJAZZ web driver instead.
- `usage_page 0xFF68` is the **control** endpoint; `0xFF67` is the **TFT** endpoint (4104-byte reports). They look identical to hidapi without descriptor inspection.
- Tauri 2 with `devUrl` pointing at Vite hangs WKWebView. Always use `frontendDist` after a static `pnpm build`.
- The Page-type enum has 16 values; `Macro` is **6**, not 4 (4 is `SYSTEM_KEY`). A wrong value silently no-ops.
- Macro wire flags are inverted-looking: `0xB0`/`0x30` is keyboard, `0x90`/`0x10` is mouse.

[Unreleased]: https://github.com/wsclx/ak820pro-modder/compare/v0.5.0-beta...HEAD
[0.5.0-beta]: https://github.com/wsclx/ak820pro-modder/releases/tag/v0.5.0-beta
