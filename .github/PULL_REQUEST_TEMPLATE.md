<!--
Thanks for the contribution! Fill in the relevant sections below — feel free to
delete any that don't apply. Empty PRs get auto-asked for context anyway, so
spending two minutes here saves a round-trip.
-->

## Summary

<!-- One-paragraph plain-English description of what changes and why. -->

## Type of change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (existing functionality changes meaningfully)
- [ ] Documentation update
- [ ] Protocol RE finding (new wire format decoded)
- [ ] Build / CI / repo housekeeping

## Hardware verification

<!--
If your change touches the wire protocol or the UI's HID-touching paths,
please describe what you tested on a real AK820 Pro.
-->

- Device firmware tested against: <!-- e.g. v1.07 -->
- Physical layout: <!-- ISO-DE / ANSI / etc. -->
- Connection: <!-- USB / 2.4 GHz / BT -->
- Hardware switch position: <!-- Mac / Win -->
- Steps used to verify: <!-- "ran ak820 rgb fill --color FF00FF, all keys turned magenta" -->

## Checklist

- [ ] Tests added / updated (`cargo test --workspace` passes)
- [ ] `cargo fmt` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `pnpm tsc --noEmit` clean
- [ ] `pnpm build` produces a working bundle
- [ ] Documentation updated (`docs/PROTOCOL.md` for wire changes, `CHANGELOG.md` for user-facing changes)
- [ ] Foot-guns annotated in code comments if you tripped over anything

## Linked issues

<!-- e.g. Closes #123, Refs #45 -->
