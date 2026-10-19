import { createContext, useContext } from "react";

export interface DeviceCapabilities {
  protocol: "web_output" | "legacy_feature";
  host_keyboard: boolean;
  lighting: boolean;
  clock_sync: boolean;
  sleep_timer: boolean;
  device_info: boolean;
  game_mode: boolean;
  keymap: boolean;
  fn_keymap: boolean;
  macros: boolean;
  per_key_rgb: boolean;
  tft_builtin: boolean;
  now_playing_tft: boolean;
  audio_reactive: boolean;
}

export const CapabilityContext = createContext<DeviceCapabilities | null>(null);
export const useCapabilities = () => useContext(CapabilityContext);

export function UnavailableFeature({ name }: { name: string }) {
  return <div className="rounded-lg border border-line bg-surface p-6">
    <h1 className="mb-3 text-xl font-medium">{name}</h1>
    <p className="text-sm text-fg-2">This keyboard uses a different firmware protocol. {name} is not supported for this revision yet. No keyboard settings have been changed.</p>
    <p className="mt-3 text-sm text-fg-2">Lighting, clock, sleep timer and image/GIF upload are available. Automations can still be run from their library.</p>
  </div>;
}
