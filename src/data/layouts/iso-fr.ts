import json from "./iso-fr.json";
import type { KeyboardLayout, PhysicalKey } from "./types";

/**
 * AK820 Pro **ISO-FR** physical layout (French AZERTY).
 *
 * **Status: 🧪 unverified.** Hand-derived from the ISO-DE reference;
 * physical structure is the same (L-shape Enter, slot 98 `<>` left
 * of bottom row), but AZERTY swaps a few letter positions relative
 * to QWERTY/QWERTZ and rearranges symbols in the number row.
 *
 * Key positional swaps vs ANSI/UK:
 *
 * | Slot | ANSI label | AZERTY label |
 * |---|---|---|
 * | 33 (top-left letter) | Q | **A** |
 * | 34 | W | **Z** |
 * | 49 (home-left letter) | A | **Q** |
 * | 58 (home, 11th key) | `; :` | **M** |
 * | 65 (bottom-left letter) | Z | **W** |
 * | 71 (bottom, 7th key) | M | **`, ?`** |
 *
 * **Why the HID codes match ISO-DE despite the label swaps**: the AK820
 * Pro firmware emits a USB-HID code based on *physical PCB position*,
 * not on the printed legend. macOS's "French (AZERTY)" input source
 * then translates each code to the legend the user expects. So slot
 * 33 always emits HID 20 ("Q-position" in USB-HID terms), and macOS-FR
 * outputs "A" — matching the printed cap.
 *
 * Number-row symbols all shift one position right (AZERTY accesses
 * digits via Shift; the unshifted layer is `& é " ' ( - è _ ç à`).
 *
 * If you own a real ISO-FR AK820 Pro and the on-screen surface looks
 * wrong, please open an issue with photos.
 */
export const ISO_FR_LAYOUT_ROWS: PhysicalKey[][] = json as PhysicalKey[][];

export const ISO_FR_LAYOUT_SLOTS: number[] = ISO_FR_LAYOUT_ROWS.flat().map(
  (k) => k.slot,
);

export const ISO_FR_LAYOUT: KeyboardLayout = {
  id: "iso-fr",
  displayName: "ISO-FR",
  description:
    "French AZERTY / ISO 75 %. Same physical structure as ISO-DE; AZERTY positional swaps + French legends and captions. 🧪 unverified — no hardware confirmation.",
  rows: ISO_FR_LAYOUT_ROWS,
};
