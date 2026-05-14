# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it leaves beta.

## [Unreleased]

### Added — alpha
- **Audio-reactive lighting (alpha)** — macOS-only · macOS 13+. New workspace crate `ak820-audio-reactive` wraps Apple's ScreenCaptureKit for system-audio capture and runs a Hann-windowed real FFT (`realfft` 3.5) → bass / mids / highs bands with dB scaling and asymmetric EMA smoothing. New "Spectrum" preset paints the three bands across the keyboard's vertical zones (red / green / blue). New `audio_reactive_start` / `_stop` / `_status` Tauri commands, new "Audio-reactive" card in the Lighting view with a **two-stage opt-in**: a `Locked / Unlocked` toggle that persists in `localStorage` (so contributors don't re-click), and a `Streaming Off / On` toggle that's disabled until unlocked. The card carries an `Alpha` badge and an `Experimental — use at your own risk` block. Known issues: visible flicker on real music because the 10-chunk HID transfer per frame outruns the firmware's per-key RGB pipeline; frame deduplication helps for the silence case but the real-music smoothness needs a protocol-layer change. New `ak820 audio meter` CLI subcommand prints band magnitudes + ASCII bars for smoke-testing the pipeline.

### Added — beta
- **iCloud-Drive profile sync (beta)** — automations library round-trips through `~/Library/Mobile Documents/com~apple~CloudDocs/ak820pro-modder/`. Plain user-visible folder in Finder (no app-entitlement / provisioning-profile gymnastics, OSS-friendly). New thin Rust transport in `src-tauri/src/icloud_sync.rs` with three Tauri commands: `icloud_sync_status` (probe + remote mtime), `icloud_sync_push` (local → iCloud), `icloud_sync_pull` (iCloud → local, only when iCloud copy is strictly newer). New "iCloud Sync" card in the System view: toggle persists `ak820:icloud-sync-enabled` in `localStorage`, manual `Pull from iCloud` / `Push to iCloud` / `Refresh status` buttons always available. When the toggle is on: app-mount auto-pulls (handled in `App.tsx` before any view fetches), and `Automations` view auto-pushes after every successful save. Conflict resolution is last-write-wins by file mtime; per-record ID-based merge is a follow-up.

Planned for 0.7.x
- TFT image upload UI (drag-and-drop GIF / PNG → frame extract → resize / dither → upload). Gated on Phase 5b3 — USB pcap of the official Windows tool's activation sequence.
- Audio-reactive lighting smoothness pass — likely skip the per-chunk `read_response` in `set_many_at` for write-only payloads so the HID pipeline can keep up at >15 fps.
- iCloud Sync expansion: per-record ID-based merge so two machines editing different automations simultaneously don't lose one side; also sync custom-LED paint snapshots and theme/setting preferences.
- Browser-tab media support in Now-Playing (currently only Music.app + Spotify desktop).

## [0.6.0-beta] — 2026-05-14

Major feature release. Five new top-level capabilities land — Per-key RGB paint surface, macOS Now-Playing reader, Automations library + keyboard-side triggers, cross-cutting Presets, full Light + Dark theming — plus a Factory Defaults rescue path in the Keymap view and a WCAG-AA contrast pass over the entire UI.

### Added
- **Per-key RGB paint-mode editor** in the Lighting view. Selecting the `custom` mode shows a clickable keyboard surface; click-to-paint, brush palette, fill / clear helpers, debounced auto-apply. Wraps the wire-level `set_custom_led` + `apply_lighting(mode=custom)` sequence so the firmware actually renders the buffer.
- **macOS Now-Playing reader**: card in the System view polls Music.app + Spotify desktop every 2 s and shows title / artist / album. Foundation for streaming the track to the TFT display once Phase 5b3 unblocks.
- **Automations library**: new top-level tab. User defines AppleScript / macOS Shortcut / shell-command entries, persisted as JSON in `~/Library/Application Support/io.github.wsclx.ak820pro-modder/`. Run-button executes host-side and shows stdout / stderr / exit-code inline.
- **15-entry starter library** in the Automations tab. First-launch users adopt curated examples (system / files / clipboard / media / web / dev) with one click instead of an empty screen.
- **Keyboard-side automation triggers**: pick "Automations" in the Keymap action picker, click an entry → backend auto-binds an F13–F24 marker via `tauri-plugin-global-shortcut` (Carbon `RegisterEventHotKey` under the hood — no Accessibility permission needed). The selected physical key now fires the automation host-side. Up to 12 automations can be keyboard-triggered simultaneously. Caps bound to automation markers show the automation name on the keyboard surface rather than the raw F-key label.
- **Factory Defaults button** in the Keymap & Knob view header. Pulls the firmware's stored default keymap for the active layer (via `GET_DEFAULT_KEY_MATRIX` cmd 31 / `GET_DEFAULT_FN_KEY_MATRIX` cmd 28) and stages it into the draft. Save commits — a misclick stays in draft state so Discard cleanly undoes it.
- **Presets tab**: new top-level nav entry with 10 curated cross-cutting profiles bundling lighting + sparse keymap overrides + automation seeds. Categories: Gaming (FPS · MMO), Dev (Linux Terminal · Vibe Coder · White Hat), Office (MS365), Creative (Music Production · Writing Focus), Lifestyle (Streaming · Travel battery saver). Each preset shows component badges before apply; the apply modal exposes per-component checkboxes so the user only takes the parts they want. Inline success banner reports exactly what changed. Applied additively — existing automations with the same name are skipped, not overwritten.
- **Light theme** with `prefers-color-scheme` detection, manual Sun / Moon toggle in the sidebar footer, persistence in `localStorage`.

### Changed
- **Foreground contrast pass**: every `fg-*` and `line-*` token lifted to pass WCAG AA on its primary surface (primaries now hit AAA). `fg-3` was previously 2.7 : 1 — a fail for normal text — and is now 4.7 : 1. The whole token system was refactored onto CSS custom properties so the Light theme could re-use the same Tailwind utilities without per-class `dark:` overrides.
- **Type scale**: `xs` from 11.5 px → 12 px, `2xs` from 10.5 px → 11 px, `sm` 13 → 13.5, `base` 14 → 14.5. Detail rows in modals and cards are no longer squinty.

### Documentation
- README, CHANGELOG, PROTOCOL, HANDOFF all swept to match the shipping feature set. HANDOFF gains two new foot-gun entries (§ 6.9c on F13–F24 marker keys silently capturing system-wide, § 6.9d on the macos-14 `cargo` → `rustup-init` shadow that bit CI).

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
- **Physical layout: ISO-DE only.** The Keymap surface in the app is hard-coded to the German ISO (QWERTZ) variant. The wire protocol is layout-agnostic, so lighting / system / per-key RGB / TFT paths work on every AK820 Pro variant — but ANSI / ISO-FR / ISO-ES / ISO-UK / JIS hardware will see mislabelled keys in the Keymap view. Multi-layout support is on the roadmap; the architecture (`src/data/layouts/`) is already prepared to host additional layouts cleanly separated.
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

[Unreleased]: https://github.com/wsclx/ak820pro-modder/compare/v0.6.0-beta...HEAD
[0.6.0-beta]: https://github.com/wsclx/ak820pro-modder/releases/tag/v0.6.0-beta
[0.5.0-beta]: https://github.com/wsclx/ak820pro-modder/releases/tag/v0.5.0-beta
