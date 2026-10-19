# AK820 0C45:800A on macOS

The attached keyboard reports USB product `AK820`, manufacturer `SONiX`,
VID `0C45`, PID `800A`, bcdDevice `0111` (USB product release, not a verified
GET_DEVICE_INFO firmware version), 12 Mb/s. It exposes nine HID collections.

## Cause and correction

The original PID filter did not include 800A. Adding the PID alone would
misidentify its FF68 screen interface as a web-protocol control interface.
This device instead has the older feature-report profile:

| Interface | Usage page | Reports | Role |
|---|---|---|---|
| 3 | FF13 | 64-byte input/output/feature, report ID 0 | control |
| 2 | FF68 | 64-byte input, 4096-byte output, report ID 0 | TFT data |

The web-driver profile (FF68 control, FF67 TFT/4104) remains supported.
Classification is shared with the UI. The bootloader PID 7140 is excluded
from application enumeration. New PID 800A also requires an AK820 product
name because Sonix IDs are reused by unrelated keyboards. macOS vendor
handles use non-exclusive opens. Standard keyboard collections are not
opened by descriptor diagnostics.

Web commands are never sent to the older profile. The capability report and UI
route supported operations to their correct protocol. USB product release 0111
is not used to infer firmware 1.11 or to enable commands.

## Features on this revision

| Feature | Legacy implementation | Verification |
|---|---|---|
| Static images / GIFs | Separate acknowledged 4096-byte transport | Full-screen quadrants and 8-frame GIF visually confirmed |
| Lighting effects | Feature-report mode transaction; 20 standard modes, no Custom | Static cyan visually confirmed with GIF still running |
| Clock sync | Local binary date/time, START/28/data/F0 | Completed feature handshakes; clock page not visually checked |
| Sleep timer | Never / 1 / 5 / 30 minutes, START/17/data | Five-minute setting sent; idle timeout and persistence not observed |
| Battery | Query 20 01; FF preserved as wired/full sentinel | Actual FF reply; percentage/charging not invented |
| Key remapping on Mac | Device-scoped hidutil, baseline backup, readback and undo | Temporary A→A and F24→F24 round trip restored initial mappings |
| Keyboard macros on Mac | App-local recorder, global shortcuts, native event playback | Validation tests; Accessibility needed for physical playback |
| Host automations / profile sync | Existing host paths | Original behavior retained |

Onboard key/Fn tables, onboard macros, knob binding, per-key RGB, live audio RGB,
factory TFT selection and live Now Playing TFT are not decoded for FF13.
The UI explains these limits and avoids unsupported writes. The Mac fallbacks
are explicitly labeled and never present their state as onboard configuration.
Preset options are checked before any writes to avoid partially applying a
preset that requires an unsupported onboard key table.

### Feature control packets

Every feature buffer is report ID 0 followed by 64 payload bytes. Control
payloads begin `04 command subcommand`, with a little-endian value at bytes
8–9. Each send has a 50 ms delay and a feature response; control echoes must
match. Lighting uses `18(value=1)`, `13(value=1)`, mode data, `F0(value=1)`.
Mode data contains mode, RGB, rainbow boolean at 8, brightness/speed/direction
at 9/10/11, and `55 AA` at 14–15. Static is Breath (7) at speed zero; Off is
SingleOn (2) with brightness zero. Sleep uses `18`, `17(sub=1,value=1)`, then
data with its step at 8 and `AA 55` at 62–63; no guessed commit is appended.
Clock uses `18`, `28`, data `[00 01 5A yy month day hour minute second]`, 04 at
10 and `AA 55` at 62–63, then `F0`. All whole feature transactions share a
nonblocking process lock with the TFT worker to prevent interleaving.

### Mac keyboard controls

Remapping matches only VID 0C45, PID 800A, primary usage page 1 / usage 6.
It never changes the MacBook keyboard. The app reads existing mappings,
records a baseline before the first write, verifies the result and offers
undo. It refuses an ambiguous device match or a conflicting external change.
No root helper, launch agent, automatic startup remap, or firmware write is
installed. macOS drops mappings after unplug/reboot; Load saved draft and
Apply restore them. Some macOS security policies may deny mappings; failures
are surfaced after readback rather than reported as success.

Mac macros store physical key-down/up events and delays in app data. Recording
is confined to the focused recorder, without a global key logger. Playback
checks Accessibility, validates balanced keys and bounded duration, prevents
overlapping runs, and releases injected keys on cancellation. Manual playback
has a three-second countdown; global shortcuts work from any keyboard and
require the app to remain open. The active Mac input language determines the
characters. Mouse actions and firmware-only Fn combinations are not emulated.

## Legacy GIF upload

An explicit confirmation is required before the older upload sequence.
The image command can replace the previous saved animation before the
transfer finishes. No device write has been used merely to diagnose it.
The GUI asks before sending; the CLI requires `--allow-legacy`.

The older path uses 65-byte hidapi feature buffers: report ID 0, then the
full 64-byte packet beginning 04. It uses START (04 18), image configuration
(04 72 03, chunk count in bytes 8-9 of the packet), 4096-byte raw output
chunks, then SAVE (04 02). Each feature command is followed by a delay and
feature read. Each data chunk requires an input acknowledgement before
continuing; a missing acknowledgement stops the transfer without SAVE.
The byte stream has a 256-byte header, frame count, all per-frame delays
in 2 ms units, RGB565 LE pixels and FF padding. It has no AA 50 chunk header.

Only the exact pair of HID descriptors read from this keyboard is accepted
by the legacy writer. It runs in a supervised child process because macOS
synchronous HID writes may block. Timeout terminates that process; there
are no automatic retries, firmware operations, resets or recovery writes.
Uploads are limited to 30 frames pending further device-capacity evidence.

## Validation on the attached keyboard

- Nine collections enumerated directly through hidapi 2.6.5.
- Both vendor interfaces opened non-exclusively outside the execution sandbox.
- FF13 GET_FEATURE report ID 0 returned 65 bytes successfully, without SET_FEATURE
  or output commands. A sandboxed open returned IOKit `not permitted`; the
  same read-only operation outside the sandbox succeeded.
- Regression tests cover profile roles, the real 4096-byte descriptor,
  old-format GIF timing/pixel preservation and rejection before upload.
- Full application build, GUI launch and CLI dry-run results are recorded
  in the delivered diagnostic report.
- Updated GUI shows connected, nine collections, FF13 as control and FF68 as TFT.
- Static Diagnostic Quadrants transferred successfully; the user confirmed the
  complete four-colour pattern and central white cross on the physical screen.
- The user then explicitly approved the supplied eight-frame test GIF.
- CLI upload completed successfully: eight 128×128 frames, 160 ms each, 65 data
  chunks with acknowledgements, final feature SAVE handshake successful.
- The user confirmed that the white square moves inside the green frame.
- Protocol and app regression tests passed; frontend build and release macOS app
  bundle succeeded. The installed app's ad hoc signature verified successfully.
- Static cyan lighting and the animated GIF were visually confirmed together.
- Battery, clock and five-minute sleep transactions completed on USB.
- Device-scoped hidutil A→A and F24→F24 identity remaps read back correctly, and undo restored
  the exact previous empty mapping.
- Power-cycle persistence, sleep expiry, wireless operation and another
  web-output keyboard have not been physically tested.

## Sources

Protocol research is evidence, not an instruction to send undocumented commands.

- https://github.com/Beattrey/ajazz-ak820-config/blob/main/src/protocol/image.ts
- https://github.com/Beattrey/ajazz-ak820-config/blob/main/src/operations.ts
- https://github.com/kabelklaus/ajazz-ak820-pro/blob/main/app.js
- https://github.com/gusleig/ajazz-ak820-pro-mac/blob/main/docs/protocol.md
- https://github.com/fromagge/ak820-screen/blob/main/PROTOCOL.md

These sources describe different firmware profiles. In particular the modern
web command 50 and the legacy 04 72 sequence are not interchangeable; do not
infer the profile from PID or USB bcdDevice alone.

- https://github.com/gohv/EPOMAKER-Ajazz-AK820-Pro
- https://github.com/Aiacos/ajazz-control-center
- https://developer.apple.com/library/archive/technotes/tn2450/

The official AJAZZ V1.0.0.5 driver was inspected statically, without running
Windows code. Its full key-table path uses 04 11 / 04 27 and a 576-byte table,
not the web firmware's 512-byte mapping. The GUI's logical indices and native
packing are not established sufficiently to reconstruct or back up every
binding. That evidence is deliberately not treated as permission to upload
an experimental whole-keyboard table.
