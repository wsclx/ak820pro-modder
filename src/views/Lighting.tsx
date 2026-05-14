import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Direction, LightingConfig, LightingModeInfo } from "../types";
import { Badge, Button, Card, ErrorBanner, Slider, Toggle } from "../components/ui";
import { PageHeader } from "../components/Layout";

const ALL_DIRECTIONS: Direction[] = ["left", "down", "up", "right"];
const APPLY_DEBOUNCE_MS = 80;

export function Lighting() {
  const [modes, setModes] = useState<LightingModeInfo[] | null>(null);
  const [cfg, setCfg] = useState<LightingConfig>({
    mode: "static",
    color: "7C5CFF",
    secondary: null,
    color_mode: 0,
    effect_mode_type: 0,
    brightness: 3,
    speed: 3,
    direction: "left",
  });
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [autoApply, setAutoApply] = useState(true);
  const [lastApplied, setLastApplied] = useState<string | null>(null);

  const pendingTimer = useRef<number | null>(null);
  const inflight = useRef(false);
  const queued = useRef<LightingConfig | null>(null);

  useEffect(() => {
    invoke<LightingModeInfo[]>("list_lighting_modes")
      .then(setModes)
      .catch((e) => setErr(String(e)));
  }, []);

  const currentMode = useMemo(
    () => modes?.find((m) => m.name === cfg.mode),
    [modes, cfg.mode],
  );

  async function applyNow(next: LightingConfig) {
    if (inflight.current) {
      queued.current = next;
      return;
    }
    inflight.current = true;
    setBusy(true);
    setErr(null);
    try {
      await invoke("apply_lighting", { config: next });
      setLastApplied(new Date().toLocaleTimeString());
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
      inflight.current = false;
      const drain = queued.current;
      queued.current = null;
      if (drain) void applyNow(drain);
    }
  }

  function scheduleApply(next: LightingConfig) {
    if (pendingTimer.current !== null) window.clearTimeout(pendingTimer.current);
    pendingTimer.current = window.setTimeout(() => {
      pendingTimer.current = null;
      void applyNow(next);
    }, APPLY_DEBOUNCE_MS);
  }

  function update<K extends keyof LightingConfig>(key: K, value: LightingConfig[K]) {
    const next = { ...cfg, [key]: value };
    setCfg(next);
    if (autoApply) scheduleApply(next);
  }

  function updateColor(v: string) {
    const next = { ...cfg, color: v };
    setCfg(next);
    if (autoApply && v.length === 6) scheduleApply(next);
  }

  if (modes === null) {
    return <p className="text-sm text-fg-2">Loading lighting modes…</p>;
  }

  const hasSecondary = cfg.secondary !== null && cfg.secondary !== undefined;

  return (
    <>
      <PageHeader
        title="Lighting"
        description="Per-keyboard global effects. Changes apply immediately when auto-apply is on."
        action={
          <div className="flex items-center gap-4">
            <Toggle checked={autoApply} onChange={setAutoApply}>
              Auto-apply
            </Toggle>
            <Button variant="primary" onClick={() => void applyNow(cfg)} disabled={busy}>
              {busy ? "Sending…" : "Apply"}
            </Button>
          </div>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6">
        <Card title="Mode" action={lastApplied && <span className="text-xs text-fg-3">last applied {lastApplied}</span>}>
          <div className="flex flex-wrap gap-2">
            {modes.map((m) => {
              const isActive = m.name === cfg.mode;
              return (
                <Button
                  key={m.name}
                  variant={isActive ? "ghost-active" : "ghost"}
                  size="sm"
                  onClick={() => update("mode", m.name)}
                  title={m.supports_direction ? `directions: ${m.directions.join(", ")}` : "direction ignored"}
                >
                  {m.name}
                </Button>
              );
            })}
          </div>
        </Card>

        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
          <Card title="Color">
            <div className="flex items-center gap-3">
              <input
                type="color"
                value={"#" + cfg.color}
                onChange={(e) => updateColor(e.target.value.replace("#", "").toUpperCase())}
                className="h-10 w-14"
              />
              <input
                type="text"
                value={cfg.color}
                onChange={(e) => {
                  const v = e.target.value.replace(/[^0-9a-fA-F]/g, "").toUpperCase().slice(0, 6);
                  updateColor(v);
                }}
                placeholder="FFFFFF"
                className="w-28 font-mono uppercase"
              />
              <Badge tone="neutral">primary</Badge>
            </div>

            <div className="mt-5 border-t border-line/60 pt-4">
              <Toggle
                checked={hasSecondary}
                onChange={(v) => update("secondary", v ? "000000" : null)}
              >
                Secondary color (dual-tone modes)
              </Toggle>
              {hasSecondary && (
                <div className="mt-3 flex items-center gap-3">
                  <input
                    type="color"
                    value={"#" + (cfg.secondary ?? "000000")}
                    onChange={(e) => update("secondary", e.target.value.replace("#", "").toUpperCase())}
                    className="h-10 w-14"
                  />
                  <input
                    type="text"
                    value={cfg.secondary ?? ""}
                    onChange={(e) => {
                      const v = e.target.value.replace(/[^0-9a-fA-F]/g, "").toUpperCase().slice(0, 6);
                      update("secondary", v);
                    }}
                    placeholder="000000"
                    className="w-28 font-mono uppercase"
                  />
                  <Badge tone="neutral">secondary</Badge>
                </div>
              )}
            </div>
          </Card>

          <Card
            title="Direction"
            action={
              !currentMode?.supports_direction && (
                <span className="text-xs text-fg-3">ignored for this mode</span>
              )
            }
          >
            <div className="flex flex-wrap gap-2">
              {ALL_DIRECTIONS.map((d) => {
                const supported = !currentMode?.supports_direction || currentMode.directions.includes(d);
                const isActive = d === cfg.direction;
                return (
                  <Button
                    key={d}
                    variant={isActive ? "ghost-active" : "ghost"}
                    size="sm"
                    onClick={() => update("direction", d)}
                    disabled={!supported}
                  >
                    {d}
                  </Button>
                );
              })}
            </div>

            <div className="mt-5 border-t border-line/60 pt-4">
              <p className="mb-2 text-xs uppercase tracking-wider text-fg-2">
                colorMode <span className="ml-1 normal-case text-fg-3">(0 = mono, &gt;0 cycles per mode)</span>
              </p>
              <input
                type="number"
                min={0}
                max={255}
                value={cfg.color_mode}
                onChange={(e) =>
                  update("color_mode", Math.max(0, Math.min(255, Number(e.target.value) || 0)))
                }
                className="w-24 font-mono"
              />
            </div>
          </Card>
        </div>

        <Card title="Levels">
          <div className="grid grid-cols-1 gap-6 sm:grid-cols-2">
            <Slider label="Brightness" value={cfg.brightness} max={5} onChange={(v) => update("brightness", v)} />
            <Slider label="Speed" value={cfg.speed} max={5} onChange={(v) => update("speed", v)} />
          </div>
        </Card>
      </div>
    </>
  );
}
