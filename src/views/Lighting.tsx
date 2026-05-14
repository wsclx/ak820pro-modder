import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Direction, LightingConfig, LightingModeInfo } from "../types";
import { Badge, Button, Card, ErrorBanner, Slider, Toggle } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { CustomLightingPaint } from "./CustomLightingPaint";
import { formatError } from "../errors";

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

  // Audio-reactive lighting (macOS only). The backend keeps the
  // authoritative state — we poll every 3s so a loop self-exit (capture
  // crashed, permission revoked) clears the toggle without the user
  // having to click anything.
  const [audioReactive, setAudioReactive] = useState(false);
  const [audioBusy, setAudioBusy] = useState(false);

  const pendingTimer = useRef<number | null>(null);
  const inflight = useRef(false);
  const queued = useRef<LightingConfig | null>(null);

  useEffect(() => {
    invoke<LightingModeInfo[]>("list_lighting_modes")
      .then(setModes)
      .catch((e) => setErr(formatError(e)));
  }, []);

  // Initial status + drift-detection poll. 3s is a compromise between
  // catching crashes promptly and not hammering the IPC channel.
  useEffect(() => {
    let alive = true;
    const refresh = () => {
      invoke<boolean>("audio_reactive_status")
        .then((on) => {
          if (alive) setAudioReactive(on);
        })
        .catch(() => {
          // The command stubs to error on non-macOS — silently treat
          // that as "not running".
          if (alive) setAudioReactive(false);
        });
    };
    refresh();
    const id = window.setInterval(refresh, 3000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  async function toggleAudioReactive(on: boolean) {
    setAudioBusy(true);
    setErr(null);
    try {
      if (on) {
        await invoke("audio_reactive_start");
        setAudioReactive(true);
      } else {
        await invoke("audio_reactive_stop");
        setAudioReactive(false);
      }
    } catch (e) {
      setErr(formatError(e));
      // Backend authority: if start failed, ensure UI shows off.
      setAudioReactive(false);
    } finally {
      setAudioBusy(false);
    }
  }

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
      setErr(formatError(e));
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
  const isCustomMode = cfg.mode === "custom";

  return (
    <>
      <PageHeader
        title="Lighting"
        description={
          isCustomMode
            ? "Per-key custom RGB — click any key on the layout below to paint it."
            : "Per-keyboard global effects. Changes apply immediately when auto-apply is on."
        }
        action={
          isCustomMode ? null : (
            <div className="flex items-center gap-4">
              <Toggle checked={autoApply} onChange={setAutoApply}>
                Auto-apply
              </Toggle>
              <Button variant="primary" onClick={() => void applyNow(cfg)} disabled={busy}>
                {busy ? "Sending…" : "Apply"}
              </Button>
            </div>
          )
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6">
        <Card
          title="Audio-reactive"
          action={
            <Toggle checked={audioReactive} onChange={toggleAudioReactive} disabled={audioBusy}>
              {audioReactive ? "Streaming" : "Off"}
            </Toggle>
          }
        >
          <p className="text-sm text-fg-2">
            Taps the macOS system-audio mix, runs an FFT, and paints the keyboard with
            bass / mids / highs as red / green / blue across vertical zones. While
            on, the firmware sits in <code>custom</code> mode and the controls
            below are paused — turn this off to set static modes or per-key colours.
          </p>
          <p className="mt-2 text-xs text-fg-3">
            First run pops the macOS Screen Recording permission prompt — that's
            normal, ScreenCaptureKit shares the same TCC bucket even for
            audio-only capture. Once granted, the toggle works silently.
          </p>
        </Card>

        <div
          className={["grid gap-6", audioReactive ? "pointer-events-none opacity-50" : ""].join(" ")}
        >
        <Card
          title="Mode"
          action={
            lastApplied && !audioReactive ? (
              <span className="text-xs text-fg-3">last applied {lastApplied}</span>
            ) : audioReactive ? (
              <span className="text-xs text-fg-3">paused while audio-reactive is on</span>
            ) : null
          }
        >
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

        {isCustomMode ? (
          <CustomLightingPaint inheritedConfig={cfg} />
        ) : (
        <>
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
        </>
        )}
        </div>
      </div>
    </>
  );
}
