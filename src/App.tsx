import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AppSettings } from "./views/AppSettings";
import { Automations } from "./views/Automations";
import { Connect } from "./views/Connect";
import { Keymap } from "./views/Keymap";
import { Lighting } from "./views/Lighting";
import { Macros } from "./views/Macros";
import { Presets } from "./views/Presets";
import { System } from "./views/System";
import { Tft } from "./views/Tft";
import { CapabilityContext, DeviceCapabilities, UnavailableFeature } from "./capabilities";
import { HostKeymap, HostMacros } from "./views/HostKeyboard";
import { LegacySystem } from "./views/LegacySystem";
import { ICLOUD_SYNC_ENABLED_KEY, trackStartupSync } from "./components/SyncCard";
import {
  Layout,
  Plug,
  Bulb,
  Settings,
  Keyboard,
  Macro,
  Screen,
  Automation,
  Preset,
  type NavItem,
} from "./components/Layout";

type Tab =
  | "app-settings"
  | "connect"
  | "lighting"
  | "system"
  | "keymap"
  | "macros"
  | "automations"
  | "presets"
  | "tft";

const ICON_PROPS = { size: 16, strokeWidth: 1.6 } as const;

const NAV: NavItem<Tab>[] = [
  { id: "connect", label: "Connectivity", icon: <Plug {...ICON_PROPS} /> },
  { id: "lighting", label: "Lighting", icon: <Bulb {...ICON_PROPS} /> },
  { id: "system", label: "System", icon: <Settings {...ICON_PROPS} /> },
  { id: "keymap", label: "Keymap & Knob", icon: <Keyboard {...ICON_PROPS} /> },
  { id: "macros", label: "Macros", icon: <Macro {...ICON_PROPS} /> },
  { id: "automations", label: "Automations", icon: <Automation {...ICON_PROPS} /> },
  { id: "presets", label: "Presets", icon: <Preset {...ICON_PROPS} /> },
  { id: "tft", label: "TFT Display", icon: <Screen {...ICON_PROPS} /> },
  { id: "app-settings", label: "App settings", icon: <Settings {...ICON_PROPS} /> },
];

interface ProbeReport {
  connected: boolean;
  interface: number;
  product: string | null;
}

export default function App() {
  const [tab, setTab] = useState<Tab>("connect");
  const [capabilities, setCapabilities] = useState<DeviceCapabilities | null>(null);
  const [probe, setProbe] = useState<ProbeReport | null>(null);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    const openRequestedSettings = async () => {
      if (!alive) return;
      try {
        if (await invoke<boolean>("take_app_settings_request") && alive) setTab("app-settings");
      } catch (e) {
        console.warn("Could not open app settings", e);
      }
    };
    // Subscribe before draining the pending request: a tray click during page
    // startup must still navigate once React is ready.
    void listen("open-app-settings", () => { void openRequestedSettings(); }).then(stop => {
      if (!alive) { stop(); return; }
      unlisten = stop;
      void openRequestedSettings();
    }).catch(e => console.warn("Could not listen for app settings requests", e));
    return () => { alive = false; unlisten?.(); };
  }, []);

  // iCloud auto-sync on app start. The backend merges the iCloud copy of
  // the automations library into the local file (per entry; conflicts
  // wait in System → iCloud Sync). Not awaited here; saves in the
  // Automations view wait for it via `startupSync`.
  useEffect(() => {
    let enabled = false;
    try {
      enabled = window.localStorage.getItem(ICLOUD_SYNC_ENABLED_KEY) === "true";
    } catch {
      /* private-browsing — ignore */
    }
    if (!enabled) return;
    // A failure on startup is swallowed by `trackStartupSync` — the user
    // sees the error inline if they open the System → Sync card.
    trackStartupSync(invoke("icloud_sync_pull"));
  }, []);

  useEffect(() => {
    let alive = true;

    // Defer first probe by a frame so React finishes mounting cleanly before
    // the first IPC call. Without this we have seen WKWebView freezes that
    // we can't reproduce in Chromium — likely a startup-time ordering issue.
    const start = window.setTimeout(async function tick() {
      if (!alive) return;
      try {
        const p = await invoke<ProbeReport>("probe_device");
        const caps = p.connected ? await invoke<DeviceCapabilities>("get_capabilities") : null;
        if (alive) { setProbe(p); setCapabilities(caps); }
      } catch {
        if (alive) setProbe(null);
      }
      if (alive) window.setTimeout(tick, 4000);
    }, 200);

    return () => {
      alive = false;
      window.clearTimeout(start);
    };
  }, []);

  const reconnect = async () => {
    try { await invoke("force_reconnect"); } catch { /* ignored */ }
  };

  return (
    <CapabilityContext.Provider value={capabilities}><Layout
      nav={NAV}
      active={tab}
      onSelect={setTab}
      connection={probe ? { connected: probe.connected, product: probe.product } : undefined}
      onReconnect={reconnect}
      wide={tab === "keymap"}
    >
      {tab === "connect" && <Connect onReconnect={reconnect} />}
      {tab === "app-settings" && <AppSettings />}
      {tab === "lighting" && <Lighting />}
      {tab === "system" && (capabilities?.protocol === "legacy_feature" ? <LegacySystem /> : <System />)}
      {tab === "keymap" && (capabilities && !capabilities.keymap ? capabilities.host_keyboard ? <HostKeymap /> : <UnavailableFeature name="Onboard key remapping" /> : <Keymap />)}
      {tab === "macros" && (capabilities && !capabilities.macros ? capabilities.host_keyboard ? <HostMacros /> : <UnavailableFeature name="Onboard macros" /> : <Macros />)}
      {tab === "automations" && <Automations />}
      {tab === "presets" && <Presets />}
      {tab === "tft" && <Tft />}
    </Layout></CapabilityContext.Provider>
  );
}
