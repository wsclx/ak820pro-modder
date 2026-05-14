# AK820 Pro

macOS-first, open-source control software for the **Epomaker / Ajazz AK820 Pro** mechanical keyboard.

The official Epomaker driver is Windows-only and limited. This project replaces it with a clean cross-platform Tauri app plus a headless CLI, built on a shared Rust protocol library.

## Status

Phase 0 (Foundation). Device enumeration and a placeholder probe handshake. See [the implementation plan](../.claude/plans/ich-habe-mir-die-inherited-glacier.md) for the full roadmap.

## Architecture

```
crates/
  ak820-protocol/     Rust library — HID transport, command encoders/decoders
  ak820-cli/          Headless `ak820` binary
src-tauri/            Tauri 2 shell — exposes protocol commands to the UI
src/                  React 19 + TypeScript + Tailwind frontend
docs/PROTOCOL.md      Living byte-level documentation of the wire protocol
```

## Hardware (for reference)

- **MCU**: HFD80CP100 (Sonix SN32F299 clone), 6×15 key matrix
- **Wireless**: WCH CH582F (BLE 5.1 + 2.4 GHz)
- **Flash**: PY25Q128HA 16 MB SPI
- **Display**: 0.85" NFP085B-10AF, 128×128, GC9107 over SPI
- **Operating VID**: `0x8009` (Ajazz SONiX) · control on HID interface 3
- **Bootloader VID/PID**: `0x0C45 / 0x7140` (hidden pins under spacebar)

## Build

```bash
# Frontend deps
pnpm install

# Headless CLI
cargo build -p ak820-cli --release
./target/release/ak820 list
./target/release/ak820 probe

# Tauri app (dev)
pnpm tauri:dev

# Tauri app (release bundle)
pnpm tauri:build
```

## Credits

Protocol RE builds on prior community work:

- [gohv/EPOMAKER-Ajazz-AK820-Pro](https://github.com/gohv/EPOMAKER-Ajazz-AK820-Pro) (MIT) — Rust reference for lighting / sleep / clock
- [TaxMachine/ajazz-keyboard-software-linux](https://github.com/TaxMachine/ajazz-keyboard-software-linux) — C++ reference and pcap-parsing approach
- [fpb/ajazz-ak820-pro](https://github.com/fpb/ajazz-ak820-pro) — hardware reverse-engineering notes

## License

MIT.
