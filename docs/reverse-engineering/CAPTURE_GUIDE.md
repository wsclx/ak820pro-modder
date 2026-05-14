# USB pcap capture guide

How to capture a USB-HID conversation between the **official AJAZZ Windows tool** and the **AK820 Pro keyboard**, on a Mac, using a free VM. The output is a single `.pcapng` file we can diff against our own implementation to find missing wire-protocol steps.

This is the highest-leverage thing a contributor can do for the project. **One good pcap unblocks weeks of speculation.**

---

## What you need

| | Item | Where to get | Notes |
|---|---|---|---|
| 1 | **UTM** | <https://mac.getutm.app> · ~150 MB | Free Apple-Silicon VM host. Open Source. |
| 2 | **Windows 11 ARM Insider Preview** ISO | <https://www.microsoft.com/en-us/software-download/windowsinsiderpreviewARM64> | Free for personal use. ~5 GB download. |
| 3 | **AJAZZ Windows tool** | The exe we already have: `AJAZZ_AK820 Pro_…V1.0.0.5.exe` | This is the **ANSI build** but the wire protocol is layout-agnostic — it's fine for TFT / RGB / system captures. |
| 4 | **USBPcap** | <https://desowin.org/usbpcap/> · ~5 MB | Free, single installer, integrates with Wireshark. |
| 5 | **Wireshark** | <https://www.wireshark.org/download.html> · ~80 MB | The capture viewer. Free. |
| 6 | **AK820 Pro** in wired USB-C mode | — | 2.4 GHz dongle works too; BT is harder (different HID stack). |

---

## Setup (one-time, ~30 min)

### 1. Install UTM and build the Windows VM

1. Download UTM, drag into Applications, launch.
2. **Create a new VM** → **Virtualize** (not Emulate; faster on Apple Silicon).
3. Pick **Windows**.
4. **Browse** the Windows 11 ARM ISO you downloaded.
5. Resources: **4 GB RAM, 64 GB disk**. Drive sharing: not needed.
6. Run the installer to completion. **Skip the Microsoft account** when offered (Insider Preview lets you, regular ARM Win11 requires offline-account workarounds).

### 2. Wire up USB pass-through

1. In UTM, with the VM shut down, open **Edit VM → Devices → USB**.
2. Set USB controller to **USB 3.0 (xHCI)**.
3. Add a new **USB Filter** for the keyboard. Click **Add USB Device** with the AK820 Pro plugged in — it should show as **Sonix Tech 0c45:8009**. Save.
4. Boot the VM.

### 3. Install the AJAZZ tool + USBPcap + Wireshark inside Windows

Inside Windows:

1. **AJAZZ tool**: copy the `AJAZZ_…V1.0.0.5.exe` into the VM (drag-and-drop or shared clipboard). Run it. It might auto-install a driver — let it.
2. **USBPcap**: download + install. It needs a reboot of Windows.
3. **Wireshark**: download + install. During setup it'll offer to install **USBPcap support** — say yes.

### 4. Hand off the keyboard to Windows

- In the UTM window bar, click the USB icon → toggle the AK820 Pro to be claimed by the VM. macOS releases it, Windows grabs it.
- Inside Windows, plug-test: open Notepad, type a few keys — they should appear. If not, replug, re-toggle.
- Open the AJAZZ tool. It should detect the keyboard and show its layout.

---

## The capture itself (5–10 min)

### Setup the capture

1. In Windows, launch **Wireshark**.
2. The interface list shows several `USBPcap1`, `USBPcap2`, etc. — one of them is the bus the AK820 Pro is on. **Pick the one whose link layer reports USB-HID traffic when you press a keyboard key in Notepad.**
3. Add a **display filter** to cut noise: `usb.idVendor == 0x0c45` (Sonix vendor). Press the blue filter-apply arrow.

### Capture **one** atomic action per file

Each pcap should cover ONE thing. Mixing several actions into one capture makes the diff harder.

**Priority list of captures we need** (in this order):

1. **TFT image upload (most wanted).** Save a known image (e.g. a fully red 128×128 PNG → drag into the AJAZZ tool's TFT/Screen tab → click Upload). Stop capture once the display changes. **Save as `phase5-tft-solid-red-01.pcapng`.**
2. **Per-key RGB enable + paint.** In the lighting tab, switch to per-key mode, colour every key red, click Apply. Save as `phase5-perkey-rgb-all-red-01.pcapng`.
3. **TFT built-in animation selector.** Cycle through every built-in TFT animation slot (0, 1, 2, …) waiting 1 second between clicks. Save as `phase5-tft-builtin-cycle-01.pcapng`.
4. *(Optional)* **Macro upload.** Record a 3-keystroke macro in the AJAZZ tool, assign to F12, save. `phase4-macro-record-write-01.pcapng`.

For each:
- **Hit the red Start-capture button → do the action → hit the red Stop button immediately.** Don't let it run for minutes.
- **File → Save As → name as above → `.pcapng` format.**
- Files should be < 1 MB each. If they're much bigger, your filter isn't tight enough.

### What "good" looks like

Open the pcap in Wireshark. You should see:
- **Outgoing** packets: `0xAA` magic byte at the start of the payload, command byte in position 1.
- **Incoming** packets: `0x55` magic byte, echoed command.
- For TFT/RGB uploads: **dozens** of `URB_INTERRUPT_OUT` packets in rapid succession.
- For a single SET_LED_EFFECT or SET_KEY: **one or two** packets.

If you see noise from other USB devices, tighten the filter.

---

## Hand the file off

Drop the `.pcapng` files into one of:

1. **Drag into this repo** at `docs/reverse-engineering/captures/`. They're gitignored, won't accidentally end up in a commit. We can reference them by name in protocol-finding issues.
2. **Or send the path** if they live elsewhere on the Mac.

Then either:
- Open a [Protocol Finding issue](https://github.com/wsclx/ak820pro-modder/issues/new?template=protocol_finding.yml) describing what you captured, OR
- Ping the maintainers and we'll dig in.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| AJAZZ tool doesn't see the keyboard in the VM | USB pass-through is on the host side | UTM USB icon → toggle the keyboard to the guest |
| Wireshark only shows one USBPcap interface | macOS has reclaimed the device | In the VM's USB menu, re-attach the AK820 Pro |
| Pcap is several MB but mostly mouse traffic | Filter not tight enough | `usb.idVendor == 0x0c45 && usb.transfer_type == 1` (interrupt only) |
| Display filter rejects my expression | Wireshark version syntax difference | Try `usb.vid == 0x0c45` instead |
| `URB_INTERRUPT_OUT` packets are 64 bytes for some, 4104 bytes for others | That's expected — control commands use 64-byte reports, TFT uploads use 4104-byte reports | This is the foot-gun documented in `docs/PROTOCOL.md` |

---

## What we'll do with the capture

For the TFT one specifically:

1. Identify the **command sequence** before the actual upload (we suspect there's a `SET_TFT_BUILT_IN_INDEX` or similar activation step we're missing).
2. Compare the **byte stream of one upload chunk** to what our Rust `Connection::upload_tft_animation()` produces. Any diff is a bug in our encoder.
3. Watch for **response packets** — does the firmware ack each chunk? Does it send a finalisation packet after the last one?
4. If the answer to "what's missing" is obvious from the pcap, the fix is usually one Rust function. If it's not obvious, we may need a few more captures (e.g. of an animation upload vs a static image).

The first pcap typically tells us 90 % of what we need. The remaining 10 % is iterating once we attempt a real upload and see what fires.

Thanks for doing this. ❤️
