# Security Policy

## Supported versions

Pre-1.0 releases ship security fixes on the current `main` branch only. We don't backport.

| Version | Supported |
|---|---|
| 0.5.x-beta | ✅ |
| < 0.5.0   | ❌ |

## Threat model

`AK820 Pro Modder` talks to one USB-HID device via `hidapi`. Concretely:

1. **Local-only attack surface.** The app does not phone home, take network input, or expose any listening port. All data lives on disk in the user's profile directory and on the keyboard itself.
2. **Privileged hardware control.** USB-HID writes can in principle:
   - brick the keyboard if firmware-illegal sequences are sent (we have not observed this on fw 1.07 across thousands of writes, but no warranty),
   - corrupt onboard storage (keymap, macros, lighting effect),
   - drain battery faster than the firmware default.
3. **macOS Notarisation.** Pre-built releases will be signed and notarised. If you build from source, macOS Gatekeeper will mark the binary as quarantined on first launch; that's expected.

## Hardware rescue paths

If a write puts your keyboard in a weird state, in increasing order of severity:

1. **Reload defaults via the app**: any view's "Factory default" or "Reload" button issues a read-only sync from the device.
2. **CLI factory reset**: not yet implemented (cmd 15 `SET_FACTORY_RESET` exists but is gated behind a `--yes-really` flag we haven't shipped — see [`docs/PROTOCOL.md`](docs/PROTOCOL.md)).
3. **Unplug + replug + reset switch on the back** (Mac ↔ Win toggle counts as a soft reset).
4. **Hidden ISP bootloader**: there are hidden pins under the spacebar (see `docs/HANDOFF.md` § 2 and the [fpb/ajazz-ak820-pro](https://github.com/fpb/ajazz-ak820-pro) hardware notes). Shorting them on power-up forces the keyboard into Sonix ISP mode (`VID/PID 0x0C45 / 0x7140`), where the official Sonix Toolchain can re-flash factory firmware.

## Reporting a vulnerability

If you find a way to:

- brick the keyboard by sending bytes via this app,
- escalate privileges via the Tauri shell,
- read or write data outside this app's profile directory without explicit user consent,
- or anything else that looks like a meaningful security issue,

**please open a private security advisory** at <https://github.com/wsclx/ak820pro-modder/security/advisories/new>.

We aim to respond within 7 days, and to ship a fix or detailed mitigation guidance within 30 days for high-severity reports. Coordinated disclosure of up to 90 days is fine — we'd rather get the fix right than rushed.

Please **do not** open a public issue for security reports.

## Scope

In-scope:
- The Rust workspace (`crates/ak820-protocol`, `crates/ak820-cli`, `src-tauri`).
- The React frontend (`src/`).
- Build artefacts shipped on the Releases page.

Out-of-scope:
- Vulnerabilities in the keyboard firmware itself (please report those to Epomaker / Ajazz directly).
- Issues affecting only the development environment (CI, build scripts) — those go in the regular issue tracker.
- Theoretical issues without a working PoC.

## Acknowledgements

Confirmed reports will be credited (with permission) in the [CHANGELOG](CHANGELOG.md) and in a forthcoming `SECURITY-ACKNOWLEDGEMENTS.md`.

Thanks for keeping the project safe to use.
