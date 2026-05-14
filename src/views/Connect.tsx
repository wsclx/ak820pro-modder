import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceInfo, ProbeReport } from "../types";
import { Badge, Button, Card, ErrorBanner, KVList, Mono, hex4, prettyProduct } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { formatError } from "../errors";

const CONTROL_USAGE_PAGE = 0xff68;

export function Connect({ onReconnect }: { onReconnect?: () => void }) {
  const [devices, setDevices] = useState<DeviceInfo[] | null>(null);
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refresh() {
    setBusy(true);
    setErr(null);
    try {
      const list = await invoke<DeviceInfo[]>("list_devices");
      setDevices(list);
      if (list.some((d) => d.usage_page === CONTROL_USAGE_PAGE)) {
        setProbe(await invoke<ProbeReport>("probe_device"));
      } else {
        setProbe(null);
      }
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  async function reconnect() {
    onReconnect?.();
    await refresh();
  }

  useEffect(() => {
    refresh();
  }, []);

  return (
    <>
      <PageHeader
        title="Connectivity"
        description="HID enumeration and control-interface health."
        action={
          <div className="flex gap-2">
            <Button onClick={reconnect} disabled={busy} title="Drop cached HID handle and re-open the device">
              Reconnect
            </Button>
            <Button variant="primary" onClick={refresh} disabled={busy}>
              {busy ? "Probing…" : "Re-probe"}
            </Button>
          </div>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6">
        <Card title="Control interface">
          {probe === null ? (
            <p className="text-sm text-fg-2">Waiting on a vendor interface…</p>
          ) : (
            <KVList
              rows={[
                {
                  label: "Status",
                  value: probe.connected ? <Badge tone="good">connected</Badge> : <Badge tone="bad">offline</Badge>,
                },
                { label: "Product", value: prettyProduct(probe.product) },
                { label: "HID interface", value: <Mono>{probe.interface}</Mono>, mono: true },
                {
                  label: "Firmware",
                  value: probe.firmware_version ? <Mono>{probe.firmware_version}</Mono> : <span className="text-fg-2">(read from System tab)</span>,
                },
              ]}
            />
          )}
        </Card>

        <Card title={`Detected HID endpoints${devices ? ` · ${devices.length}` : ""}`}>
          {devices === null ? (
            <p className="text-sm text-fg-2">Scanning…</p>
          ) : devices.length === 0 ? (
            <p className="text-sm text-fg-2">No AK820 candidates found.</p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-xs uppercase tracking-wider text-fg-2">
                    <th className="px-2 py-2 font-normal">iface</th>
                    <th className="px-2 py-2 font-normal">usage page</th>
                    <th className="px-2 py-2 font-normal">vid:pid</th>
                    <th className="px-2 py-2 font-normal">product</th>
                    <th className="px-2 py-2 font-normal">role</th>
                  </tr>
                </thead>
                <tbody>
                  {devices.map((d) => {
                    const isControl = d.usage_page === CONTROL_USAGE_PAGE;
                    return (
                      <tr
                        key={`${d.path}:${d.interface}:${d.usage_page}`}
                        className="border-t border-line/60"
                      >
                        <td className="px-2 py-2 font-mono">{d.interface}</td>
                        <td className="px-2 py-2 font-mono">{hex4(d.usage_page)}</td>
                        <td className="px-2 py-2 font-mono">
                          {hex4(d.vid)}:{hex4(d.pid)}
                        </td>
                        <td className="px-2 py-2">{d.product ?? "—"}</td>
                        <td className="px-2 py-2">
                          {isControl ? (
                            <Badge tone="accent">control</Badge>
                          ) : (
                            <span className="text-xs text-fg-3">standard HID</span>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      </div>
    </>
  );
}
