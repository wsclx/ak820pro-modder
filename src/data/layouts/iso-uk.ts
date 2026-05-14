import json from "./iso-uk.json";
import type { KeyboardLayout, PhysicalKey } from "./types";

/**
 * AK820 Pro **ISO-UK** physical layout (British English).
 *
 * **Status: 🧪 unverified.** Hand-derived from the ISO-DE reference;
 * the ISO physical structure (L-shape Enter, 1 u Backspace, narrow
 * L-Shift with the `<>\|` key to its right) is identical to ISO-DE,
 * so the difference is purely *which legends* are printed and *which
 * HID codes* the firmware emits at the divergent positions:
 *
 * | Slot | Position | ISO-DE | ISO-UK |
 * |---|---|---|---|
 * | 16 | Grave | `^°` (HID 53) | `` ` ¬`` (HID 53) |
 * | 17–26 | Number row | `1 !` … `0 }` | `1 !` … `0 )` |
 * | 27 | After 0 | `ß ?` (HID 45) | `- _` (HID 45) |
 * | 28 | Right of 27 | `´ \`` (HID 46) | `= +` (HID 46) |
 * | 43 | After P | `Ü` (HID 47) | `[ {` (HID 47) |
 * | 44 | Right of 43 | `+ ~ *` (HID 48) | `] }` (HID 48) |
 * | 58 | After L | `Ö` (HID 51) | `; :` (HID 51) |
 * | 59 | Right of 58 | `Ä` (HID 52) | `' @` (HID 52) |
 * | 97 | ISO home-extra | `# '` (HID 50) | `# ~` (HID 50) |
 * | 98 | ISO bottom-extra | `< > \|` (HID 100) | `\ \|` (HID 100) |
 * | 65 | Bottom row 1st | `Y` (HID 29) | `Z` (HID 29) |
 * | 71 | Bottom row 7th | `M µ` (HID 16) | `M` (HID 16) |
 *
 * HID codes track the USB HID spec's position-relative-to-US-QWERTY
 * convention, which both ISO-DE and ISO-UK obey identically. The OS's
 * input-method layer is what translates these codes to the printed
 * letter based on the user's macOS keyboard layout setting.
 *
 * If you own a real ISO-UK AK820 Pro and the on-screen surface looks
 * wrong, please open an issue with photos.
 */
export const ISO_UK_LAYOUT_ROWS: PhysicalKey[][] = json as PhysicalKey[][];

export const ISO_UK_LAYOUT_SLOTS: number[] = ISO_UK_LAYOUT_ROWS.flat().map(
  (k) => k.slot,
);

export const ISO_UK_LAYOUT: KeyboardLayout = {
  id: "iso-uk",
  displayName: "ISO-UK",
  description:
    "British English / ISO 75 %. Same physical structure as ISO-DE; UK legends and HIDs. 🧪 unverified — no hardware confirmation.",
  rows: ISO_UK_LAYOUT_ROWS,
};
