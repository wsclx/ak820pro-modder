# Contributing to AK820 Pro Modder

First off — thanks for considering a contribution. This project lives or dies by the patience of people willing to share captures, test on physical hardware, or sand off rough edges. Whether you're a Rust hacker, a frontend designer, or just a user with a USB sniffer, there's something useful you can do.

## Ground rules

1. **Read the [Code of Conduct](CODE_OF_CONDUCT.md)** — short version: don't be a jerk; assume good faith; the keyboard is more important than any debate.
2. **Open an issue before a big PR.** A 30-second sketch of what you want to do beats sending a 500-line patch that has to be reworked.
3. **Hardware changes need hardware evidence.** Anything that writes to the keyboard ships with a CLI smoke test or a unit-test fixture, and ideally a USB pcap reference.
4. **No personal credentials in commits.** Configs, paths, secrets all stay out of the repo. We're a public project.

## Ways to help

### 🧪 Hardware testing
You own an AK820 Pro? Open the app, try every feature, and report what doesn't work for your firmware / layout. Especially valuable:

- **Different firmware versions** (run `ak820 info` to see yours — we're verified on `v1.07`).
- **The Mac / Win hardware switch on the back** behaves differently between firmwares — we'd love captures of which slots respond to remap in each mode.

### ⌨️ Adding a new physical layout

`v0.5.0-beta` ships **ISO-DE only**. The wire protocol is layout-agnostic — only the on-screen keyboard surface is layout-specific. To add ANSI / ISO-FR / ISO-ES / ISO-UK / JIS support cleanly:

1. **Capture your layout's keycaps.** Run `ak820 info` to confirm the firmware version, then export the factory keymap (planned CLI helper: `ak820 dump-default-keymap`). Cross-reference each printed legend with the device slot from `GET_KEY`.
2. **Add the descriptor file.** Drop `src/data/layouts/<layout-id>.json` with the same schema as `iso-de.json` — `PhysicalKey[][]` rows of `{ slot, label, hid, cls? }`. Slot numbers are firmware-internal and identical across variants; only `label` and the Tailwind `cls` flexbox hints change.
3. **Wire it through.** Create `<layout-id>.ts` mirroring `iso-de.ts`, then register it in `src/data/layouts/index.ts`'s `LAYOUTS` map. **Never** change `DEFAULT_LAYOUT_ID` without an explicit discussion — the default has always been `iso-de` and will stay so until multi-layout coverage is complete.
4. **Test on real hardware** and attach screenshots / captures to your PR. The PR template asks specifically about layout — fill it in.

Layout-aware branches in the Keymap view itself are off-limits — render uniformly from whichever `KeyboardLayout` the registry returns. All variant-specific differences live exclusively in the variant's `.json` + `.ts` pair.

### 🕵️ Reverse engineering
We have ~80 % of the wire protocol decoded against the AJAZZ web driver. The remaining 20 % is invisible from JS source alone and needs USB pcap captures of the official Windows tool. Highest-priority captures:

1. **TFT upload + activation** — current open question. We can write the buffer; we can't yet make the device play it.
2. **Per-key RGB enable path** — `SET_CUSTOM_LED_DATA` writes succeed, but the visible LEDs only update with the right mode-switch sequence.
3. **Knob remap** — the AJAZZ web driver has no UI for this, but the offline Windows tool might.
4. **Profile switch** — onboard slots 0..3, no UI exposed yet.
5. **Battery / charging notifications** — the device pushes async events on cmd 250; we haven't decoded the payload.

How to capture a useful pcap:
- UTM (free) + Windows 11 ARM Insider Preview, or any Windows VM with USB passthrough.
- Install the official AJAZZ offline app + their driver.
- [USBPcap](https://desowin.org/usbpcap/) — free, ~5 MB, integrates with Wireshark.
- Start capture, do **one** atomic action (e.g. upload a 128×128 solid-red PNG), stop capture.
- Save as `phase{N}-{family}-{action}-{seq}.pcapng` and attach to an issue or PR.
- We treat captures as evidence — they go into `docs/reverse-engineering/captures/` (gitignored for size, referenced by name in the PR).

### 🦀 Rust
Pick up any of these:

- New command modules — `src/commands/` is one file per family. Pattern: encoder + decoder + unit tests with hex fixtures.
- Better error types — currently `Error::OutOfRange` is overloaded; could split per family.
- Tokio-async HID I/O — we use `tokio::sync::Mutex` but the I/O itself is still blocking. `tokio::task::spawn_blocking` wrappers would let other commands progress during long writes.
- More HID interface autodetection — `probe_interfaces()` parses descriptors crudely; a real HID-descriptor crate would let us identify "this is the TFT endpoint" without env-var fallbacks.

### ⚛️ Frontend
- **TFT image upload UI** (drag-and-drop GIF / PNG → preview → upload with `tauri-plugin-fs` + the `image` / `gif` crates on the Rust side).
- **Per-key RGB paint mode** — small keyboard surface in the Lighting tab, click-to-colour.
- **Audio-reactive visualiser** — preview the FFT-driven colour map before committing it as an effect.
- **Visual polish** — ISO-Enter L-shape, animated layer-switch transitions, dark/light theme toggle.

### 📝 Documentation
- Better install instructions per platform.
- Tutorial-style walk-throughs (your first macro, your first per-key RGB, …).
- Translate the README into other languages.

## Development setup

```bash
git clone https://github.com/wsclx/ak820pro-modder.git
cd ak820pro-modder

# Prereqs: Rust 1.82+, Node 20+, pnpm 9+
pnpm install

# Run the Tauri app in dev mode (rebuilds the static frontend on each launch)
pnpm tauri:dev

# Build & test only the Rust side
cargo test --workspace
cargo build -p ak820-cli --release

# Frontend-only iteration (no live device)
pnpm dev   # ⚠️ this serves Vite; do NOT point the Tauri shell at it — see docs/HANDOFF.md § 6.2
```

### Branch / PR conventions

- Branch names: `feature/...`, `fix/...`, `docs/...`, `re/...` (for protocol RE work).
- One logical change per PR — split refactors and features.
- Reference an issue in the PR body (`Closes #123` / `Refs #45`).
- The PR template ([`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md)) asks specifically about hardware verification — fill it in.

### Commit messages

Aim for the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) shape:

```
feat(lighting): add audio-reactive mode hook

Wire-format unchanged — this only adds a host-side FFT pipeline that
runs every 33ms and writes the dominant frequency colour via
SET_CUSTOM_LED_DATA. See PROTOCOL.md § audio-reactive for the colour
mapping rationale.

Closes #42
```

Types we use: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `re` (reverse-engineering), `hardening`, `ui`, `protocol`.

### Code style

**Rust**:
- `cargo fmt` before every commit.
- `cargo clippy --workspace --all-targets -- -D warnings` should be clean.
- Public functions need rustdoc. If it touches the wire, link to the AJAZZ source location and add a foot-gun annotation if you tripped over anything.

**TypeScript / React**:
- `pnpm tsc --noEmit` should be clean (it's part of `pnpm build`).
- No `any` types in new code.
- Component files match the view they expose (`Lighting.tsx` exports `Lighting`).
- Tailwind utility classes preferred over inline styles for static layout; inline styles for measured-pixel positioning (keymap surface).

## Testing

Every protocol PR ships with:

1. **Unit tests** on the encoder / decoder against either AJAZZ JS-source-derived byte fixtures or USB pcap captures.
2. **CLI smoke command** so a maintainer can verify on real hardware without launching the app.
3. **Round-trip evidence** — read after write should match what was written (where the device exposes a read).

UI PRs ship with:
1. Screenshots before / after in the PR body.
2. A note on accessibility (keyboard navigation, contrast).
3. No regressions in the existing typecheck (`pnpm tsc --noEmit`).

## Safety first

We've never bricked an AK820 Pro doing this work, but the protocol is undocumented and the firmware is opaque. Whenever you touch a write path:

- Stage it through the CLI before the UI.
- Default to `--dry-run` when the operation is irreversible.
- Document the recovery: how do you back this out? Sometimes it's a factory-reset HID command (cmd 15), sometimes it's the ISP bootloader under the spacebar (see [`docs/HANDOFF.md`](docs/HANDOFF.md) § 2).

## License

By contributing you agree your contributions are licensed under the [MIT License](LICENSE).
