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

/* ---- Per-key RGB (cmd 36 SET_CUSTOM_LED_DATA, mode 0x80) ---- */

/** Mirrors `LedColor` from `ak820_protocol::commands::per_key_rgb`. */
export interface LedColor {
  led_id: number;
  red: number;
  green: number;
  blue: number;
}

/** 128-LED snapshot. Index in `leds` == slot id (and `led_id` mirrors it). */
export interface CustomLedMap {
  leds: LedColor[];
}

/* ---- macOS Now-Playing (Phase 6 preview path) ---- */

/** Mirrors `NowPlaying` from `src-tauri/src/now_playing.rs`. */
export interface NowPlaying {
  /** "Music", "Spotify", or "none" when nothing is playing. */
  source: string;
  is_playing: boolean;
  title: string | null;
  artist: string | null;
  album: string | null;
}
