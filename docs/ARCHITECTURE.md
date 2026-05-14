# Architecture

## Layering

```
+--------------------------------------------------+
|  React UI (src/)                                 |
|    – Views: Connect, Lighting, Keymap, Macros... |
+----------------------↑---------------------------+
                       | Tauri IPC (invoke)
+----------------------↓---------------------------+
|  Tauri shell (src-tauri/)                        |
|    – Thin command handlers, no business logic    |
+----------------------↑---------------------------+
                       | Rust function calls
+----------------------↓---------------------------+
|  ak820-protocol (crates/ak820-protocol)          |
|    – Device enumeration                          |
|    – Frame encoding / decoding                   |
|    – Feature modules: lighting / sleep / clock / |
|      keymap / macros / tft                       |
+----------------------↑---------------------------+
                       | hidapi
+----------------------↓---------------------------+
|  Keyboard hardware (HID interface 3)             |
+--------------------------------------------------+
```

Both the Tauri shell **and** the CLI link `ak820-protocol` directly. No business logic lives in either UI layer — they're both clients of the same library. This means:

- Every Tauri command has an equivalent CLI subcommand (CI-friendly).
- Anything decoded once is usable from both surfaces immediately.
- The library can be reused in unrelated tools (e.g. a menu-bar widget, a CI smoke-test harness).

## Why Tauri 2

- macOS-first with first-class Notarization tooling.
- Bundle size <10 MB. Compared with Electron's ~150 MB, this matters for beta distribution.
- Rust backend lets us share code with the CLI.
- WebView2/WKWebView/WebKitGTK render native UI; no Chromium bundled.

## Why a separate library crate

The HID protocol decoding is the irreplaceable artifact of this project. Keeping it isolated from any UI framework means:

- The project survives a future UI-stack switch.
- The library can be published to crates.io independently.
- Other projects (OpenRGB-style aggregators, automation tools) can depend on it.

## Phase-aware error surface

`ak820_protocol::Error::NotImplemented(&'static str)` is reserved for features whose protocol is **known to exist** but **not yet decoded**. Calling them returns a structured error the UI uses to show "decoded in Phase N" instead of failing silently.

## Read-only first

Every new write-command lands behind a `--dry-run` flag in the CLI that logs the would-be bytes without sending. Only after byte-for-byte review do we flip the switch. This is enforced by convention, not (yet) by the type system.
