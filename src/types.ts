export interface DeviceInfo {
  vid: number;
  pid: number;
  interface: number;
  usage_page: number;
  usage: number;
  manufacturer: string | null;
  product: string | null;
  serial: string | null;
  path: string;
}

export interface ProbeReport {
  connected: boolean;
  interface: number;
  product: string | null;
  firmware_version: string | null;
}

export interface LightingModeInfo {
  name: string;
  supports_direction: boolean;
  directions: string[];
}

export type Direction = "left" | "down" | "up" | "right";

export interface LightingConfig {
  mode: string;
  /** 6-char hex, no `#` */
  color: string;
  /** Optional secondary RGB for dual-colour effects. */
  secondary?: string | null;
  /** 0 = monochrome; >0 selects palette / rainbow variants per mode. */
  color_mode: number;
  /** Payload offset 12 — purpose still being mapped. */
  effect_mode_type: number;
  /** 0–5 */
  brightness: number;
  /** 0–5 */
  speed: number;
  direction: Direction;
}
