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
| Product | AK820 Pro (Mario's unit: ISO-DE, firmware **1.07**) | device GET_DEVICE_INFO |
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
/Users/mario/DEV/ak820pro/
├── README.md                       <- public-facing
├── docs/
│   ├── HANDOFF.md                  <- this file — read first
│   ├── ARCHITECTURE.md             <- layered breakdown
│   ├── PROTOCOL.md                 <- byte-level protocol reference
│   └── reverse-engineering/
│       ├── official-export-firmware-1.07.json   <- Mario's profile export
│       └── online-driver/
│           └── default-protocol.js  <- the AJAZZ driver source, snapshotted
├── crates/
│   ├── ak820-protocol/             <- Rust core library (no UI)
│   │   └── src/
│   │       ├── lib.rs              <- VID, PID, control interface
│   │       ├── error.rs
│   │       ├── protocol.rs         <- build_frame, command bytes, MAGIC_*
│   │       ├── device.rs           <- Connection, enumerate, get/set,
│   │       │                          get_many, set_many, all command APIs
│   │       └── commands/
│   │           ├── lighting.rs     <- Mode, Direction, LightingConfig, encode
│   │           ├── system.rs       <- DeviceInfoReport, GameMode, sleep presets
│   │           ├── keymap.rs       <- KeyAction enum, Keymap, 4-byte codec
│   │           ├── sleep.rs        (stub)
│   │           ├── clock.rs        (stub)
│   │           ├── macros.rs       (stub — Phase 4)
│   │           └── tft.rs          (stub — Phase 5)
│   └── ak820-cli/
│       └── src/main.rs             <- `ak820` binary: list, probe,
│                                       lighting set/modes
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json             <- frontendDist=../dist, no devUrl
│   ├── build.rs
│   ├── entitlements.plist          <- USB-HID permission
│   ├── capabilities/default.json
│   ├── icons/
│   └── src/
│       └── lib.rs                  <- Tauri commands + ConnState mutex +
│                                       native macOS menu
├── src/                            <- React 19 + Vite + TS
│   ├── main.tsx
│   ├── App.tsx                     <- tab routing, probe polling
│   ├── index.css                   <- Tailwind base + theme tokens
│   ├── types.ts
│   ├── components/
│   │   ├── ui.tsx                  <- Card, Button, Badge, KVList, Slider,
│   │   │                              Toggle, BatteryBar, ErrorBanner,
│   │   │                              Mono, hex4, formatInt, prettyProduct
│   │   └── Layout.tsx              <- Sidebar + page chrome + lucide-react
│   │                                  icon re-exports
│   ├── views/
│   │   ├── Connect.tsx             <- HID enumeration + control-interface info
│   │   ├── Lighting.tsx            <- 20 modes, color, direction, sliders
│   │   ├── System.tsx              <- firmware, battery, sleep, game-mode
│   │   └── Keymap.tsx              <- visual ISO-DE, click-to-edit, save
│   └── data/
│       ├── iso-de-layout.json      <- physical key positions, slots & HIDs
│       ├── iso-de-layout.ts        <- typed wrapper
│       ├── hid-usage-names.ts      <- HID code → human label
│       └── action-catalog.ts       <- picker groups (letters/digits/F-keys/
│                                       modifiers/media/special)
├── index.html
├── vite.config.ts                  <- HMR disabled (see Learning #2)
├── tailwind.config.ts              <- OKLCH-tuned tokens, fg/surface/accent
├── tsconfig.json
├── package.json
├── Cargo.toml                      <- workspace root
├── rust-toolchain.toml             <- pinned to 1.90+
└── tests/fixtures/
```

---

## 5. Phase / Feature Status

| Phase | Status | What works | Notes |
|---|---|---|---|
| 0 — Foundation | ✅ done | Tauri 2 app, workspace, CLI, device probe | live-validated on Mario's keyboard |
| RE — Decode wire format | ✅ done | All command bytes, frame layout, encoders/decoders | from online-driver source |
| 1 — Lighting | ✅ done | 20 modes, color, secondary, direction, brightness/speed, debounced auto-apply | LEDs visibly respond on hardware |
| 2 — System | ✅ done | Firmware, battery, profile, macro space, frame version, sleep-timer set+verify | full round-trip |
| Design Pass v2 | ✅ done | Linear/Raycast-style sidebar, Lucide icons, OKLCH tokens, no Geist (caused issues) | dark surfaces with rim-light |
| Polish + Connect UX | ✅ done | Adaptive polling (2s/8s), Reconnect button, hotplug-recovery, number formatting, "AK820 Pro" label, native ⌘+R menu | sequential reads in System view to avoid mutex contention |
| 3 — Keymap read | ✅ done | Full ISO-DE visual layout, Base + Fn layer, knob/TFT/LED visual placeholders, **responsive scaling 0.55x–1.5x via ResizeObserver+CSS transform** | reads via multi-packet GET_KEY |
| 3 — Keymap edit | ✅ done | Click-to-select, action picker (letters, digits, F-keys, editing, navigation, modifiers, media, factory-default), dirty-state badge, save+verify, discard | tested with Caps→F12 round-trip |
| 4 — Macros | ⏳ pending | — | needs more RE (`SET_MACRO` 37, format unknown) |
| 5 — TFT display | ⏳ pending | placeholder UI exists | needs RE of `SET_TFT_USER_ANIMATION` (80), chunking strategy |
| 6 — Power features | ⏳ pending | — | audio-reactive, now-playing, AppleScript bridge, iCloud sync |

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

**Eventual hardening (not done):** switch `ConnState` to
`tokio::sync::Mutex` and make the commands `async fn` so awaits don't
block worker threads.

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
- Better Lighting color accuracy (Mario noted picker ≠ LED tone —
  could be HW gamma or it could be the firmware's color-mode handling;
  worth deeper RE).

---

## 10. Memory Files (across sessions)

Always check these before assuming you have to re-learn something:

- `~/.claude/projects/-Users-mario-DEV-ak820pro/memory/MEMORY.md`
- `~/.claude/projects/-Users-mario-DEV-ak820pro/memory/feedback_*.md`
- `~/SecondBrain/9-agents/claude/code/` (instance-specific notes)

Current entries cover: subagent prompt-too-long blocker, Tauri+Vite
dev-server failure mode, AK820 protocol RE pivot, std::Mutex deadlock,
Tauri 2 menu wiring.
