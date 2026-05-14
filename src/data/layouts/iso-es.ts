import json from "./iso-es.json";
import type { KeyboardLayout, PhysicalKey } from "./types";

/**
 * AK820 Pro **ISO-ES** physical layout (Spanish).
 *
 * **Status: 🧪 unverified.** Hand-derived from the ISO-DE reference;
 * physical structure is identical to other ISO variants (L-shape
 * Enter, 1 u Backspace, narrow L-Shift with the `<>` key to its
 * right at slot 98). What changes is the legends + special-character
 * positions and the Spanish-language captions on the modifier keys.
 *
 * Notable Spanish-specific positions:
 *
 * - slot 27: `' ?` (instead of `- _`)
 * - slot 28: `¡ ¿` (instead of `= +`)
 * - slot 43: `` ` ^`` (instead of `[ {`)
 * - slot 44: `+ *` (instead of `] }`)
 * - slot 58: `Ñ` (Spanish ñ replaces ANSI's `; :`)
 * - slot 59: `´ ¨` (instead of `' "`)
 * - slot 97: `Ç` (instead of `# ~` / `# '`)
 * - Modifier captions: Bloq Mayús, Mayús, Intro, Retroceso, Espacio
 *
 * HID codes track the USB HID spec's position-relative-to-US-QWERTY
 * convention. macOS's "Spanish" keyboard layout translates the codes
 * to the printed legends at the OS layer; the firmware just emits
 * the position code.
 *
 * If you own a real ISO-ES AK820 Pro and the on-screen surface looks
 * wrong, please open an issue with photos.
 */
export const ISO_ES_LAYOUT_ROWS: PhysicalKey[][] = json as PhysicalKey[][];

export const ISO_ES_LAYOUT_SLOTS: number[] = ISO_ES_LAYOUT_ROWS.flat().map(
  (k) => k.slot,
);

export const ISO_ES_LAYOUT: KeyboardLayout = {
  id: "iso-es",
  displayName: "ISO-ES",
  description:
    "Spanish / ISO 75 %. Same physical structure as ISO-DE; Spanish legends and modifier captions. 🧪 unverified — no hardware confirmation.",
  rows: ISO_ES_LAYOUT_ROWS,
};
