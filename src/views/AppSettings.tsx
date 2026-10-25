import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { formatError } from "../errors";

interface Status {
  start_at_login: boolean | null;
  login_error: string | null;
  keep_running_on_close: boolean;
  menu_bar_ready: boolean;
}

export function AppSettings() {
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);

  async function refresh() {
    setBusy(true);
    setError(null);
    try {
      setStatus(await invoke<Status>("app_lifecycle_status"));
    } catch (e) {
      setError(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => { void refresh(); }, []);

  async function change(kind: "login" | "close", enabled: boolean) {
    setBusy(true);
    setError(null);
    setReceipt(null);
    try {
      await invoke(kind === "login" ? "set_start_at_login" : "set_keep_running_on_close", { enabled });
      setStatus(await invoke<Status>("app_lifecycle_status"));
      setReceipt(kind === "login"
        ? (enabled ? "Start at login enabled. The app will launch quietly in the menu bar." : "Start at login disabled.")
        : (enabled ? "Closing the window will keep the app running in the menu bar." : "Closing the window will quit the app."));
    } catch (e) {
      setError(formatError(e));
      // A registration may have succeeded even if the read-back failed.
      try { setStatus(await invoke<Status>("app_lifecycle_status")); } catch { /* retain the error */ }
    } finally {
      setBusy(false);
    }
  }

  return <div className="space-y-6">
    <PageHeader title="App settings" description="Choose how AK820 Pro Modder starts and runs on this computer." />
    <ErrorBanner>{error}</ErrorBanner>
    {receipt && <p role="status" className="text-sm text-good">{receipt}</p>}
    <Card title="Startup and menu bar">
      <div className="space-y-6 text-sm text-fg-2">
        <label className="flex items-start gap-3">
          <input type="checkbox" aria-label="Start at login" className="mt-1 accent-accent-500"
            checked={status?.start_at_login ?? false} disabled={busy || !status || status.start_at_login === null}
            onChange={e => void change("login", e.target.checked)} />
          <span><span className="block font-medium text-fg-0">Start at login</span>
            <span className="mt-1 block">Launch quietly in the menu bar when you log in. Existing automation and Mac macro shortcuts remain available while the app is running.</span>
          </span>
        </label>
        {status?.login_error && <ErrorBanner>{`Could not read login registration: ${status.login_error}`}</ErrorBanner>}
        <label className="flex items-start gap-3">
          <input type="checkbox" aria-label="Keep running when the window closes" className="mt-1 accent-accent-500"
            checked={status?.keep_running_on_close ?? false} disabled={busy || !status || !status.menu_bar_ready}
            onChange={e => void change("close", e.target.checked)} />
          <span><span className="block font-medium text-fg-0">Keep running when the window closes</span>
            <span className="mt-1 block">Closing hides the window and keeps your drafts and background activity. Use AK820 in the menu bar to show it again or quit. When disabled, closing the window quits the app.</span>
          </span>
        </label>
        {status && !status.menu_bar_ready && <p className="text-bad">The menu bar icon could not start. The window will stay accessible; restart the app to try again.</p>}
        <div className="flex flex-wrap gap-2">
          <Button disabled={busy || !status?.menu_bar_ready}
            onClick={() => void invoke("hide_to_menu_bar").catch(e => setError(formatError(e)))}>Hide to menu bar</Button>
          <Button disabled={busy} onClick={() => void refresh()}>Refresh settings</Button>
          <Button variant="ghost" onClick={() => void invoke("quit_keyboard_app").catch(e => setError(formatError(e)))}>Quit app</Button>
        </div>
        <p className="text-xs text-fg-3">Both options are off initially. Quit always exits, even when background running is enabled. On macOS, reopening from the Dock or Applications shows the window. If login launch is blocked, check System Settings → General → Login Items & Extensions.</p>
      </div>
    </Card>
  </div>;
}
