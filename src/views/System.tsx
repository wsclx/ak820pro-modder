import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, BatteryBar, Button, Card, ErrorBanner, KVList, Mono, formatInt, hex4 } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { NowPlayingCard } from "../components/NowPlayingCard";
import { formatError } from "../errors";

interface DeviceInfoReport {
  rom_size: number;
  macro_space_size: number;
  vid: number;
  pid: number;
  firmware_version: number;
  sensor: number;
  manufacturer_id: number;
  product_id: number;
  work_mode: number;
  battery_level: number;
  charge_status: number;
  current_profile: number;
  axis_info: number;
  tft_max_frames: number;
  gif_max_frames: number;
  led_max_frames: number;
  tft_direction: number;
  rt_precision: number;
  frame_version: number;
  lighting_version: number;
}

interface GameMode {
  game_mode: number;
  fn_switch: number;
  sleep_time: number;
  key_delay: number;
  report_rate: number;
  system_mode: number;
  tft_display_time: number;
  top_dead_zone: number;
  bottom_dead_zone: number;
  stability_mode: number;
  auto_calibration: number;
  single_key_wakeup: number;
}

interface SleepPreset {
  value: number;
  label: string;
}

export function System() {
  const [info, setInfo] = useState<DeviceInfoReport | null>(null);
  const [gm, setGm] = useState<GameMode | null>(null);
  const [presets, setPresets] = useState<SleepPreset[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refresh() {
    setBusy(true);
    setErr(null);
    try {
      // Sequential, not Promise.all — get_device_info and get_game_mode both
      // hold the persistent HID mutex (sync `std::sync::Mutex`); running them
      // in parallel blocks a tokio worker and can freeze the Tauri runtime.
      const p = await invoke<SleepPreset[]>("list_sleep_presets");
      setPresets(p);
      const i = await invoke<DeviceInfoReport>("get_device_info");
      setInfo(i);
      const g = await invoke<GameMode>("get_game_mode");
      setGm(g);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function setSleep(value: number) {
    if (!gm) return;
    setBusy(true);
    setErr(null);
    try {
      const next = { ...gm, sleep_time: value };
      await invoke("set_game_mode", { mode: next });
      const readback = await invoke<GameMode>("get_game_mode");
      setGm(readback);
      if (readback.sleep_time !== value) {
        setErr(`Set sleep_time=${value} but keyboard reports ${readback.sleep_time}`);
      }
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title="System"
        description="Live firmware, battery, and onboard settings."
        action={
          <Button variant="primary" onClick={refresh} disabled={busy}>
            {busy ? "Reading…" : "Refresh"}
          </Button>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6">
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
          <Card title="Device">
            {info === null ? (
              <p className="text-sm text-fg-2">Reading…</p>
            ) : (
              <KVList
                rows={[
                  {
                    label: "Firmware",
                    value: <Mono>v{info.firmware_version.toFixed(2)}</Mono>,
                  },
                  {
                    label: "VID:PID",
                    value: <Mono>{hex4(info.vid)}:{hex4(info.pid)}</Mono>,
                  },
                  {
                    label: "Battery",
                    value: <BatteryBar level={info.battery_level} charging={info.charge_status === 1} />,
                  },
                  {
                    label: "Profile",
                    value: <Badge tone="accent">slot {info.current_profile}</Badge>,
                  },
                  {
                    label: "Macro space",
                    value: <Mono>{formatInt(info.macro_space_size)} bytes</Mono>,
                  },
                  {
                    label: "TFT capacity",
                    value: <Mono>{info.tft_max_frames} frames</Mono>,
                  },
                  {
                    label: "Frame version",
                    value: <Mono>{info.frame_version}</Mono>,
                  },
                ]}
              />
            )}
          </Card>

          <Card title="Sleep timer">
            {gm === null ? (
              <p className="text-sm text-fg-2">Reading…</p>
            ) : (
              <>
                <p className="mb-4 text-sm text-fg-2">
                  Current:{" "}
                  <span className="font-mono text-fg-0">
                    {presets.find((p) => p.value === gm.sleep_time)?.label ?? `value ${gm.sleep_time}`}
                  </span>
                </p>
                <div className="flex flex-wrap gap-2">
                  {presets.map((p) => (
                    <Button
                      key={p.value}
                      variant={p.value === gm.sleep_time ? "ghost-active" : "ghost"}
                      size="sm"
                      onClick={() => setSleep(p.value)}
                      disabled={busy}
                    >
                      {p.label}
                    </Button>
                  ))}
                </div>
                <p className="mt-4 border-t border-line/60 pt-3 text-xs text-fg-3">
                  After a write the keyboard's value is read back to confirm — UI updates only after the round-trip succeeds.
                </p>
              </>
            )}
          </Card>
        </div>

        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
          <Card title="Other settings">
            {gm === null ? (
              <p className="text-sm text-fg-2">Reading…</p>
            ) : (
              <KVList
                rows={[
                  { label: "Key delay", value: <Mono>{gm.key_delay}</Mono> },
                  { label: "Report rate", value: <Mono>{gm.report_rate}</Mono> },
                  { label: "TFT display time", value: <Mono>{gm.tft_display_time}</Mono> },
                  { label: "Stability mode", value: <Mono>{gm.stability_mode}</Mono> },
                  { label: "Auto calibration", value: <Mono>{gm.auto_calibration}</Mono> },
                  { label: "Single-key wakeup", value: <Mono>{gm.single_key_wakeup}</Mono> },
                ]}
              />
            )}
          </Card>

          <NowPlayingCard />
        </div>
      </div>
    </>
  );
}
