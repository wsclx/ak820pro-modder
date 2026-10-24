# ANSI AK820 feature-report per-key colours

> **Imported from vespovios/ak820pro-modder** (branch
> `feature/ak820-800a-macos` @ `cc2a8df`, author Peter). The protocol,
> evidence and hardware validation apply here: `legacy_rgb.rs` implements
> this wire format and its tests. The sections "App behaviour", "Named
> colour profiles", "Shortcut and sharing" and their validation notes
> describe vespovios' app. This repository has none of that: its paint view
> starts black (no read-back, no local `feature-rgb.json`), writes only on
> Apply, and has no colour profiles or profile shortcut. The RGB sessions
> here send START and FINISH with flag 0 as verified below; the other
> settings use flag 1 (adamauthor's shared transaction since his 0.9.6,
> never tested with the RGB sessions). Each write waits 35 ms before and
> 50 ms after instead of 35 ms each.

Verified on the user's wired `0C45:800A` keyboard on 2026-10-23. The user
confirmed W red, A green, S blue and D yellow against a purple background.

## Evidence

The [vendor's ANSI Windows driver download](https://ajazzstore.com/blogs/software/ajazz-ak820-pro-driver)
links to `AJAZZ_AK820_Pro_Triple_mode_0.85inch_Screen_RGB_V1.0.0.5.zip`.
The package was extracted locally for static analysis; its executable was
not run or redistributed. Analysis covered `DeviceDriver.exe` and its
`layouts/rgb-keyboard.xml` mapping.

- Package SHA-256: `98da8b9c801d413c1e84c4e08a2186922c1c297b9a173148123903b3867b8cf5`
- Executable SHA-256: `14b6919b59b86f6bd1465e544032b41ca4f12ca35e36bd81d3ab33aca14a605a`
- `0044B900`: selects custom lighting mode and brightness.
- `0044BA10`: builds and uploads an indexed RGB bank, then commits it.
- `0044EED0`: transport; splits long buffers into raw 64-byte reports.
- `00427E10`: separate live-colour path; not needed by this implementation.
- `0042AEE0` / `0044F3A0`: colour-bank read attempt; not a TFT upload path.

Earlier community notes mislabel `0042D6F0` (`19` / `15`) as lighting.
Its event packing, delays and action types identify it as a macro path.
No `19`/`15` writes were used for this feature.

## Wire format

Every hidapi buffer is report ID `00` followed by 64 payload bytes.
Each write settles for 35 ms and is followed by a feature read.
All feature commands also wait 35 ms after each read before the next
write, matching the vendor's pre-write pacing. The first packaged-editor
attempt with only the read-side delay rejected SAVE; a deliberate retry
worked, which prompted this timing correction. Switching to a global
effect exposed the same issue, so pacing is shared across feature commands.
Rejected commits are still
reported as failures and never automatically retried.

Custom mode selection:

1. `04 18`, byte 8 = 0 (START).
2. `04 13`, byte 8 = 1 (lighting preamble).
3. Raw mode data: byte 0 = `80`, byte 9 = brightness 0–5,
   bytes 14–15 = `AA 55`; all other bytes zero.
4. `04 02`, flag 0 (SAVE), then `04 F0`, flag 0 (FINISH).

Colour-bank upload:

1. START, flag 0.
2. `04 23`, byte 8 = 9 (RGB bank preamble).
3. Nine **raw** 64-byte feature payloads, without packet headers or block IDs.
4. SAVE, flag 0, then FINISH, flag 0.

The contiguous bank is 576 bytes, initially zero. Each mapped LED uses
four bytes at `light_index * 4`: `[light_index, R, G, B]`. LED indices
come from the vendor's ANSI layout and are one-based with row gaps.
Bytes 574–575 are `AA 55`; unused/reserved locations remain zero.

Examples: W = LED 39, A = 56, S = 57, D = 58, ANSI Enter = 85,
Space = 94, Backspace = 103. `legacy_rgb::ANSI_LED_SLOTS` translates the
app's existing physical UI slots to these firmware indices. The key below
Delete is labelled Home in the feature paint view, matching the vendor XML.

Command responses must match `04 <command> ... 01` at byte 3. Raw RGB
responses echo colour data and have no command status field. The successful
SAVE and FINISH responses plus human visual confirmation establish operation.

The library validates the complete map before opening a transaction. A
failed transfer attempts FINISH, skips the bank SAVE and is reported as an
error. It is not automatically replayed after a stale handle or partial
write. Mode selection occurs first, matching the vendor; a failed bank
write can therefore leave custom mode selected with the earlier colours.

## App behaviour

Select **ANSI**, open **Lighting → custom**, choose a brush, paint keys,
then **Apply to keyboard**. Fill all, Clear all and brightness are supported.
The feature path uses explicit Apply, avoiding a full colour-bank upload for
every paint click. Switching to a global effect hides the custom bank;
select custom and Apply to restore it.

The last successfully applied map and brightness are stored atomically in
`~/Library/Application Support/io.github.wsclx.ak820pro-modder/feature-rgb.json`.
The preview and **Reload saved** use this local profile, explicitly not
device read-back. Getter experiments returned repeated/partial snapshots
from GET_FEATURE and no complete bank through the input queue, so no
unverified getter is exposed. No colour upload runs merely by opening a view.

### Named colour profiles

The Colour profiles section saves up to 100 named maps, each with its own
brightness, in `feature-rgb-profiles.json` in the same application data
folder. The original last-applied profile is copied to **Current colours**
when the library is first opened. Subsequent keyboard uploads do not
overwrite any named profile.

- Paint keys, enter a profile name, and choose **Save as new**.
- Select a saved profile and choose **Use profile** to load and apply its
  colours and brightness in one upload. This replaces editor drafts.
- **Save changes** updates the selected entry from the editor, including
  a changed name. Saving does not change the keyboard's lights.
- **Delete profile** requests confirmation and removes only that entry;
  it does not change the keyboard's current colours.

Profile writes are atomic and serialized. Names are trimmed, limited to
64 characters and checked for duplicates without case sensitivity.

### Shortcut and sharing

Control + Option + Command + P cycles saved profiles in list order and
wraps around, including brightness. The app must stay open; it can be in
the background. The Cycle profiles shortcut field can record a different
combination, followed by Enable / update shortcut. Disable shortcut releases
it. Registration conflicts are reported and preserve the previous binding.
The setting persists across restarts and is independent of automation and
macro trigger bindings. Holding the shortcut or pressing during an upload
does not queue a series of bank writes.

Export selected or Export all opens a native Save dialog for a portable
JSON file. Import profiles opens a native file chooser. Import validates
the entire file before adding anything, never overwrites existing profiles,
and appends “(imported N)” to colliding names. Import does not upload to the
keyboard; select the imported entry and Use profile to apply it.

The format uses `format: "ak820-colour-profiles"`, `version: 1`,
`keyboard: "ak820-pro-ansi"`, and a `profiles` array. Each entry contains
`name`, `brightness` (0–5), optional `base_color` (default black), and
`keys`, a map of key names to six-digit RGB hex strings (optional `#`).
Unspecified keys take the base colour. See
`docs/examples/gaming-colours.json` in the vespovios fork for a prepared
profile that can be imported directly. Exports include all 81 visible keys.

Key names (case sensitive):

```
Esc F1 F2 F3 F4 F5 F6 F7 F8 F9 F10 F11 F12 Delete
Backquote 1 2 3 4 5 6 7 8 9 0 Minus Equal Backspace Home
Tab Q W E R T Y U I O P BracketLeft BracketRight Backslash PageUp
CapsLock A S D F G H J K L Semicolon Quote Enter PageDown
ShiftLeft Z X C V B N M Comma Period Slash ShiftRight ArrowUp
ControlLeft MetaLeft AltLeft Space AltRight Fn ControlRight
ArrowLeft ArrowDown ArrowRight
```

This format describes ANSI per-key colours. Files from other vendors' apps
or firmware formats require conversion; they are not treated as colour JSON.
Portable exports omit local IDs, shortcut settings, macros, and machine paths.

The output-report keyboard's original per-key path is preserved. Only the
ANSI feature-report mapping has been validated on this hardware; other
physical layouts are gated in the feature paint editor.

## Validation

The settings-neutral `feature_rgb_probe` example verifies connection. Pass
`-- --write-test` deliberately to apply the WASD pattern above, or
`-- --restore-purple` to return to global purple. Close the app first so its
exclusive HID handle is released.

Tests cover the ANSI matrix and RGB byte order, the bank delimiter and
reserved padding, both transaction sessions, nine raw data reports,
malformed inputs and cleanup without committing a failed bank.

Hardware validation on the user's wired ANSI AK820:

- Production library uploaded W red, A green, S blue, D yellow on a purple
  base; the user confirmed all four physical keys.
- The installed paint editor changed E to cyan; the user confirmed E changed
  while WASD stayed unchanged. The saved profile retained those colours
  after restarting the app.
- With the shared read/write pacing, the production global-purple probe
  completed without a rejected SAVE. The final installed app also applied
  the saved custom profile on its first attempt after launch, then switched
  to static lighting successfully.
- The final app passes strict ad-hoc code-signature verification, and its
  Apple Silicon DMG passes `hdiutil verify`.
- After the user re-added the final Applications build to Accessibility,
  macro playback typed `hi` in the app's test area. The app was left open
  on the custom lighting editor with the confirmed profile loaded.
- The workspace suite passes 63 unit tests, with one native integration
  test ignored by default. TypeScript/Vite and Rust formatting checks pass.

Named-profile validation (66 unit tests after adding three persistence tests):

- The previous upload migrated to Current colours, identical to the backup.
- The installed UI saved a second profile with E orange, updated its brightness
  to 3, and applied it with a keyboard commit acknowledgement.
- Use profile restored Current colours with E cyan and brightness 5; the
  original profile was unchanged by applying or saving the other entry.
- Both named entries remained available after quitting and reopening the app.
  The temporary test entry was then deleted through the UI.
- The final app's ad-hoc signature and Apple Silicon DMG both verify.
- After renewed Accessibility approval and a restart, the named-profile
  build replayed the saved macro and typed `hi` into its test area.

Shortcut/sharing validation (71 unit tests):

- Native Open imported the prepared Gaming example without changing lights.
- Native Save exported Gaming with all 81 key colours and brightness 3;
  the resulting file's format and values were checked independently.
- The user physically pressed Control + Option + Command + P and confirmed
  that the keyboard switched to Gaming. The final app reported a successful
  upload and updated the selected profile and paint preview.
- Recording and registering a different shortcut, then restoring the
  documented default, succeeded. The enabled setting is persisted.
- The final build restored Current colours, retained Gaming as an example,
  and after renewed Accessibility approval replayed the macro test (`hi`).
- Tests cover cycle order/wraparound, duplicate colour maps, shortcut
  validation, portable round-trips, invalid key names/values/layout/version,
  oversized files, duplicate import names and all-or-nothing batch imports.
