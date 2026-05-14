import json from "./iso-de.json";
import type { KeyboardLayout, PhysicalKey } from "./types";

/**
 * AK820 Pro **ISO-DE** physical layout.
 *
 * This is the **only** layout `v0.5.0-beta` ships against. The 6×N row grid
 * maps each printed key on the German ISO variant to its firmware slot
 * (0..127 in `GET_KEY` / `SET_KEY` payloads), HID Keyboard Usage Code, and
 * any Tailwind class hints from the official AJAZZ web driver.
 *
 * Source: profile export from a real AK820 Pro on firmware 1.07. See
 * `docs/reverse-engineering/online-driver/official-export-firmware-1.07.json`.
 *
 * **Foot-gun**: do NOT use this layout for ANSI / ISO-FR / ISO-ES /
 * ISO-UK / JIS hardware. Slot numbers overlap (firmware-internal addresses
 * are constant) but printed legends and physical key positions diverge.
 * Rendering the surface with the wrong layout will mislabel keys and
 * confuse users. Multi-layout support is on the roadmap — see README §
 * Roadmap. Until then, the app declares itself ISO-DE-only in the
 * sidebar footer.
 */
export const ISO_DE_LAYOUT_ROWS: PhysicalKey[][] = json as PhysicalKey[][];

/** All slot indices used by this layout, in row-major order. */
export const ISO_DE_LAYOUT_SLOTS: number[] = ISO_DE_LAYOUT_ROWS.flat().map(
  (k) => k.slot,
);

export const ISO_DE_LAYOUT: KeyboardLayout = {
  id: "iso-de",
  displayName: "ISO-DE",
  description: "German ISO (QWERTZ) — the only layout v0.5.0-beta is built and tested against.",
  rows: ISO_DE_LAYOUT_ROWS,
};
