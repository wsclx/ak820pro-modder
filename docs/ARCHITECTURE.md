# Architecture

## Layers

```
+---------------------------------------------------------+
|  React UI (src/)                                        |
|    one view per tab: Connect, Lighting, Keymap, ...     |
+---------------------------↑-----------------------------+
                            | Tauri IPC (invoke)
+---------------------------↓-----------------------------+
|  App backend (src-tauri/)                               |
|    Tauri commands, connection state, TFT writer lock,   |
|    automations, presets, iCloud sync, Mac keyboard      |
|    remapping and macros                                 |
+---------------------------↑-----------------------------+
                            | Rust calls
+---------------------------↓-----------------------------+
|  ak820-protocol (crates/ak820-protocol)                 |
|    device detection and firmware family (A or B)        |
|    family A: 0xAA frames on 0xFF68, TFT on 0xFF67       |
|    family B: feature reports on 0xFF13, TFT on 0xFF68   |
|    command modules: lighting, keymap, macros, tft, ...  |
+---------------------------↑-----------------------------+
                            | hidapi
+---------------------------↓-----------------------------+
|  Keyboard (USB HID; control and display on separate     |
|  interfaces, see docs/PROTOCOL.md)                      |
+---------------------------------------------------------+
```

The desktop app and the `ak820` CLI both link `ak820-protocol`. Anything
that talks to the keyboard lives there, so a newly decoded command is
usable from both. Features that only exist on the Mac side (automations,
presets, iCloud sync, Now Playing, Mac-side remapping) live in
`src-tauri/` and have no CLI equivalent.

`crates/ak820-audio-reactive` captures system audio (ScreenCaptureKit) and
turns it into band levels; the app maps those to per-key colours.

## Why Tauri 2

- Small bundle (a few MB instead of Electron's ~150 MB) and the system
  WebView instead of a bundled Chromium.
- The backend is Rust, so it shares the protocol library with the CLI.
- macOS signing and notarisation are supported by the toolchain.

## Why a separate protocol crate

The decoded protocol is the part of this project that is hard to
reproduce. Keeping it free of UI code means it survives a change of UI
framework and could be used by other tools (a menu-bar helper, OpenRGB-
style aggregators) or published on its own.

## Errors for features that exist but aren't decoded

`ak820_protocol::Error::NotImplemented(&'static str)` marks features the
keyboard has but this project hasn't decoded yet; the UI shows that
instead of failing silently. On family-B boards, commands that are only
decoded for family A are refused before anything is sent.

## Writes

New write commands get tried through the CLI first. Some of them, such as
the image upload and the family-B clock sync, have a `--dry-run` that
prints or validates the bytes without opening the device. This is a
convention, not something the type system enforces.
