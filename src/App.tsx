import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Automations } from "./views/Automations";
import { Connect } from "./views/Connect";
import { Keymap } from "./views/Keymap";
import { Lighting } from "./views/Lighting";
import { Macros } from "./views/Macros";
import { Presets } from "./views/Presets";
import { System } from "./views/System";
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
  { id: "tft", label: "TFT Display", icon: <Screen {...ICON_PROPS} />, comingSoon: true },
];

interface ProbeReport {
  connected: boolean;
  interface: number;
  product: string | null;
}

export default function App() {
  const [tab, setTab] = useState<Tab>("connect");
  const [probe, setProbe] = useState<ProbeReport | null>(null);

  useEffect(() => {
    let alive = true;

    // Defer first probe by a frame so React finishes mounting cleanly before
    // the first IPC call. Without this we have seen WKWebView freezes that
    // we can't reproduce in Chromium — likely a startup-time ordering issue.
    const start = window.setTimeout(async function tick() {
      if (!alive) return;
      try {
        const p = await invoke<ProbeReport>("probe_device");
        if (alive) setProbe(p);
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
    <Layout
      nav={NAV}
      active={tab}
      onSelect={setTab}
      connection={probe ? { connected: probe.connected, product: probe.product } : undefined}
      onReconnect={reconnect}
      wide={tab === "keymap"}
    >
      {tab === "connect" && <Connect onReconnect={reconnect} />}
      {tab === "lighting" && <Lighting />}
      {tab === "system" && <System />}
      {tab === "keymap" && <Keymap />}
      {tab === "macros" && <Macros />}
      {tab === "automations" && <Automations />}
      {tab === "presets" && <Presets />}
    </Layout>
  );
}
