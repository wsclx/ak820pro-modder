# AK820 Pro Project — Handoff Document

Single source of truth for any future session picking this project up.
Read this start-to-finish before touching code.

---

## 1. Vision

Build the **definitive macOS-first control software for the Epomaker /
Ajazz AK820 Pro mechanical keyboard**. The official AJAZZ tool is
Windows-only, ugly, and limited. We want a fast, beautiful, Tauri-based
app that surpasses it on every axis (visual design, feature coverage,
reliability, cross-platform support).

Stretch goals (Phase 6): things the official tool can't do — audio-
reactive lighting, now-playing on the TFT, AppleScript bridge, iCloud
profile sync.

---

## 2. Hardware Facts (do not re-discover)

| Field | Value | Source |
|---|---|---|
| Product | AK820 Pro (test unit: ISO-DE, firmware **1.07**) | device GET_DEVICE_INFO |
| MCU | **HFD80CP100** (Sonix SN32F299 clone), 6×15 key matrix | fpb/ajazz-ak820-pro |
| Wireless | WCH **CH582F** (BLE 5.1 + 2.4 GHz, I²C-attached) | same |
| Flash | PY25Q128HA 16 MB SPI | same |
| TFT | NFP085B-10AF, 128×128, **GC9107** over SPI | same |
| USB VID | **0x0C45** (Sonix Technology Co. Ltd.) | live IOHIDDevice enumeration |
| USB PID | **0x8009** (USB / 2.4 GHz mode), `0xFEFE` (Bluetooth), `0x7140` (ISP/bootloader) | live enumeration |
| Macro space | **3072 bytes** (device reports), 512 hardcoded fallback in AJAZZ tool | GET_DEVICE_INFO |
| TFT capacity | 255 frames | GET_DEVICE_INFO |

### HID interface that actually accepts our writes

macOS exposes the AK820 Pro as **9 HID endpoints**. The right one for
control commands is:

- **Interface**: `2` (Sonix `iface` numbering on macOS)
- **Usage page**: **`0xFF68`** (and only that — `0xFF67` opens fine but
  silently drops writes)
- **Path**: e.g. `DevSrvsID:4295011008`

Detection rule (see `crates/ak820-protocol/src/device.rs` and
`src-tauri/src/lib.rs`):

```rust
candidates.iter().find(|d| d.usage_page == 0xFF68)
```

---

## 3. The Wire Protocol (decoded)

### Source of truth

The official AJAZZ online driver at <https://ajazz.driveall.cn>. We
saved its main module to `docs/reverse-engineering/online-driver/default-protocol.js`.

**Do not trust** the upstream Linux ports
(`gohv/EPOMAKER-Ajazz-AK820-Pro`, `TaxMachine/ajazz-keyboard-software-linux`).
They use a completely different framing (Feature Reports + START/FINISH
wrapper + wrong report ID) that the firmware silently ignores on macOS.
A full diff is in `docs/PROTOCOL.md`.

### Outgoing frame (64 bytes via `device.write` / WebHID `sendReport(0, …)`)

```
0   1   2          3     4     5     6     7   8..63
[0xAA, cmd, len_or_type, addr_lo, addr_hi, opt0, opt1_or_lastFlag, opt2, payload…]
```

Response frames are identical except byte 0 is **`0x55`** (incoming magic).

### Command bytes (subset; full list in `docs/PROTOCOL.md`)

| Name | Value | Notes |
|---|---|---|
| `COMMUNICATION_START` | 1 | not strictly required for our commands |
| `COMMUNICATION_END` | 2 | |
| `SET_FACTORY_RESET` | 15 | arg `0xFF` = full reset |
| `GET_DEVICE_INFO` | **16 (0x10)** | 48-byte response |
| `GET_GAME_MODE` | **17 (0x11)** | 56-byte response |
| `GET_KEY` | **18 (0x12)** | 512-byte (128 × 4) multi-packet read |
| `GET_LED_EFFECT` | 19 (0x13) | 16-byte response |
| `GET_CUSTOM_LED_DATA` | 20 | 512 bytes per-key RGB |
| `GET_MACRO` | 21 | |
| `GET_FN_KEY` | **22 (0x16)** | 512-byte FN-layer keymap |
| `SET_GAME_MODE` | **33 (0x21)** | 56-byte payload |
| `SET_KEY` | **34 (0x22)** | 512-byte multi-packet write |
| `SET_LED_EFFECT` | **35 (0x23)** | 16-byte payload |
| `SET_FN_KEY` | **38 (0x26)** | |
| `SET_TFT_USER_ANIMATION` | 80 | for Phase 5 |
| `GET_DEFAULT_KEY_MATRIX` | 31 | factory keymap (for revert) |

### Payload layouts

**`SET_LED_EFFECT` (16 bytes):**
```
0: mode (0–0x13)        // see Mode enum
1: red                  // 0-255
2: green
3: blue
4: 0xFF                 // driverSetting, hardcoded
5: secondaryRed         // for dual-tone modes
6: secondaryGreen
7: secondaryBlue
8: colorMode            // 0 = mono, >0 = palette variants
9: brightness           // 0-5
10: speed               // 0-5
11: direction           // 0=L, 1=D, 2=U, 3=R
12: effectModeType
13: 0 (padding)
14: 0xAA                // checkCodeL (high-byte first vs upstream — non-obvious!)
15: 0x55                // checkCodeH
```

**`GET_DEVICE_INFO` (48 bytes):**
```
0  : romSize
2-3: macroSpaceSize (LE u16)
4-5: vid
6-7: pid
8-9: firmware version (BCD-ish: ((e[8]&15) + ((e[8]&240)>>4)*10 + e[9]*100) / 100)
17 : batteryLevel
18 : chargeStatus (1 = charging)
19 : currentProfile
22-23: tftMaxFrames
30 : frameVersion
…
```

**`GET_GAME_MODE` / `SET_GAME_MODE` (56 bytes):**
```
1: gameMode
2: fnSwitch
3: sleepTime            // index 0–5 — see SLEEP_PRESETS
4: keyDelay
5: reportRate
6: systemMode
7: tftDisplayTime
8: topDeadZone * 100
9: bottomDeadZone * 100
11: stabilityMode
14: autoCalibration
15: singleKeyWakeup
```

**Keymap slot (4 bytes per slot, 128 slots, 512 bytes total):**

Each slot is a tagged 4-byte action. `byte 0` is the **pageType** /
action class (see `crates/ak820-protocol/src/commands/keymap.rs`):

| pageType | Variant | Encoding |
|---|---|---|
| 0 | Default (use factory) | `[0, 0, 0, 0]` |
| 1 | Mouse | `[1, button, value, 0]` |
| **2** | **Keyboard (standard HID)** | `[2, 0, hid_usage, 0]` |
| 3 | Consumer (media) | `[3, value_lo, value_hi, 0]` |
| 4 | Macro | raw |
| 5 | CB | raw |
| 8 | DKS | `[8, value, 0, 0]` |
| 10 | TGL (layer) | `[10, value, 0, 0]` |
| 13 | FUNC (24-bit BE) | `[13, b2, b1, b0]` |
| 14 | END | raw |
| 15 | MPT | raw |
| ≥0x80 | FUNC_V2 | bit-7 of byte 0 set, packs two 16-bit params |

### Multi-packet GET / SET

`GET_KEY` / `SET_KEY` / `GET_FN_KEY` etc. carry 512 bytes — they're
chunked. Per packet: 8-byte header + 56-byte payload chunk. Number of
chunks = `ceil(content_size / 56)`. The transport loop in
`Connection::get_many` / `set_many` mirrors the JS `C()` function from
the online driver.

Each request gets one response; the last request has byte 6 = 1 (last-
packet flag).

---

## 4. Repository Layout

```
ak820pro-modder/
├── README.md                       <- public-facing pitch + feature matrix
├── CHANGELOG.md                    <- Keep-a-Changelog
├── CONTRIBUTING.md                 <- how-to-help + add-a-layout recipe
├── CODE_OF_CONDUCT.md
├── SECURITY.md
├── LICENSE                         <- MIT © wsclx
├── .github/                        <- issue templates, PR template, CI
├── docs/
│   ├── HANDOFF.md                  <- this file — read first as a maintainer
│   ├── ARCHITECTURE.md             <- layered breakdown
│   ├── PROTOCOL.md                 <- byte-level protocol reference
│   ├── INSTALL.md
│   └── reverse-engineering/
│       ├── README.md               <- local-only convention + SHA-256 ledger
│       ├── CAPTURE_GUIDE.md        <- 30-min UTM/USBPcap recipe for new contributors
│       ├── online-driver/          <- vendor bundles (gitignored)
│       │   ├── default/            <- ajazz.driveall.cn  (covers ISO-DE)
│       │   ├── iso-fr/             <- a-jazz-fr.driveall.cn
│       │   └── iso-es/             <- a-jazz-es.driveall.cn
│       ├── tools/                  <- vendor exes (gitignored)
│       └── captures/               <- USB pcaps (gitignored)
├── crates/
│   ├── ak820-protocol/             <- Rust core library (no UI)
│   │   └── src/
│   │       ├── lib.rs              <- VID, PID, control interface
│   │       ├── error.rs
│   │       ├── protocol.rs         <- build_frame, command bytes, MAGIC_*
│   │       ├── device.rs           <- Connection, enumerate, get_many,
│   │       │                          set_many, all command APIs
│   │       └── commands/
│   │           ├── lighting.rs     <- Mode (incl. Custom 0x80), Direction,
│   │           │                      LightingConfig, encode
│   │           ├── system.rs       <- DeviceInfoReport, GameMode, sleep presets
│   │           ├── keymap.rs       <- KeyAction enum (incl. Macro page 6),
│   │           │                      Keymap, 4-byte codec
│   │           ├── macros.rs       <- Macro recorder format (cmd 21 / 37)
│   │           ├── per_key_rgb.rs  <- 128-LED custom buffer (cmd 36 + mode 0x80)
│   │           ├── tft.rs          <- TFT animation encode + chunk header
│   │           │                      (cmd 80; activation pending pcap)
│   │           ├── sleep.rs        <- stub
│   │           └── clock.rs        <- stub
│   └── ak820-cli/
│       └── src/main.rs             <- `ak820` binary: list, probe, lighting,
│                                       info, game-mode, macros list,
│                                       rgb fill/rainbow, hid-descriptors,
│                                       tft solid/cycle/select-index
├── src-tauri/
│   ├── Cargo.toml                  <- adds tauri-plugin-global-shortcut
│   ├── tauri.conf.json             <- frontendDist=../dist, no devUrl;
│   │                                  identifier io.github.wsclx.ak820pro-modder
│   ├── build.rs
│   ├── entitlements.plist          <- USB-HID + automation entitlements
│   ├── capabilities/default.json   <- shell:allow-open + global-shortcut:default
│   ├── icons/
│   └── src/
│       ├── lib.rs                  <- Tauri commands + ConnState (tokio::Mutex)
│       │                              + native macOS menu + About dialog
│       │                              + global-shortcut listener for marker
│       │                              HIDs 104..115 → automation dispatch
│       ├── now_playing.rs          <- JXA probe of Music.app + Spotify
│       ├── automations.rs          <- host-side library (CRUD + run + persist)
│       ├── starter_library.rs      <- 15 curated AppleScript/Shortcut/Shell
│       │                              examples shipped with the app
│       └── presets.rs              <- 10 curated cross-cutting profiles
├── src/                            <- React 19 + Vite + TS
│   ├── main.tsx
│   ├── App.tsx                     <- tab routing, probe polling, nav
│   ├── index.css                   <- Tailwind base + theme tokens
│   ├── version.ts                  <- APP_VERSION + APP_AUTHOR single source
│   ├── types.ts
│   ├── components/
│   │   ├── ui.tsx                  <- Card, Button, Badge, KVList, Slider,
│   │   │                              Toggle, BatteryBar, ErrorBanner, Mono
│   │   ├── Layout.tsx              <- Sidebar + page chrome + lucide icon
│   │   │                              re-exports + sidebar credit footer
│   │   └── NowPlayingCard.tsx      <- live Music/Spotify card (Phase 6)
│   ├── views/
│   │   ├── Connect.tsx             <- HID enumeration
│   │   ├── Lighting.tsx            <- 20 effect modes + 21st (custom)
│   │   ├── CustomLightingPaint.tsx <- click-to-paint per-key RGB surface
│   │   ├── System.tsx              <- firmware, battery, sleep, game-mode
│   │   │                              (+ NowPlayingCard)
│   │   ├── Keymap.tsx              <- visual ISO-DE editor + action picker
│   │   │                              (incl. dynamic Macros + Automations
│   │   │                              groups + Factory Default button)
│   │   ├── Macros.tsx              <- recorder + editor + slot list
│   │   ├── Automations.tsx        <- AppleScript / Shortcut / Shell CRUD
│   │   │                              + starter library picker + run output
│   │   └── Presets.tsx             <- 10-profile picker + apply modal
│   └── data/
│       ├── layouts/
│       │   ├── types.ts            <- PhysicalKey + KeyboardLayout + LayoutId
│       │   ├── index.ts            <- registry, default, resolveLayout()
│       │   ├── iso-de.json         <- ISO-DE physical positions (only one
│       │   │                          v0.5.0-beta ships)
│       │   └── iso-de.ts           <- typed wrapper
│       ├── hid-usage-names.ts      <- HID code → human label
│       └── action-catalog.ts       <- picker groups + Action union type
├── index.html
├── vite.config.ts                  <- HMR disabled (see Learning #2)
├── tailwind.config.ts              <- OKLCH-tuned tokens
├── tsconfig.json
├── package.json
├── Cargo.toml                      <- workspace root
├── rust-toolchain.toml             <- pinned to 1.82+
└── tests/fixtures/
```

---

## 5. Phase / Feature Status

| Phase | Status | What works | Notes |
|---|---|---|---|
| 0 — Foundation | ✅ done | Tauri 2 app, workspace, CLI, device probe | live-validated on test hardware |
| RE — Decode wire format | ✅ done | All command bytes, frame layout, encoders/decoders | from online-driver source |
| 1 — Lighting | ✅ done | 20 modes, color, secondary, direction, brightness/speed, debounced auto-apply | LEDs visibly respond on hardware |
| 2 — System | ✅ done | Firmware, battery, profile, macro space, frame version, sleep-timer set+verify | full round-trip |
| Design Pass v2 | ✅ done | Linear/Raycast-style sidebar, Lucide icons, OKLCH tokens, no Geist (caused issues) | dark surfaces with rim-light |
| Polish + Connect UX | ✅ done | Adaptive polling (2s/8s), Reconnect button, hotplug-recovery, number formatting, "AK820 Pro" label, native ⌘+R menu | sequential reads in System view to avoid mutex contention |
| 3 — Keymap read | ✅ done | Full ISO-DE visual layout, Base + Fn layer, knob/TFT/LED visual placeholders, **responsive scaling 0.55x–1.5x via ResizeObserver+CSS transform** | reads via multi-packet GET_KEY |
| 3 — Keymap edit | ✅ done | Click-to-select, action picker (letters, digits, F-keys, editing, navigation, modifiers, media, factory-default), Factory Default button (cmd 31 / 28), dirty-state badge, save+verify, discard | tested with Caps→F12 round-trip |
| 4 — Macros | ✅ done | Two-phase encoded write (cmd 21/37), recorder UI, ActionCatalog group, ISO-DE keyboard binding | wire flags inverted-looking — see § 6.8 |
| 5a — Per-key RGB | ✅ done | `SET_CUSTOM_LED_DATA` (cmd 36), 128-LED × 4 B buffer, `Mode::Custom = 0x80`, click-to-paint UI in Lighting view | CLI hardware-verified |
| 5b — TFT display | 🟡 protocol-only | `SET_TFT_USER_ANIMATION` (cmd 80), 4104-byte chunks on iface 3 (0xFF67), bespoke 8-byte chunk header, RGB565 LE pixels, CLI `tft solid/cycle` writes | upload reaches device but display still shows default animation — pending USB pcap of the official Windows tool for the activation sequence |
| 6a — Now-Playing reader | ✅ done | JXA probe of Music.app + Spotify desktop every 2 s, surfaced in System view | Phase-6 preview for the TFT pipeline |
| 6b — Automations engine | ✅ done | AppleScript / Shortcut / Shell library with 15 curated starters, persistence to `$APP_DATA/automations.json`, run-with-output panel, keyboard-side triggers via F13–F24 markers (Carbon RegisterEventHotKey via `tauri-plugin-global-shortcut`) | up to 12 keyboard-bindable automations at once |
| 6c — Cross-cutting presets | ✅ done | 10 curated profiles across Gaming / Dev / Office / Creative / Lifestyle, additive apply with per-component opt-in | sparse keymap overrides + automation seeds resolved against starter library |
| 6d — Audio-reactive lighting | 🧪 alpha | `crates/ak820-audio-reactive` — ScreenCaptureKit (`macos_13_0`) + `realfft` 3.5 → bass/mids/highs → 3-zone Spectrum preset on the per-key grid. Two-stage Alpha unlock in Lighting view (Locked/Unlocked + Streaming Off/On), `localStorage`-persisted. CLI smoke probe `ak820 audio meter`. | Real-music smoothness still flickery — wire-level cadence of `set_custom_led` (10 HID chunks per frame) saturates the firmware pipeline at ~15 fps. Frame deduplication kills the silence-flicker; faster-than-15-fps needs a protocol-layer change (skip per-chunk `read_response` in `set_many_at`). |
| 6e — Now-playing on TFT | ⏳ pending | — | gated on Phase 5b activation sequence |
| 6f — iCloud profile sync | 🧪 beta | `src-tauri/src/icloud_sync.rs` — thin transport: detect `$HOME/Library/Mobile Documents/com~apple~CloudDocs/`, push/pull `ak820pro-modder/automations.json` with last-write-wins by mtime. Tauri commands `icloud_sync_status/_push/_pull`. SyncCard in System view with toggle + manual buttons. App-mount auto-pull, save-hook auto-push. Hermetic unit tests inject the iCloud root so CI without iCloud still passes. | Per-record ID-based merge + custom-LED snapshots + settings sync are 0.7.x follow-ups. |

### Open visual TODOs

- ISO-Enter L-shape (currently rendered as a normal 1.5u key)
- Per-key RGB editor (the 128-LED `customLedData` from JSON / `SET_CUSTOM_LED_DATA` 36)
- Wheel/knob configurable actions (online driver has a `wheelKeys`
  structure — needs separate decoding)
- Real LED indicators (Caps, Win-Lock, Battery) instead of static dots

---

## 6. Critical Learnings (foot guns the next session would otherwise hit)

These are saved in `/Users/mario/.claude/projects/-Users-mario-DEV-ak820pro/memory/MEMORY.md`
plus inline comments at the call sites. Don't re-discover them.

### 6.1 The upstream Linux ports are wrong on macOS
`gohv/EPOMAKER-Ajazz-AK820-Pro` and `TaxMachine/...` build packets the
AK820 firmware **silently ignores** on macOS:
- They use HID **Feature Reports** (`hid_send_feature_report`). The
  firmware wants **Output Reports** (`device.write` / WebHID `sendReport`).
- They wrap every command in `START (0x18) / preamble / data / FINISH
  (0xF0)`. Real protocol: one self-contained frame with magic `0xAA`.
- They confuse GET and SET command bytes (`0x13` is GET_LED_EFFECT, the
  WRITE is `0x23`).
- They write the check-code bytes in the opposite order (`0x55 0xAA`
  vs the real `0xAA 0x55`).

**Source of truth = the official AJAZZ online driver**
(<https://ajazz.driveall.cn>). Re-do RE by fetching its bundles and
grepping for `SET_*` / `GET_*` if you ever doubt.

### 6.2 Tauri 2 + Vite dev-server hangs in WKWebView
On macOS, loading the frontend from `http://localhost:5173` (Vite dev)
hangs the WKWebView indefinitely. Symptoms: black/white window,
spinner, "Programm reagiert nicht".

**Always** use `frontendDist: "../dist"` and run `pnpm build` as the
`beforeDevCommand` in `tauri.conf.json`. HMR is off in `vite.config.ts`
for the same reason.

Trade-off: every frontend change requires `pnpm build` + ⌘+R to see.
Acceptable.

### 6.3 `std::sync::Mutex<Connection>` + concurrent Tauri commands = deadlock
Tauri 2 sync commands run on a worker pool. Two simultaneous invokes
that both grab a `std::Mutex` block the worker threads; under load the
runtime freezes (white screen, Mac beachball).

**Mitigations in place:**
- `probe_device` is **read-only** — it just enumerates, never opens
  the persistent connection.
- The frontend **never** uses `Promise.all` on HID-touching commands —
  System view loads `get_device_info` → `get_game_mode` sequentially.
- App-level polling only calls `probe_device`; view-level reads only
  fire on mount.

**Hardening landed in 0.5.0-beta:** `ConnState` now uses
`tokio::sync::Mutex` and every HID-touching Tauri command is
`async fn`. The lock yields the *task*, not the worker thread.

### 6.4 Tauri 2 needs an explicit native macOS menu for ⌘+R
Out of the box there's no menu, so ⌘+R does nothing and ⌘+Alt+I
doesn't open DevTools. We register App / Edit / View / Window menus
in `src-tauri/src/lib.rs::setup()` with `PredefinedMenuItem` for
quit/edit and custom `MenuItem::with_id` entries for reload + devtools.

### 6.5 Selecting the right HID interface on macOS
hidapi reports `interface` numbers per HID descriptor, not per USB
interface. **Always filter by `usage_page == 0xFF68`** — that's the
endpoint that actually accepts output reports for control commands.
`0xFF67` opens fine but writes go nowhere.

### 6.6 Responsive keyboard surface
Don't try to perfectly recreate the ISO-DE layout via flexbox math.
Use a `ResizeObserver` + CSS `transform: scale()` wrapper (see
`Keymap.tsx::ResponsiveScale`). Pixel-perfect, no rounding drift,
identical between WKWebView and Chromium.

`Layout.tsx` accepts a `wide` prop so the Keymap view can opt out of
the standard 960 px content cap and use the full window.

### 6.7 Mac-mode F-row keys preempt base-layer remaps
The keyboard has a physical Mac / Win toggle on the back. The AJAZZ
web driver does **not** expose any software equivalent — `fnSwitch`
(byte 2 of the game-mode struct) is a passive mirror of the hardware
switch. We tested writing into it but in practice the user has to
flick the physical slider.

What this means for `SET_KEY`: in **Mac mode**, the F-row's *base*
layer is firmware-preempted by media keys (brightness ±, volume ±,
mission control, etc.) regardless of what we write to slots 1..12.
The Fn layer is still freely remappable; Fn+F-key fires whatever the
Fn-layer slot contains. **Workaround**: assign F-row macros to the
**Fn layer**, not the base layer, when the keyboard is in Mac mode.
Letter keys (J, etc.) and the rest of the matrix are fully remappable
on both layers.

We hit this during Phase-4 hardware testing: F12 = Macro-M1 on base
layer fired nothing in Mac mode; pressing the same key on Win mode
(or assigning on Fn layer) replayed the macro correctly.

### 6.8 Macro wire flags are inverted-looking
`actionType` 3 = keyboard (wire flags `0xB0`/`0x30`), `actionType` 1 = mouse
(wire flags `0x90`/`0x10`). The numeric ordering invites mistakes —
0x90 looks like "kbd" because the high nibble starts at 1, but it's
actually a mouse press. A wrong mapping turns a "type H" macro into
"right-click+left-click+button-4 down" because keyCode 11 = 0b1011 is
read as a mouse button mask. Hardware-confirmed on fw 1.07. See
`docs/PROTOCOL.md` § action-flags for the full table.

### 6.9a Knob is firmware-fixed on the AK820 Pro
The protocol bundle has a `wheelKeys` array path in both encoder and store
(`Me()` builds it from `keyList`), but inspecting every device config in
`layout-default-DElMT--A.js` (productIds 0x8006…0x8800 incl. all `AK820*`
variants) shows that **no device populates a `wheelKeys: [...]` block**.
Confirmed by string-search of the entire bundle — `wheelKeys:[` literal
appears zero times. The AJAZZ web driver also has no UI for knob remap.
Practical conclusion: the AK820 Pro's knob (Volume±/Mute press) is wired
firmware-internally, NOT through the standard `SET_KEY` slot array, and
therefore cannot be reassigned through this protocol. If knob remap ever
becomes a priority, the only paths are (a) firmware-level RE (Sonix
toolchain) or (b) empirical probing of unused slot numbers (13–15, 93–96,
109–127) by writing a distinct test action and watching what fires.

### 6.9b Physical layout: ISO-DE verified, others unverified
The AK820 Pro ships in at least five regional variants (ISO-DE, ANSI,
ISO-FR, ISO-ES, ISO-UK, JIS). **Five layouts now ship**:

- **ISO-DE** — ✅ hardware-verified against Mario's keyboard
  (firmware 1.07).
- **ANSI, ISO-UK, ISO-ES, ISO-FR** — 🧪 **unverified**. Hand-derived
  from the ISO-DE export plus public AK820 Pro / 75 % conventions.
  No real hardware was used to confirm slot ↔ key assignment. Users
  on these variants get correctly-shaped legends but should report
  visual mismatches.
- **JIS** — still roadmap-only. Japanese boards have additional
  physical keys (Henkan, Muhenkan, Kana) and a different bottom-row
  count that doesn't map cleanly onto the slot allocation we
  inferred from the ISO-DE firmware export. Needs hardware.

The **wire protocol is layout-agnostic** — slot numbers are
firmware-internal addresses, identical across variants. So lighting,
system commands, per-key RGB, and TFT upload all work on every AK820
Pro regardless of which layout file the UI is rendering. The Keymap
view + Custom-Lighting paint surface are the only places where the
layout descriptor matters; both consume the active layout via
`useLayout()` (`src/data/layouts/use-layout.ts`).

The sidebar footer now has a layout `<select>` picker. The user's
choice persists to `localStorage["ak820:layout"]`.

**Architectural rule** (do not break): layout-aware branches go into
`src/data/layouts/<id>.{json,ts}` ONLY. The Keymap view renders
uniformly from whichever `KeyboardLayout` the registry returns;
adding `if (layoutId === 'ansi')` branches in views is forbidden.

When the AJAZZ Windows app or other AK820 Pro variants come into the
RE picture — like the v1.0.0.5 Win driver, which is the ANSI build —
**never** mix that variant's keycap data into the ISO-DE descriptor.
The wire-protocol findings are still valid (the protocol is
layout-agnostic), but copy any layout-specific positions into a
*separate* layout file.

### 6.9c Automation marker keys must never overlap user-remapped F-keys
The Automations subsystem reserves HID 104..115 (F13–F24) as global-hotkey
markers. `tauri-plugin-global-shortcut` (Carbon `RegisterEventHotKey`)
captures these keystrokes system-wide — they don't reach the focused app.
That's deliberate (we want the keyboard to trigger automations, not also
type F19 into Notepad), but it means a user who remaps a physical key to
F19 for some unrelated reason will find F19 silently swallowed. Hence the
auto-allocator picks the LOWEST free marker on each new binding, leaves
markers attached to deleted automations only until the next save.

If a user complains "F-something doesn't type anymore", check the
Automations tab for a binding using that marker.

### 6.9d macOS-14 CI shadowed `cargo` with `rustup-init` (two layers)
**Layer 1 (PATH shadow):** On macos-14 ARM runners, `setup-node` and
`pnpm/action-setup` re-prepend `/opt/homebrew/bin` to `PATH` *after* the
Rust toolchain install. Brew's `cargo` symlink at `/opt/homebrew/bin/cargo`
resolves to `rustup-init`, which has no `metadata` subcommand and emits
"unexpected argument" — exactly what `tauri build`'s internal `cargo
metadata` call surfaces.

**Layer 2 (cache poison):** Swatinem/rust-cache@v2 caches `~/.cargo/bin/`
by default. If a previous run ever leaked rustup-init into that directory,
every subsequent run *restores* the bad binary on top of the freshly-installed
cargo from `dtolnay/rust-toolchain`. `which cargo` still returns
`~/.cargo/bin/cargo` (the "right" path!) but the binary itself is rustup-init.
This is what broke the 0.6.0-beta release run despite the PATH fix being
in place.

Fix in the workflow: pass `cache-bin: false` + `prefix-key: v1-rust` to
Swatinem/rust-cache, and run a `Verify Rust toolchain` step that fails
loudly if `cargo --version` ever contains "rustup-init" again. Cache
delete via `gh cache delete <id>` if you suspect poisoning.

### 6.9f Stale HID handle survives unplug — auto-reconnect lives in `ConnState::with`
After a USB unplug, hidapi keeps returning "Device is disconnected" for
every subsequent call against the **cached** `Connection` in
`ConnState`. The handle is dead, but the slot still says `Some`, so
re-plugging doesn't help on its own — every action keeps hitting the
zombie handle.

The fix is centralised in `src-tauri/src/lib.rs::ConnState::with()`:
if a *cached* connection returns an error whose message contains
`disconnected`, `Device not found`, or `HID error`, the slot is cleared
and the closure runs **once more**. The retry re-enters `ensure_open()`
which re-enumerates and opens fresh. So after a re-plug the very next
user action (Refresh, View switch, anything) succeeds transparently —
no need for a dedicated Reconnect button or two clicks.

The retry is gated by `had_cached_conn` so a *first* `open_control()`
failure on an empty slot doesn't double-pay latency when the device
genuinely isn't there. All closures in the file take their inputs by
reference (`&keymap`, `&map`, …), so `FnMut` is sound; if you ever add
a `with(move |slot| …)` that consumes an owned value, you'll get a
loud "value moved" compile error to force you to clone or rethink.

`apply_lighting` used to carry its own hand-rolled retry loop. That's
been removed — the generic retry in `ConnState::with` covers it now,
and keeping per-command loops would diverge in subtle ways.

### 6.9e Frontend error banners showing "[object Object]"
The Rust `AppError` is `#[derive(Serialize)]` with
`#[serde(tag = "kind", content = "message")]`, so Tauri rejects with an
*object* `{ kind: "Protocol", message: "..." }`. Doing `setErr(String(e))`
on that object yields the literal string `"[object Object]"` — which is
what System, Macros, and Keymap views were showing on every failure.
Always go through `src/errors.ts → formatError(e)` in view code, never
`String(e)`. Lint-rule candidate but currently policed by code review.

### 6.9g Swift runtime rpath dance for `ak820-audio-reactive`
`screencapturekit 2.1.0` uses a Swift static lib (via swift-bridge) that
references `@rpath/libswift_Concurrency.dylib`. Three traps stacked on
each other:

1. **screencapturekit's own build.rs assumes full Xcode.** It computes
   `$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib/...`.
   On Command-Line-Tools-only machines (the default for most contributors),
   `xcode-select -p` returns `/Library/Developer/CommandLineTools`, which
   has no `Toolchains/` subdir, so the path is invalid and *no* rpath gets
   baked. The binary fails to load with `dyld: Library not loaded …
   (Reason: no LC_RPATH's found)`.

2. **`cargo:rustc-link-arg` from a library doesn't propagate to dependent
   binary crates.** Putting the rpath fix in `ak820-audio-reactive/build.rs`
   alone leaves the CLI / Tauri binaries unaffected. Each binary crate that
   transitively depends on `screencapturekit` needs its *own* build.rs.

3. **Pointing rpath at the on-disk CLT/Xcode toolchain causes duplicate
   loads on macOS 13+.** Apple frameworks (AVFoundation, CoreAudio, …)
   reference `/usr/lib/swift/libswift_Concurrency.dylib`, which is a
   dyld-shared-cache-only path (no file on disk). If our rpath additionally
   points at the toolchain copy on disk, dyld loads *both* and you get
   `objc[…]: Class _TtCs… is implemented in both …` warnings plus
   warnings about "spurious casting failures and mysterious crashes".

**The fix in `build.rs`:** on macOS 13+ emit exactly one rpath,
`/usr/lib/swift` — that resolves to the same dyld-shared-cache library
Apple frameworks load, so there's a single shared copy. Pre-13 hosts
fall back to the toolchain path on disk. The build script lives in both
`crates/ak820-audio-reactive/build.rs` (covers its own test binaries)
and `crates/ak820-cli/build.rs` (covers the `ak820` binary). When we
wire audio-reactive into the Tauri app, the same recipe needs to land
in `src-tauri/build.rs` — Cargo doesn't share `rustc-link-arg` across
the workspace.

### 6.9h TFT chunk magic was off-by-one — 0x064F vs 0x0650
The 8-byte chunk header for `SET_TFT_USER_ANIMATION` ends in a magic
2-byte constant the firmware uses to validate the payload class. Up
to v0.7.0-beta we shipped `0x064F` (= 1615 LE u16); the AJAZZ web
driver actually sends `0x0650` (= 1616 LE u16), derived from
`6619136 / 4096`. Mario's hardware accepted our upload silently but
**never switched the display from its built-in animation to user
content** — exactly the visibility issue Phase 5b has been tracking
since 0.5.0-beta. The pcap path remains a stronger ground truth,
but this single-byte fix is consistent with what the web driver
sends and should at least unblock the activation step.

Decoded from the web-driver via grep + Python bracket-matching
extraction (see commit `382d59c..HEAD`). Also surfaced two related
findings worth noting here:

* `SET_TFT_USER_ANIMATION` chunks are ack'd with cmd **65**
  (`SET_LED_USER_ANIMATION`), not cmd 80. Our send path is
  fire-and-forget so we don't currently consume the ack — if we
  add per-chunk response reading later, filter on cmd 65 for these
  uploads.
* The AJAZZ tool also exposes `setTftDateTime` (10 B payload via
  cmd **52** `SET_TEMPORARY_COMMAND_DATA`) and `setTftScreenInfo`
  (24 B payload via the same cmd) — these push live data into the
  firmware's existing date/stats overlays, not new animations.
  Not yet implemented in our codebase; just constants reserved.

### 6.9i Knob remap is firmware-fixed on this hardware (confirmed)
Re-checked with the web-driver: the `wheelKeys` array on the AK820
Pro comes back **empty** when the AJAZZ tool reads the device. The
web tool has a full editor UI for wheel keys (`xk` Vue component,
right-click context menu, `setKeyData` plumbing), but the AK820 Pro
firmware just doesn't expose the wheel slot indices for remap. The
knob stays at Volume + / − / Mute (consumer HID 233 / 234 / 226)
no matter what we send over the standard `SET_KEY` (cmd 34) path.

Host-side workaround (not implemented): use Karabiner-Elements to
remap consumer HID 233/234/226 to F13/F14/F15, then bind those F
keys in the Automations tab. That gives the user a "configurable
knob" without the device cooperating.

### 6.9 Keymap action-page enum `O` values (mismatch landmine)
The official enum is:
`0 DEFAULT, 1 MOUSE, 2 KEYBOARD, 3 CONSUMER_KEY, 4 SYSTEM_KEY,`
`5 EXTRA_FUNCTION, 6 MACRO, 7 CB, 8 DKS, 9 MT, 10 TGL, 11 SOCD,`
`12 RS, 13 FUNC, 14 END, 15 MPT.`
Earlier Rust drafts had Macro=4 (= SYSTEM_KEY). Assigning a macro to
a key silently no-op'd because the firmware accepted byte 4 as a
system-key remap with system-key=0. Always cross-check `Page` enum
constants in `commands/keymap.rs` against the table in
`docs/PROTOCOL.md`.

---

## 7. Dev Workflow

```bash
# from project root
pnpm install                                  # frontend deps
cargo build -p ak820-cli --release            # CLI binary

# headless CLI checks
./target/release/ak820 list                   # show HID endpoints
./target/release/ak820 probe                  # confirm interface 2 / 0xFF68
./target/release/ak820 lighting modes
./target/release/ak820 lighting set --mode static --color FF0000 --brightness 5 --speed 0

# desktop app (must run in a real terminal — background tasks die without TTY)
pnpm tauri:dev
```

Frontend iteration loop:
1. Edit `.tsx`/`.css` files
2. `pnpm build` (rebuilds `dist/`)
3. **⌘+R** in the running app window (now works thanks to the menu setup)

Rust iteration loop:
1. Edit a `.rs` file
2. tauri-dev watcher auto-recompiles and relaunches the app

CLI does **not** coexist with a running tauri-dev that holds the HID
device — kill one before exercising the other (or use `force_reconnect`
between).

---

## 8. Tauri IPC Commands (frontend ↔ backend)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `list_devices` | — | `DeviceInfo[]` | full HID enumeration |
| `probe_device` | — | `ProbeReport` | **read-only**, no mutex |
| `force_reconnect` | — | — | drops cached Connection |
| `list_lighting_modes` | — | `LightingModeInfo[]` | static |
| `apply_lighting` | `{ config: LightingConfig }` | — | retries once on stale handle |
| `get_device_info` | — | `DeviceInfoReport` | grabs ConnState mutex |
| `get_game_mode` | — | `GameMode` | grabs ConnState mutex |
| `set_game_mode` | `{ mode: GameMode }` | — | grabs ConnState mutex |
| `list_sleep_presets` | — | `SleepPreset[]` | static |
| `get_keymap` | — | `Keymap` | multi-packet GET_KEY |
| `get_fn_keymap` | — | `Keymap` | multi-packet GET_FN_KEY |
| `set_keymap` | `{ keymap: Keymap }` | — | multi-packet SET_KEY |
| `set_fn_keymap` | `{ keymap: Keymap }` | — | multi-packet SET_FN_KEY |

All errors come back serde-tagged as
`{ kind: "Protocol" | "Lock", message: "…" }`.

---

## 9. Backlog / Open Questions

### Phase 4 — Macros
- RE the `GET_MACRO` (21) / `SET_MACRO` (37) wire format in the online
  driver bundle (`docs/reverse-engineering/online-driver/`).
- Frontend: macro recorder (capture key sequence + timings), assign to
  a slot via the existing keymap picker as a new action category.
- Hardware test against the 3072-byte macro space.

### Phase 5 — TFT display
- Decode `SET_TFT_USER_ANIMATION` (80) chunking: 128×128 RGB frames,
  total payload >> single packet so multi-chunk SET is required.
- Image upload pipeline: GIF/PNG → frames → resize+dither →
  device. Use `image-rs` and `gif` crates.
- Restore-default-animation command (probably `SET_TFT_BUILT_IN_INDEX` 81).

### Phase 6 — Power features
- Audio-reactive RGB via `ScreenCaptureKit` system-audio tap + FFT →
  color mapping → `SET_CUSTOM_LED_DATA` (36) per-key RGB.
- Now-playing on TFT via macOS `MediaRemote` private framework → text
  rendering → TFT frame upload.
- AppleScript / Shortcuts trigger from macro actions.
- iCloud-Drive backed profile sync
  (`~/Library/Application Support/ak820pro/`).
- Cross-platform builds: GitHub Actions matrix (macOS arm64/x86,
  Windows x86, Linux x86).
- Notarization: Apple Developer ID + entitlements, automated in CI.

### Loose ends
- ISO-Enter L-shape rendering (current: 1.5u rectangle).
- Per-key RGB editor (the `customLedData` 128-LED array).
- Wheel/knob actions — online driver has a `wheelKeys` structure that
  shares the keymap encoding. Needs UI for "knob CW / CCW / press"
  triple.
- Real LED indicators (Caps Lock, Win-Lock, Battery state) from
  `GET_DEVICE_INFO` / `GET_DEVICE_NOTIFY` (250).
- Backend hardening: `tokio::sync::Mutex` + async commands so future
  parallel invocations never deadlock the runtime.
- Better Lighting color accuracy (the picker doesn't quite match the
  rendered LED tone — could be HW gamma or the firmware's color-mode
  handling; worth deeper RE).

---

## 10. About this document

This is the **engineering handoff** — a long-form, foot-gun-annotated
trail through every non-obvious decision made building this project.
It's intentionally chattier than the public README and ARCHITECTURE
docs. Treat it as required reading before doing anything to the wire
protocol or to the Tauri shell's state-management layer.

If you find something in here that's wrong, or a foot-gun we haven't
documented yet, open a PR. Future contributors will thank you for
saving them the half-day of debugging.
