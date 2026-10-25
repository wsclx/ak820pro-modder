# Mac keyboard services and recovery

The Mac fallback applies an explicit remapping draft to the keyboard services
of one wired AK820 (`0C45:800A`). It preserves other tools' entries and keeps
each service's baseline separately. These changes affect macOS, not keyboard
firmware. Apply and Undo are explicit actions; startup never applies a draft.

## Why services are grouped

One physical keyboard can expose multiple services that conform to keyboard
usage page 1 / usage 6. On the tested ANSI unit, one service's primary usage
is Keyboard and the other's is Consumer Control. Both conform to Keyboard;
filtering only the primary usage misses the second service.

The native implementation retains the IOHID event-system client and its
service handles throughout the operation. It resolves each service's registry
ID through IORegistry's IOService parent chain to the physical `IOUSBHostDevice`
(or older `IOUSBDevice`). Both the service and USB parent must identify an AK820
with the expected vendor/product IDs. Only wired USB services are included.

Two physical parents are refused, even if their product names, locations or
serials match. Missing ancestry is an error. Location IDs, serials and service
enumeration order do not select or group devices.

## Apply and Undo

The editor shows the app's requested draft after Apply, not a flattened union
of all native mapping tables. Before the first Apply, equal, representable
service maps can be shown. Different maps or other usage pages produce a
diagnostic; a new draft can still be entered and applied while preserving those
entries. Full-width native usage values remain in backups.

Apply computes every service's result before saving or writing anything. A
source already mapped differently by another tool is a conflict. An identical
pre-existing entry is preserved and does not become an app-owned change.
Subsequent Apply and Undo remove only differences owned by the app. External
edits to an owned source block the operation; unrelated additions and removals
are preserved.

A version 2 recovery record is written before any native mutation. It stores
the attachment identity, requested draft, separate baseline and applied maps,
and, during a transaction, its before/after maps and previous/next records.
Writes are checked immediately and all services are checked again before the
record is finalized. Backup writes use a synchronized temporary file and rename.

If a write, read-back or final backup operation fails, rollback checks each
service and restores only a map equal to the transaction's expected result.
A service already at its original map needs no write. An unexpected third map
is preserved for manual review rather than assuming it belongs to the app.
Missing services do not prevent rollback of the remaining available services.
Any unresolved failure retains the recovery record and reports an error.

After an interruption, **Undo app changes** first reverses the interrupted
transaction. If earlier app mappings remain, the editor shows their draft and
explains that another Undo removes them. Apply remains blocked while recovery
is pending. Normal Undo clears the app draft; unrelated native mappings remain.

## Reconnects and older backups

Registry IDs are valid only for the current attachment. The backup includes
the macOS boot-session UUID to prevent an old record matching recycled IDs
after a restart. A new USB parent, service set or boot session is refused;
the app never assigns an old baseline by list position or product ID.

The earlier unversioned `{baseline, applied}` file is migrated only when
exactly one live service has the recorded applied map. With multiple services
there is no evidence of which baseline belonged to which service, so migration
is refused and the old file is preserved.

For a stale attachment or ambiguous older backup, first use **Load saved
draft** and record the desired mappings. Quit the app, then archive
`host-remap-backup.json` from the app's data directory to a separate location.
Keep that archive until the old mappings and any pending recovery have been
reviewed. Reopen the app to read the new attachment and enter the reviewed
draft. Apply captures fresh baselines. Do not copy old per-service baselines
onto a new connection. This conservative recovery step is required because a
matching VID/PID does not prove that it is the same physical keyboard.

On macOS the data directory is
`~/Library/Application Support/io.github.wsclx.ak820pro-modder`.
Do not downgrade while an app mapping or recovery record is active: older
versions cannot read version 2. Undo successfully before changing versions.

## Validation

Mocked backend and persistence tests exercise different service baselines,
unrelated external changes, preflight conflicts, no-op and partially effective
failed writes, failed rollback, disconnection, interrupted recovery, backup
failures, legacy migration and changed attachment identities. Default tests do
not write native mappings.

Two ignored native tests are available on macOS:

```sh
cargo test -p ak820pro-app --lib inspect_usb_services_read_only -- --ignored --nocapture
cargo test -p ak820pro-app --lib usb_remap_round_trip_and_undo -- --ignored --nocapture
```

The first only reads ancestry and mappings. The second temporarily adds an
unused F24 identity mapping to every selected service, verifies read-back, then
restores each baseline. It uses a separate temporary recovery directory and
retains it if cleanup fails; it does not use the app's saved profiles.

On 25 October 2026 both tests passed on a wired ANSI AK820 (`bcdDevice 0x0110`,
which is a USB release number, not a verified firmware version). Two services were
resolved to one USB parent; F24 identity was read back on both and both empty
mapping tables were restored. This validates native property writes and Undo,
not a physical key press or the integrated TFT/lighting protocol.

Native service handling is adapted from Peter's earlier
`host_controls/macos.rs` fork, with physical-device targeting and upstream
backup/Undo integration. The macOS SDK declares the IOHID service and IORegistry
APIs used here in `IOHIDServiceClient.h`, `IOHIDEventSystemClient.h` and `IOKitLib.h`.
