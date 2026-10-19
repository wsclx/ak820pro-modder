import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceInfo } from "../types";
import { Badge, Button, Card, ErrorBanner, KVList, Mono, hex4 } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { NowPlayingCard } from "../components/NowPlayingCard";
import { SyncCard } from "../components/SyncCard";
import { formatError } from "../errors";

interface Status { device: DeviceInfo; battery: { percent: number | null; wired_full: boolean } }

export function LegacySystem() {
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function refresh() {
    setBusy(true); setError(null);
    try { setStatus(await invoke<Status>("get_legacy_status")); }
    catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }
  useEffect(() => { void refresh(); }, []);

  async function apply(command: string, args: Record<string, unknown>, message: string) {
    setBusy(true); setError(null); setNotice(null);
    try { await invoke(command, args); setNotice(message); }
    catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }

  return <>
    <PageHeader title="System" description="Keyboard status, clock and sleep timer." action={<Button onClick={refresh} disabled={busy}>Refresh</Button>} />
    <ErrorBanner>{error}</ErrorBanner>
    <div className="grid gap-6">
      {notice && <Card title="Status"><p className="text-sm text-fg-2">{notice}</p></Card>}
      <Card title="Device">
        {!status ? <p className="text-sm text-fg-2">{busy ? "Reading…" : "Status unavailable. Try Refresh."}</p> : <KVList rows={[
          { label: "Product", value: status.device.product ?? "AK820" },
          { label: "VID:PID", value: <Mono>{hex4(status.device.vid)}:{hex4(status.device.pid)}</Mono> },
          { label: "Connection", value: <Badge tone="good">USB</Badge> },
          { label: "Battery", value: status.battery.wired_full ? "Wired / full" : status.battery.percent === null ? "Not reported" : `${status.battery.percent}%` },
          { label: "Firmware version", value: "Not reported by this firmware" },
        ]} />}
      </Card>
      <Card title="Clock">
        <p className="mb-4 text-sm text-fg-2">Set the keyboard clock to this Mac’s local date and time. Select the clock face on the keyboard to see it.</p>
        <Button disabled={busy} onClick={() => apply("sync_legacy_clock", {}, "Clock synchronized to the Mac’s local date and time.")}>Sync clock</Button>
      </Card>
      <Card title="Sleep timer">
        <p className="mb-4 text-sm text-fg-2">Choose how long the keyboard lighting stays awake while idle.</p>
        <div className="flex flex-wrap gap-2">{[0,1,5,30].map((minutes) => <Button key={minutes} disabled={busy}
          onClick={() => apply("set_legacy_sleep", { minutes }, `Sleep timer sent: ${minutes === 0 ? "never" : `${minutes} minute(s)`}. This firmware does not report the current timer back.`)}>
          {minutes === 0 ? "Never" : `${minutes} min`}
        </Button>)}</div>
        <p className="mt-4 text-xs text-fg-3">The current value cannot be read from this revision. A successful send is not a read-back verification.</p>
      </Card>
      <NowPlayingCard /><SyncCard />
    </div>
  </>;
}
