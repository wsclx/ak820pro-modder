# Installing AK820 Pro Modder

There are no signed releases yet, so the app is built from source.
Signed builds will appear on the [Releases](https://github.com/wsclx/ak820pro-modder/releases)
page once code signing is set up.

## Prerequisites

| Tool | Version | Used for |
|---|---|---|
| Rust | 1.90 or newer | protocol library, CLI, app backend |
| Node.js | 22 | frontend build |
| pnpm | 10 (pinned in `package.json`) | frontend packages |
| macOS | 11 or newer; 13 or newer for audio-reactive lighting | |

Rust comes from [rustup.rs](https://rustup.rs); Node.js 22 from
nodejs.org, nvm or Homebrew (`node@22`).

```bash
corepack enable          # provides the pnpm version from package.json

rustc --version          # 1.90 or newer
node --version           # v22.x
pnpm --version           # 10.x
```

## Build the app

```bash
git clone https://github.com/wsclx/ak820pro-modder.git
cd ak820pro-modder
pnpm install

pnpm tauri:build                 # .app and .dmg under target/release/bundle/
pnpm tauri:build --bundles app   # only the .app, faster
```

Open the `.dmg` from `target/release/bundle/dmg/`, drag
**AK820 Pro Modder.app** into Applications and start it. The build is
unsigned, so macOS blocks the first launch: right-click the app, choose
**Open** and confirm. After that it starts normally.

## Permissions

The app asks for these only when you use the feature that needs them:

| Permission | Needed for |
|---|---|
| Automation (control Music / Spotify) | Now Playing in the System view and on the display |
| Screen & System Audio Recording | audio-reactive lighting, which reads the system audio through ScreenCaptureKit |
| Accessibility | Mac-side macros on family-B boards, which type keys through macOS |

Key bindings for automations (F13–F24) need no permission. After granting
Accessibility, restart the app. A rebuilt unsigned app has a new code
identity, so macOS may require you to remove and re-add it under
Accessibility.

## Build only the CLI

```bash
cargo build -p ak820-cli --release
./target/release/ak820 --help
cp ./target/release/ak820 /usr/local/bin/   # optional
```

## Development mode

```bash
pnpm tauri:dev
```

This builds the frontend into `dist/` and starts the app against those
static files. Don't point Tauri at the Vite dev server (`pnpm dev`):
WKWebView hangs on its reload socket ([`HANDOFF.md`](HANDOFF.md) § 6.2).
`pnpm dev` alone is fine for layout and styling work in a browser, without
access to the keyboard.

## Tests

```bash
cargo test --workspace --lib
pnpm tsc --noEmit
pnpm build
```

## First check with the keyboard

Connect the keyboard by cable and run:

```bash
./target/release/ak820 list           # interfaces and which one is control / display
./target/release/ak820 capabilities   # firmware family and supported features
./target/release/ak820 info           # firmware, battery, profile (family A)
```

If `list` shows nothing, the keyboard isn't connected. If it lists
interfaces but opening fails with "Device not found" or an exclusive-access
error, another program holds the device: the official tool, a second
instance of this app, or another AK820 utility.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Black window after `pnpm tauri:dev` | Tauri points at the Vite dev server | `src-tauri/tauri.conf.json` must keep `"frontendDist": "../dist"` |
| ⌘R doesn't reload | stale build | the reload menu item is set up in `src-tauri/src/lib.rs`; rebuild |
| `hid_open_path: exclusive access` | another process holds the keyboard | quit other AK820 tools and app instances |
| "timeout waiting for cmd 0x10" | wrong interface, or a family-B board on an old build | update; the error now names the interface, see [#8](https://github.com/wsclx/ak820pro-modder/issues/8) |
| Remapped F-row keys send media keys | the Mac / Win switch on the back is on Mac, and the firmware handles the F-row first | use the Fn layer, or switch the keyboard to Win |
| "Another keyboard operation is running" (family B) | an image upload is still running | wait until it finishes, then retry |

More pitfalls are collected in [`HANDOFF.md`](HANDOFF.md). If none of this
helps, open an [issue](https://github.com/wsclx/ak820pro-modder/issues/new/choose)
with the output of `ak820 list`.
