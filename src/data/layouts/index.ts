/**
 * Layout registry for the AK820 Pro keyboard variants.
 *
 * ## Current shipping coverage
 *
 * `v0.5.0-beta` is built and tested **only** against the **ISO-DE** (German
 * QWERTZ) layout. Selecting any other layout is intentionally not exposed
 * in the UI yet — see the README § Roadmap for the multi-layout milestone.
 *
 * ## Adding a new layout (when its hardware variant has been verified)
 *
 * 1. Drop a `<layout-id>.json` file next to `iso-de.json` with the same
 *    schema (`PhysicalKey[][]` — rows of `{ slot, label, hid, cls? }`).
 *    Slot numbers are firmware-internal and identical across variants;
 *    only labels and `cls` flexbox hints change.
 * 2. Create a `<layout-id>.ts` sibling that imports the JSON, defines the
 *    typed wrapper, and exports a `KeyboardLayout` matching `types.ts`.
 *    Mirror the structure of `iso-de.ts`.
 * 3. Register it in `LAYOUTS` below. **Never** change `DEFAULT_LAYOUT_ID`
 *    without an explicit ship-decision discussion — the default has been
 *    `iso-de` for the entire project's history.
 *
 * Any other variant-specific differences (e.g. the JIS muhenkan / henkan
 * keys, the Brazilian ABNT2 backslash position) must stay isolated to the
 * variant's own `.json` + `.ts` pair. **Do not** add layout-aware branches
 * to the Keymap view; render uniformly from whichever `KeyboardLayout` the
 * user (or default) picks.
 */

import { ISO_DE_LAYOUT } from "./iso-de";
import type { KeyboardLayout, LayoutId } from "./types";

export type { KeyboardLayout, LayoutId, PhysicalKey } from "./types";
export { ISO_DE_LAYOUT, ISO_DE_LAYOUT_ROWS, ISO_DE_LAYOUT_SLOTS } from "./iso-de";

/**
 * Every layout the app *knows about*. Only entries with a corresponding
 * `.json` file present are runnable; the type union in `types.ts` enumerates
 * planned future entries so they show up in autocomplete, but the registry
 * is the source of truth for what's actually available at runtime.
 */
export const LAYOUTS: Partial<Record<LayoutId, KeyboardLayout>> = {
  "iso-de": ISO_DE_LAYOUT,
};

/** The layout the app uses unless / until the user picks a different one. */
export const DEFAULT_LAYOUT_ID: LayoutId = "iso-de";

/** All registered layouts in insertion order — handy for the future selector UI. */
export const REGISTERED_LAYOUTS: KeyboardLayout[] = Object.values(LAYOUTS).filter(
  (l): l is KeyboardLayout => l !== undefined,
);

/**
 * Resolve a `LayoutId` to its `KeyboardLayout`, falling back to the default
 * if the id isn't registered. Returns the default so the app never crashes
 * on a stale user preference — a banner / picker should still let the user
 * notice the mismatch.
 */
export function resolveLayout(id: LayoutId | undefined | null): KeyboardLayout {
  if (id && LAYOUTS[id]) return LAYOUTS[id] as KeyboardLayout;
  return LAYOUTS[DEFAULT_LAYOUT_ID] as KeyboardLayout;
}
