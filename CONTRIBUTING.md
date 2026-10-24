# Contributing to AK820 Pro Modder

Thanks for considering it. Most of what this project knows about the
keyboard came from people who tested on their own board, shared a USB
capture or read the vendor driver closely, so hardware reports are as
useful as code.

## Ground rules

1. Follow the [Code of Conduct](CODE_OF_CONDUCT.md).
2. Open an issue before a large PR. A short sketch saves both sides a
   rewrite.
3. Changes that write to the keyboard need evidence: a unit test with
   byte fixtures, a CLI command a maintainer can run, and ideally a USB
   capture or a reference to the vendor driver source.
4. No credentials, personal paths or private data in commits.

## Ways to help

### Test on your keyboard

Run `ak820 list` and `ak820 capabilities` first. They show which firmware
family your board belongs to (see the README) and its USB release number.
Then try the features and report what does and doesn't work, together
with that output. Especially useful right now:

- family-B boards (`0xFF13` in `ak820 list`): the test plan in
  [#14](https://github.com/wsclx/ak820pro-modder/pull/14);
- firmware other than 1.07;
- how the Mac / Win switch on the back changes which keys can be
  remapped.

Be aware that on family-B boards every lighting and display write is
stored in flash. Test deliberately, not in a loop.

### Add or verify a physical layout

The protocol doesn't depend on the layout; only the on-screen keyboard in
the Keymap view does. ISO-DE is verified. ANSI, ISO-UK, ISO-ES and ISO-FR
exist but are unverified ([#7](https://github.com/wsclx/ak820pro-modder/issues/7)),
JIS is missing ([#5](https://github.com/wsclx/ak820pro-modder/issues/5)).

To fix or add one:

1. Compare each key in the Keymap view with your keycaps. The slot number
   of a key is the same on every variant; only the legend and the key
   shape differ.
2. Edit or add `src/data/layouts/<id>.json` (rows of
   `{ slot, label, hid, cls? }`) and the matching `<id>.ts`, then register
   it in `src/data/layouts/index.ts`. Leave `DEFAULT_LAYOUT_ID` alone.
3. Attach a photo of the board and a screenshot of the Keymap view to
   the PR.

All layout differences belong in the layout's `.json` and `.ts` files, not
in the Keymap view.

### Reverse engineering

Open questions, roughly by value:

1. Live per-key colours without writing to flash (command 50/53 on
   family A, `04 20` on family B), for audio-reactive lighting
   ([#3](https://github.com/wsclx/ak820pro-modder/issues/3)).
2. The display artefacts in [#1](https://github.com/wsclx/ak820pro-modder/issues/1)
   and [#2](https://github.com/wsclx/ak820pro-modder/issues/2), and the
   frame-delay unit (2 ms or 5 ms).
3. Onboard key maps and macros on family B.
4. Knob assignments, profile switching, the device notifications on
   command 250.

[`docs/research/2026-10-25-board-scan.md`](docs/research/2026-10-25-board-scan.md)
collects what other projects found, with sources.

To make a useful USB capture: a Windows machine or VM with USB
passthrough, the official AJAZZ tool, [USBPcap](https://desowin.org/usbpcap/)
and Wireshark. Capture exactly one action (for example uploading a
single red 128 × 128 PNG), save it as
`phase{N}-{area}-{action}-{seq}.pcapng` and attach it to an issue.
Captures are kept out of the repository because of their size; refer to
them by file name.

### Code

Open issues are the best starting point. When you touch the protocol
library, one module per area under `crates/ak820-protocol/src/commands/`
(plus `legacy*.rs` for family B) is the pattern: encoder, decoder and
unit tests with byte fixtures.

## Development setup

```bash
git clone https://github.com/wsclx/ak820pro-modder.git
cd ak820pro-modder
corepack enable          # uses the pnpm version pinned in package.json
pnpm install

pnpm tauri:dev           # desktop app; builds the frontend into dist/ first
cargo build -p ak820-cli # command-line tool
```

Prerequisites: Rust 1.90+, Node.js 22. Don't point the Tauri window at
the Vite dev server (`pnpm dev`): WKWebView hangs on it. The details are
in [`docs/HANDOFF.md`](docs/HANDOFF.md) § 6.2.

## Checks

CI runs these on macOS; run them before you push:

```bash
pnpm install --frozen-lockfile && pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --no-deps -- -D warnings
cargo test --workspace --lib
pnpm tsc --noEmit
```

Tests that need a keyboard are marked `#[ignore]` and run by hand.

## Pull requests

- Branch names: `feature/…`, `fix/…`, `docs/…`, `re/…` for protocol work.
- One logical change per PR.
- Reference the issue (`Closes #123` closes it on merge, `Refs #45`
  doesn't).
- Fill in the hardware section of the
  [PR template](.github/PULL_REQUEST_TEMPLATE.md), including "not tested
  on hardware" when that's the case.
- PRs are squash-merged, so the PR title becomes the commit message.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/)
loosely: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ui`,
`protocol`, `re`.

## Code style

Rust: `cargo fmt`, Clippy clean, rustdoc on public items. Code that
touches the wire should say where the format comes from (vendor driver
file, capture, contributor measurement) and note any pitfall you hit.

TypeScript: `pnpm tsc --noEmit` clean, no new `any`, one view per file
named after its export (`Lighting.tsx` exports `Lighting`).

## Working on write paths

The firmware is undocumented. When you add or change a write:

- Try it through the CLI before wiring it into the app.
- Give anything that replaces stored data a `--dry-run` that prints the
  bytes without opening the device.
- Write down how to undo it: rewriting the previous value, the Factory
  default button in the Keymap view, or, as a last resort, the ISP
  bootloader described in [`SECURITY.md`](SECURITY.md).
- On family B, keep writes to explicit user actions.

## License

Contributions are licensed under the [MIT License](LICENSE).
