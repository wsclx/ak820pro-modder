# Startup and menu bar

**App settings** is available without a connected keyboard. Both options are
off on a fresh installation; existing `app-preferences.json` preferences are
preserved when upgrading from the contributor build.

- **Start at login** registers the installed app for this user with
  `--background`. On macOS it uses Tauri's LaunchAgent backend. A login launch
  keeps the window and Dock icon hidden; the AK820 menu remains available.
  Install the app bundle before enabling this option. Moving it later requires
  disabling and enabling the option again to update its registered path.
- **Keep running when the window closes** hides the existing window, preserving
  unsaved drafts and existing background work. It does not start an additional
  controller. Without this option, closing the main window exits the app.
- The **AK820** menu offers Show, App settings, Hide window and Quit. Hide is
  also available explicitly in App settings regardless of the close preference.
  The app menu's **App settings…** item (Command+,) opens the same page.
- **Quit**, including Command+Q, always exits. Background running does not
  intercept explicit quit requests. Keyboard remapping keeps its existing
  explicit Apply/Restore behavior; this feature does not apply or clear maps.
- Reopening through the Dock or Applications shows the existing window. The
  single-instance plugin is registered before the other plugins: another manual
  launch shows the first instance, while another `--background` launch leaves
  its visibility unchanged.

On macOS a window shown interactively keeps its Dock icon as a second way to
reopen it after hiding. The status item uses a stable autosave name and seeds
only its initial position toward the right edge, where a display notch cannot
cover it. A subsequent Command-dragged position is retained.

If the menu icon fails to initialize, a background launch shows the window.
Close-to-background also keeps the window open in that case. A failed preference
write leaves the previous in-memory close behavior in effect. Failure to read
login registration is reported separately so the other controls remain usable.

## Persistence and scope

The close preference is stored atomically in `app-preferences.json` in the app
data directory. Login registration is read from the platform registration, not
duplicated in the preferences file. On macOS the registration is
`~/Library/LaunchAgents/AK820 Pro Modder.plist`, with `RunAtLoad` and no
`KeepAlive`. Disabling removes this registration. The status reports registration
presence; macOS may independently block it in System Settings → General →
Login Items & Extensions.

Enabling these options, showing/hiding the window and opening settings do not
write keyboard lighting, sleep or TFT settings. Existing automation shortcuts
continue while hidden. Saved setups and automatic switching by foreground app
are outside this change.

The plugins use the current Tauri 2.12 runtime. Tauri's [autostart plugin](https://v2.tauri.app/plugin/autostart/)
provides login registration; its [single-instance plugin](https://v2.tauri.app/plugin/single-instance/)
handles subsequent launches. This contribution targets macOS; Windows and Linux
native behavior requires separate validation.

## Validation checklist

Run the contributor checks in `CONTRIBUTING.md`. Unit coverage includes default
and existing preferences, close behavior with a missing tray, persistence across
reload, failed preference replacement and exact background-argument matching.

Native checks should use a separately identified test bundle and empty app data,
so the installed app's data and login item remain untouched:

1. Normal launch shows the window. Both options are off with fresh data.
2. Enable keep-running, close the window, reopen through the tray and verify
   that a draft remains. Relaunch and verify the preference persists.
3. Show, Hide and App settings work from the tray, including after a quiet launch.
4. A second manual launch shows the same process; a second background launch
   leaves its window hidden.
5. Enable login launch, inspect the registration and its `--background` argument,
   then disable it and verify removal. Test the registered launch without
   logging the user out. A real logout/login remains a separate manual check.
6. Quit from the tray, Command+Q and close-with-background-disabled each exit.
7. Confirm the production app, data, login item and keyboard maps are unchanged.
