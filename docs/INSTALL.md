# Installing AK820 Pro Modder

`AK820 Pro Modder` is currently distributed as source — pre-built signed `.dmg`s will land on the [Releases](https://github.com/wsclx/ak820pro-modder/releases) page once the CI codesigning pipeline is wired up.

## Prerequisites

| Tool | Version | Why |
|---|---|---|
| **Rust** | 1.82+ | The protocol library, CLI, and Tauri shell are all Rust. |
| **Node.js** | 20+ | TypeScript / React frontend build. |
| **pnpm** | 9+ | Frontend package manager. |
| **macOS** | 11+ (Big Sur) | The Tauri shell ships with this minimum target. |

```bash
# macOS (Homebrew):
brew install rustup-init pnpm
rustup-init -y --default-toolchain stable
nvm install 20  # or any Node 20 source you trust

# Verify
rustc --version    # → rustc 1.82.x …
node --version     # → v20.x.x
pnpm --version     # → 9.x.x
```

## Build the desktop app

```bash
git clone https://github.com/wsclx/ak820pro-modder.git
cd ak820pro-modder
pnpm install

# Production-style bundle (creates src-tauri/target/release/bundle/dmg/*.dmg)
pnpm tauri:build

# Or just the .app for quick testing
pnpm tauri:build --bundles app
```

Open the resulting `.dmg`, drag **AK820 Pro Modder.app** into Applications, and launch.

> macOS Gatekeeper will mark the unsigned binary as quarantined on first launch. To approve it, right-click the app and choose **Open**, then confirm in the dialog. After that it launches normally. We'll move to signed + notarised builds before 1.0.

## Build just the CLI

If you don't need the GUI:

```bash
cargo build -p ak820-cli --release
./target/release/ak820 --help
```

This produces a single statically-linked binary (~3 MB) you can copy into `/usr/local/bin` if you like:

```bash
cp ./target/release/ak820 /usr/local/bin/
ak820 list
```

## Development mode

For frontend / app iteration:

```bash
pnpm tauri:dev
```

This runs `pnpm build` first (static frontend → `dist/`) and then launches Tauri pointed at the static dist. **Do not** try to wire Tauri at `pnpm dev` (the Vite dev server) — WKWebView consistently hangs on the dev-server's HMR socket. See [`HANDOFF.md`](HANDOFF.md) § 6.2 for the gory details.

For just the React frontend (no live device — useful for layout / styling work):

```bash
pnpm dev   # serves at http://localhost:5173, but Tauri APIs won't be available
```

## Tests

```bash
# Rust unit tests across the workspace
cargo test --workspace

# TypeScript typecheck
pnpm tsc --noEmit

# Frontend production build (catches JSX runtime errors)
pnpm build
```

## Hardware-in-the-loop verification

Plug in the AK820 Pro and run:

```bash
./target/release/ak820 list           # Should list 9 HID interfaces
./target/release/ak820 probe          # "Connected: true"
./target/release/ak820 info           # Firmware, battery, profile
```

If `list` returns nothing, the device isn't connected on USB / 2.4 GHz / BT. If it returns 9 interfaces but `probe` errors with "Device not found", check that another app (the official AJAZZ tool, another instance of this app, etc.) isn't holding the HID handle exclusively.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `cargo build` fails on `hidapi` | Missing libusb headers on Linux | `sudo apt install libusb-1.0-0-dev libudev-dev` |
| `pnpm tauri:dev` shows a black window | Hit Vite-dev-server hang | Make sure `tauri.conf.json` has `"frontendDist": "../dist"` (it does, by default) |
| `⌘+R` doesn't reload | Tauri 2 ships no menu by default | Already wired in `src-tauri/src/lib.rs::setup()` — your build is stale, rerun `pnpm tauri:dev` |
| `Error: hid_open_path: exclusive access` | Another process owns the HID handle | Quit any other AK820 controller, close other instances of this app |
| App freezes when clicking certain tabs | `std::sync::Mutex` deadlock pattern | Already fixed in 0.5.0-beta+. If you see this on a recent build, file a bug. |
| Macros don't fire on F-row keys | macOS hardware switch on the back is set to "Mac" — firmware preempts the F-row with media keys | Use the **Fn** layer in the Keymap view (Fn + F-key triggers your macro), or switch the back of the keyboard to "Win" mode |

For anything not covered above, see [`docs/HANDOFF.md`](HANDOFF.md) for the full foot-gun catalogue or open an [issue](https://github.com/wsclx/ak820pro-modder/issues/new/choose).
