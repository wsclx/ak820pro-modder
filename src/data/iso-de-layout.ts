import layout from "./iso-de-layout.json";

/**
 * A physical key on the AK820 Pro ISO-DE layout.
 *
 * `slot` is the device-internal value (0–127) that the firmware uses to
 * address this position in `GET_KEY` / `SET_KEY` payloads. `hid` is the
 * factory-default HID Usage Code. `label` is the printed legend.
 *
 * Source: extracted from Mario's official online-driver JSON export
 * (firmware 1.07), see `docs/reverse-engineering/`.
 */
export interface PhysicalKey {
  slot: number;
  label: string;
  hid: number;
  /** Layout flexbox tweaks straight from the official tool's Tailwind classes. */
  cls?: string;
}

export const ISO_DE_LAYOUT: PhysicalKey[][] = layout as PhysicalKey[][];

/** All key slots used by the layout, in row-major order. */
export const SLOTS_IN_LAYOUT: number[] = ISO_DE_LAYOUT
  .flat()
  .map((k) => k.slot);
