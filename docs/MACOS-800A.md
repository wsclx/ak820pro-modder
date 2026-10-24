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
name because Sonix IDs are reused by unrelated keyboards. This profile's
vendor handles use non-exclusive opens (the TFT worker process opens the
control endpoint the app already holds); web-profile boards keep hidapi's
exclusive open. Standard keyboard collections of this profile are not
opened by descriptor diagnostics. A device counts as this profile only if it
exposes FF13 and no FF67.

Web commands are never sent to the older profile. The capability report and UI
route supported operations to their correct protocol. USB product release 0111
is not used to infer firmware 1.11 or to enable commands.

## Features on this revision

| Feature | Legacy implementation | Verification |
|---|---|---|
| Static images / GIFs | Separate acknowledged 4096-byte transport | Full-screen quadrants and 8-frame GIF visually confirmed |
| Lighting effects | Feature-report mode transaction (START/13/data/SAVE/END); 20 standard modes | Static cyan visually confirmed with GIF still running |
| Per-key RGB (custom) | Mode 80 session plus 576-byte bank session, ANSI slot table | WASD + E visually confirmed (vespovios); paint view by adamauthor |
| Clock sync | Local binary date/time, START/28/data/SAVE/END | Completed feature handshakes; clock page not visually checked |
| Sleep timer | Never / 1 / 5 / 30 minutes, START/17/data/SAVE/END | Five-minute setting sent; idle timeout and persistence not observed |
| Battery | Query 20 01; FF preserved as wired/full sentinel | Actual FF reply; percentage/charging not invented |
| Key remapping on Mac | Device-scoped hidutil, baseline backup, readback and undo | Temporary A→A and F24→F24 round trip restored initial mappings |
| Keyboard macros on Mac | App-local recorder, global shortcuts, native event playback | Six recorded key events survived app restart; manual playback typed фис under Russian input; owner confirmed the global shortcut; Stop cancelled before input |
| Host automations / profile sync | Existing host paths | Original behavior retained |

Onboard key/Fn tables, onboard macros, knob binding, live audio RGB,
factory TFT selection and live Now Playing TFT are not decoded for FF13.
The UI explains these limits and avoids unsupported writes. The Mac fallbacks
are explicitly labeled and never present their state as onboard configuration.
Preset options are checked before any writes to avoid partially applying a
preset that requires an unsupported onboard key table.

### Feature control packets

Every feature buffer is report ID 0 followed by 64 payload bytes. Control
payloads begin `04 command subcommand`, with a little-endian value at bytes
8–9. Each send waits 35 ms first (the vendor driver's `cmd_delaytime`;
vespovios and gha-projects/ak820-mac saw the final `04 02` refused or ignored
without it), then 50 ms before its feature response; control echoes must
match. Every setting ends with `02(value=0)` (SAVE) and `F0(value=1)` (END);
see [Feature transactions](#feature-transactions). Lighting uses
`18(value=1)`, `13(value=1)`, mode data. Mode data contains mode, RGB,
rainbow boolean at 8, brightness/speed/direction at 9/10/11, and `55 AA` at
14–15. Static is Breath (7) at speed zero; Off is SingleOn (2) with
brightness zero. Sleep uses `18`, `17(sub=1,value=1)`, then data with its
step at 8 and `AA 55` at 62–63. Clock uses `18`, `28`, data
`[00 01 5A yy month day hour minute second]`, 04 at 10 and `AA 55` at 62–63.
All whole feature transactions share a process lock with the TFT worker to
prevent interleaving.

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
After granting Accessibility, quit and reopen the app once: during testing, a
running process saw the new AX permission but posted no text until restarted.
Both Accessibility trust and event-posting access are checked before playback.
Locally ad-hoc-signed development builds have a changing code identity; macOS
may require permission again after rebuilding. No signing or TCC protection is
weakened to avoid that check.

## Legacy GIF upload

An explicit confirmation is required before the older upload sequence.
The image command can replace the previous saved animation before the
transfer finishes. No device write has been used merely to diagnose it.
The GUI asks before sending; the CLI requires `--allow-legacy`.

The older path uses 65-byte hidapi feature buffers: report ID 0, then the
full 64-byte packet beginning 04. It uses START (04 18), image configuration
(04 72 01, chunk count in bytes 8-9 of the packet), 4096-byte raw output
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

Byte 2 of the image configuration is the one-based destination slot, not a
transport variant (vendor ANSI driver V1.0.0.5 sends the selected LCD entry
plus one; its layout declares one GIF slot). Earlier builds sent slot 3,
which was acknowledged and played on this keyboard. On another ANSI 800A,
slot 3 showed a static image only briefly among other animation content and
slot 1 kept it; see vespovios/ak820pro-modder@3264608 and its
docs/COMPATIBILITY.md. Slot 1 still needs to be confirmed on this keyboard.
Other tools use 1 (gha-projects, DarkTiengo/ak820ctl) or 2
(allshouldbeready), so the slot is a parameter: default 1, the CLI takes
`--slot 1|2|3`, other values are refused before anything is sent.

## Legacy per-key RGB

Ported from vespovios/ak820pro-modder (docs/PER-KEY-RGB.md there). Two
feature sessions: START (04 18), lighting (04 13 flag 1), mode packet
`80` with brightness 0-5 in byte 9 and `AA 55` at bytes 14-15, SAVE (04 02),
END (04 F0); then START, 04 23 flag 9, nine unframed 64-byte blocks of the
576-byte colour bank (`[index, R, G, B]` at index × 4, `AA 55` in the last
two bytes), SAVE and END. Command replies must carry status 1 in byte 3;
data replies echo colour bytes and are not checked. END is always sent, and
a failed bank is never saved. The slot table is ANSI only and was verified
there with WASD and E; the ISO key left of Z (slot 98) has no LED index.

`set_lighting(custom)` runs the first session only (mode 80 renders the
stored bank) and `set_custom_led` the second, so the app's paint view and
`ak820 rgb fill` / `rgb rainbow` use the same calls as on web firmware.
The firmware has no bank read-back; the paint view starts black and Apply
writes the whole map. Each write is saved, so audio-reactive streaming
stays disabled.

## Feature transactions

Every setting (effect, sleep, clock, custom mode, colour bank) is one
session: START (04 18), the command preamble, its data packets, SAVE
(04 02, flag 0), END (04 F0). Command replies must echo `04 <command>`
with status 1 in byte 3; data replies echo their bytes and are not checked.
END is sent after a failure too.

Naming: this document keeps adamauthor's labels SAVE (`04 02`) and END
(`04 F0`). What either does inside the firmware is not established:
gha-projects/ak820-mac calls `04 F0` "save to flash" and `04 02` a commit
with a checksum. The code therefore names them `COMMIT` and `FINISH`
(`crates/ak820-protocol/src/legacy.rs`).

Measured on this keyboard: without SAVE, END answers FF and for about 2 s
START, the preamble and any data are refused (status FF or an all-zero
report). Earlier builds wrote effects that way, so quick successive
changes could be dropped, and an image upload right after an effect lost
its first block ("No acknowledgement for TFT chunk 1"). With SAVE,
back-to-back writes all return 01 and take about 0.3 s. START still waits
up to 3 s for a keyboard left busy by another program.

Persistence, tested: static green with SAVE, then static red without SAVE
(END answered FF), then a full power-off. The keyboard came back red, so a
lighting write is stored even without SAVE. Both runs still sent `04 F0`;
whether a write without `04 F0` would stay in RAM has not been tested.
Host-driven lighting that changes often (notifications, per-app colours,
audio-reactive streaming) therefore wears the flash, and so does every
image upload, which rewrites the stored animation. The app only writes on
an explicit user action: no auto-apply in the Lighting and paint views, no
automatic TFT restore, no Now-Playing or audio-reactive streaming.

The image upload checks every command status, waits for a busy START the
same way, and sends END after a failed transfer. The cross-process
transaction lock now waits up to 12 s instead of failing at once. A
lighting change during an upload of up to about 70 blocks (a 49-block
upload takes about 8 s) is applied after it; during a longer upload, such
as a 30-frame GIF of about 240 blocks (roughly 40 s), it fails with
"Another keyboard operation is running" and nothing is written.

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
