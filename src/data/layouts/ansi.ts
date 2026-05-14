import json from "./ansi.json";
import type { KeyboardLayout, PhysicalKey } from "./types";

/**
 * AK820 Pro **ANSI** physical layout (US English).
 *
 * **Status: 🧪 unverified.** This layout file was hand-derived from
 * `iso-de.json` plus public AK820 Pro / 75 % ANSI conventions. No real
 * ANSI-variant hardware was used to confirm slot ↔ key assignment.
 * The wire-level protocol is layout-agnostic (slot 0 is always Esc,
 * slot 44 is always the rightmost key in the top letter row, etc.),
 * so remapping and lighting will work on every AK820 Pro variant
 * regardless of this file. What this file gets *visually* right
 * vs wrong is what an ANSI user sees on the on-screen keyboard
 * surface in the Keymap view.
 *
 * Structural differences vs the ISO-DE reference:
 *
 * | What | ISO-DE | ANSI |
 * |---|---|---|
 * | `<>\|` key | slot 98 (left of Y/Z) | **dropped** — physically absent on ANSI |
 * | `\|` backslash | n/a | slot 97, top letter row (between `]}` and Enter) |
 * | Enter | row 2, L-shaped (`h-30 w-18`) | **row 3, single-row 2.25 u wide** |
 * | Top letter row last key | Enter (slot 76) | `\|` (slot 97) |
 * | Home row last key | `#'` (slot 97) | Enter (slot 76) |
 * | L-Shift width | 1 u (slot 98 takes the space) | wider (no slot 98) |
 * | Backspace width | 1 u + `Back` legend | 2 u, label `Back` |
 *
 * **HID codes stay the same** as ISO-DE wherever the *position* is the
 * same. The USB HID Keyboard Usage Page 0x07 codes are position-
 * relative-to-US-QWERTY, so e.g. slot 38 (sixth letter in the top
 * letter row) is always HID 28; on ANSI it reads "Y", on QWERTZ it
 * reads "Z", and the OS layout driver does the translation when the
 * keyboard emits the code.
 *
 * If you own a real ANSI AK820 Pro and the on-screen surface looks
 * wrong, please open an issue with photos — we'll happily correct it.
 */
export const ANSI_LAYOUT_ROWS: PhysicalKey[][] = json as PhysicalKey[][];

/** All slot indices used by this layout, in row-major order. */
export const ANSI_LAYOUT_SLOTS: number[] = ANSI_LAYOUT_ROWS.flat().map(
  (k) => k.slot,
);

export const ANSI_LAYOUT: KeyboardLayout = {
  id: "ansi",
  displayName: "ANSI",
  description:
    "US English / ANSI 75 %. Flat 2.25 u Enter, wide L-Shift, `\\|` in the top letter row. 🧪 unverified — no hardware confirmation.",
  rows: ANSI_LAYOUT_ROWS,
};
