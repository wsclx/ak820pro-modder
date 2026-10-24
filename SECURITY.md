# Security Policy

## Supported versions

Pre-1.0 releases ship security fixes on the current `main` branch only. We don't backport.

| Version | Supported |
|---|---|
| `main` / 0.7.x-beta | yes |
| older | no |

## Threat model

`AK820 Pro Modder` talks to one USB-HID device via `hidapi`. Concretely:

1. Local only. The app opens no network connections and no listening
   ports. Its data lives in the user's Application Support folder, in
   iCloud Drive if sync is turned on, and on the keyboard.
2. It runs what you tell it to. Automations are AppleScripts, Shortcuts
   and shell commands from your own library, run with your user's rights.
   On family-B boards the Mac-side macros post key events through macOS
   (Accessibility permission).
3. It writes to the keyboard. USB-HID writes can in principle
   - leave the keyboard unusable if the firmware rejects a sequence badly
     (not observed so far, but there is no warranty),
   - corrupt stored settings (key map, macros, lighting),
   - wear the flash: on family-B boards every lighting and display write
     is stored, so the app only writes on explicit actions there.
4. Unsigned builds. Builds from source are not notarised; macOS asks for
   confirmation on first launch.

## Hardware rescue paths

If a write puts your keyboard in a weird state, in increasing order of severity:

1. Write the previous value again: lighting, sleep timer and per-key
   colours can simply be set back. The Keymap view's **Factory default**
   button restores the default key map (family A).
2. Unplug the keyboard and plug it back in.
3. A full factory reset over HID (command 15) is known from the vendor
   driver but deliberately not exposed by the app or the CLI.
4. The hidden ISP bootloader: pads under the spacebar (see
   [fpb/ajazz-ak820-pro](https://github.com/fpb/ajazz-ak820-pro)).
   Shorting them at power-up starts Sonix ISP mode (`0C45:7140`), where
   Sonix tools can flash the factory firmware again. The app ignores
   devices in this mode.

## Reporting a vulnerability

If you find a way to:

- brick the keyboard by sending bytes via this app,
- escalate privileges via the Tauri shell,
- read or write data outside this app's profile directory without explicit user consent,
- or anything else that looks like a meaningful security issue,

**please open a private security advisory** at <https://github.com/wsclx/ak820pro-modder/security/advisories/new>.

We aim to respond within 7 days, and to ship a fix or detailed mitigation guidance within 30 days for high-severity reports. Coordinated disclosure of up to 90 days is fine; we'd rather get the fix right than rush it.

Please **do not** open a public issue for security reports.

## Scope

In-scope:
- The Rust workspace (`crates/ak820-protocol`, `crates/ak820-cli`, `src-tauri`).
- The React frontend (`src/`).
- Build artefacts shipped on the Releases page.

Out-of-scope:
- Vulnerabilities in the keyboard firmware itself (please report those to Epomaker / Ajazz directly).
- Issues affecting only the development environment (CI, build scripts); those go in the regular issue tracker.
- Theoretical issues without a working PoC.

## Acknowledgements

Confirmed reports are credited in the [CHANGELOG](CHANGELOG.md) if the reporter agrees.

Thanks for keeping the project safe to use.
