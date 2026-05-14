/**
 * iCloud-Drive profile sync card, lives in the System view.
 *
 * Three states it has to render:
 *   1. macOS user with iCloud Drive set up → full UI: auto-sync toggle
 *      plus manual Push / Pull buttons, status line with last-synced
 *      timestamp.
 *   2. macOS user without iCloud Drive → "not detected" message
 *      explaining how to enable it.
 *   3. Non-macOS / build failure → falls into the not-detected branch
 *      via the backend stub.
 *
 * The toggle persists to `localStorage["ak820:icloud-sync-enabled"]`
 * so the auto-sync preference survives app restarts. The actual
 * mount-time pull happens up in `App.tsx` (so it fires regardless of
 * which view the user opens first); this component only handles the
 * presentation + manual operations.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, Mono } from "./ui";
import { Toggle } from "./ui";
import { formatError } from "../errors";

export const ICLOUD_SYNC_ENABLED_KEY = "ak820:icloud-sync-enabled";

interface SyncStatus {
  icloud_available: boolean;
  icloud_path: string | null;
  remote_automations_present: boolean;
  remote_automations_mtime_ms: number | null;
}

function readEnabled(): boolean {
  try {
    return window.localStorage.getItem(ICLOUD_SYNC_ENABLED_KEY) === "true";
  } catch {
    return false;
  }
}

function writeEnabled(v: boolean) {
  try {
    window.localStorage.setItem(ICLOUD_SYNC_ENABLED_KEY, v ? "true" : "false");
  } catch {
    /* private-browsing / quota — silently ignore */
  }
}

function formatRelative(ms: number | null): string {
  if (ms === null) return "—";
  const delta = Date.now() - ms;
  if (delta < 0) return new Date(ms).toLocaleString();
  const sec = Math.floor(delta / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(ms).toLocaleDateString();
}

export function SyncCard() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [enabled, setEnabled] = useState<boolean>(readEnabled);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const s = await invoke<SyncStatus>("icloud_sync_status");
      setStatus(s);
    } catch (e) {
      // Status probe failing is the "no iCloud" path; don't spam the
      // user with an error banner — just render the not-detected state.
      setStatus({
        icloud_available: false,
        icloud_path: null,
        remote_automations_present: false,
        remote_automations_mtime_ms: null,
      });
      setErr(formatError(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function handleToggle(v: boolean) {
    setEnabled(v);
    writeEnabled(v);
    setInfo(
      v
        ? "Auto-sync on. Future saves push to iCloud; next launch pulls from it."
        : "Auto-sync off. Manual buttons still work below.",
    );
  }

  async function manualPush() {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      const mtime = await invoke<number>("icloud_sync_push");
      setInfo(`Pushed at ${new Date(mtime).toLocaleTimeString()}`);
      await refresh();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  async function manualPull() {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      const mtime = await invoke<number | null>("icloud_sync_pull");
      if (mtime === null) {
        setInfo("Already up to date — local copy is at least as recent.");
      } else {
        setInfo(
          `Pulled iCloud copy (mtime ${new Date(mtime).toLocaleTimeString()}). ` +
            "Reload the Automations tab to see the merged list.",
        );
      }
      await refresh();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title={
        <span className="inline-flex items-center gap-2">
          <span>iCloud Sync</span>
          <Badge tone="neutral">Beta</Badge>
        </span>
      }
      action={
        status?.icloud_available ? (
          <Toggle checked={enabled} onChange={handleToggle}>
            {enabled ? "On" : "Off"}
          </Toggle>
        ) : null
      }
    >
      {status === null ? (
        <p className="text-sm text-fg-3">Checking iCloud Drive…</p>
      ) : !status.icloud_available ? (
        <>
          <p className="text-sm text-fg-2">
            iCloud Drive isn't set up on this machine. Sign in to iCloud in
            System Settings → Apple Account → iCloud, enable iCloud Drive,
            and reopen this app to use sync.
          </p>
          <p className="mt-2 text-xs text-fg-3">
            We sync into <Mono>~/Library/Mobile Documents/com~apple~CloudDocs/ak820pro-modder/</Mono>
            {" "}— a plain folder in iCloud Drive, no app entitlements required.
            You can wipe it from Finder at any time.
          </p>
        </>
      ) : (
        <>
          <p className="text-sm text-fg-2">
            Round-trips your automations list through iCloud Drive so a fresh
            install on another Mac picks up the same library. Last-write-wins
            by modification time; no per-record merging yet.
          </p>
          <dl className="mt-3 grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
            <dt className="text-fg-3">Sync folder</dt>
            <dd className="break-all font-mono text-xs text-fg-1">
              {status.icloud_path}
            </dd>
            <dt className="text-fg-3">Remote automations</dt>
            <dd className="text-fg-1">
              {status.remote_automations_present ? (
                <>
                  yes ·{" "}
                  <span className="text-fg-2">
                    {formatRelative(status.remote_automations_mtime_ms)}
                  </span>
                </>
              ) : (
                <span className="text-fg-3">none yet</span>
              )}
            </dd>
          </dl>

          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button onClick={() => void manualPull()} disabled={busy}>
              Pull from iCloud
            </Button>
            <Button onClick={() => void manualPush()} disabled={busy}>
              Push to iCloud
            </Button>
            <Button variant="ghost" size="sm" onClick={() => void refresh()} disabled={busy}>
              Refresh status
            </Button>
          </div>

          {(info || err) && (
            <p
              className={[
                "mt-3 rounded-md border px-3 py-2 text-xs",
                err
                  ? "border-rose-500/40 bg-rose-500/10 text-rose-200"
                  : "border-line/60 bg-surface-raised/40 text-fg-2",
              ].join(" ")}
            >
              {err ?? info}
            </p>
          )}
        </>
      )}
    </Card>
  );
}
