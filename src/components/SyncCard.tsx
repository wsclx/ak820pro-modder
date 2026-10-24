/**
 * iCloud-Drive profile sync card, lives in the System view.
 *
 * Three states it has to render:
 *   1. macOS user with iCloud Drive set up → full UI: auto-sync toggle
 *      plus a manual "Sync now" button, status line with last-synced
 *      timestamp, and the list of open conflicts with one button per
 *      side to settle each.
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
import type { Automation, SyncReport, SyncStatus } from "../types";

export const ICLOUD_SYNC_ENABLED_KEY = "ak820:icloud-sync-enabled";

/**
 * Settles once the app-start sync in `App.tsx` is done, never rejects.
 * `persist()` in the Automations view waits for it: that sync may write
 * the local library between the view's `list_automations` and its
 * `save_automations`, and the backend would then record the entries it
 * just merged in as deleted.
 */
export let startupSync: Promise<void> = Promise.resolve();

export function trackStartupSync(p: Promise<unknown>) {
  startupSync = p.then(
    () => undefined,
    () => undefined,
  );
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

/** One side of a conflict; `null` means the entry was deleted there. */
function describeCopy(a: Automation | null): string {
  if (a === null) return "deleted";
  // Entries from before the merge carry updated_at = 0.
  return `“${a.name}”, edited ${formatRelative(a.updated_at || null)}`;
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
        conflicts: [],
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
        ? "Auto-sync on. Every save and every app start merge with iCloud."
        : "Auto-sync off. Manual buttons still work below.",
    );
  }

  async function syncNow(resolve?: { id: number; keep: "local" | "remote" }) {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      const r = await invoke<SyncReport>("icloud_sync_pull", { resolve: resolve ?? null });
      const what = [
        r.local_changed && "took changes from iCloud",
        r.remote_changed && "sent changes to iCloud",
      ]
        .filter(Boolean)
        .join(" and ");
      setInfo(what ? `Synced: ${what}.` : "Already in sync.");
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
            Merges your automations library with iCloud Drive entry by entry,
            so changes and deletions made on different Macs add up. If the
            same automation changed on both sides, it shows up below and you
            pick the copy to keep.
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

          {status.conflicts.length > 0 && (
            <div className="mt-4 space-y-2">
              <p className="text-sm text-warn">
                {status.conflicts.length === 1
                  ? "1 automation changed"
                  : `${status.conflicts.length} automations changed`}{" "}
                on this Mac and in iCloud since the last sync. Pick the copy
                to keep; it replaces the other one on every Mac.
              </p>
              {status.conflicts.map((c) => (
                <div
                  key={c.id}
                  className="rounded-md border border-warn/40 bg-warn-soft px-3 py-2 text-sm"
                >
                  <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1">
                    <dt className="text-fg-3">This Mac</dt>
                    <dd className="text-fg-1">{describeCopy(c.local)}</dd>
                    <dt className="text-fg-3">iCloud</dt>
                    <dd className="text-fg-1">{describeCopy(c.remote)}</dd>
                  </dl>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      onClick={() => void syncNow({ id: c.id, keep: "local" })}
                      disabled={busy}
                    >
                      Keep this Mac's
                    </Button>
                    <Button
                      size="sm"
                      onClick={() => void syncNow({ id: c.id, keep: "remote" })}
                      disabled={busy}
                    >
                      Use iCloud's
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}

          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button onClick={() => void syncNow()} disabled={busy}>
              Sync now
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
