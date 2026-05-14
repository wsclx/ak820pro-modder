/**
 * Type definitions for AK820 Pro physical keyboard layouts.
 *
 * The keymap WIRE PROTOCOL is layout-agnostic — every variant of the
 * AK820 Pro uses the same 128-slot `GET_KEY` / `SET_KEY` byte layout
 * (see `crates/ak820-protocol/src/commands/keymap.rs`). The slot
 * numbers are firmware-internal and stay constant.
 *
 * What changes between layouts is what's *printed* on each physical
 * keycap and where each cap sits visually. A layout file (one per
 * regional variant) captures those differences so the on-screen
 * keyboard surface and the action picker can render the right legends.
 */

/** A single physical key on the AK820 Pro. */
export interface PhysicalKey {
  /** Device-internal slot index (0..127) used by `GET_KEY` / `SET_KEY`. */
  slot: number;
  /** Printed legend on the keycap. */
  label: string;
  /** Factory-default HID Keyboard Usage Code (Usage Page 0x07). */
  hid: number;
  /** Tailwind/CSS class hints carried straight from the official AJAZZ web driver. */
  cls?: string;
}

/** Identifier for a known regional layout. */
export type LayoutId = "iso-de" | "ansi" | "iso-fr" | "iso-es" | "iso-uk" | "jis";

/** A complete keyboard-layout descriptor. */
export interface KeyboardLayout {
  /** Stable id used in routing, registry lookups, and user preferences. */
  id: LayoutId;
  /** Human-readable label (e.g. "ISO-DE", "ANSI"). */
  displayName: string;
  /** Short note shown next to the layout picker. */
  description: string;
  /** Row-major matrix of physical keys. */
  rows: PhysicalKey[][];
}
