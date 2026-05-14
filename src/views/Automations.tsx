/**
 * Automations library.
 *
 * v0.6 scope: manual execution only — user defines AppleScript / macOS
 * Shortcut / shell-command entries here, clicks Run to execute. v0.7 will
 * add a global-hotkey listener (Carbon RegisterEventHotKey) so a marker
 * key sent from the AK820 Pro fires the same execution path; the schema
 * already carries a `marker_hid` placeholder for that.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner, Mono } from "../components/ui";
import { PageHeader } from "../components/Layout";
import type {
  Automation,
  AutomationKind,
  AutomationRunResult,
} from "../types";

const KIND_LABEL: Record<AutomationKind, string> = {
  apple_script: "AppleScript",
  shortcut: "macOS Shortcut",
  shell: "Shell command",
};

const KIND_HINT: Record<AutomationKind, string> = {
  apple_script: "Runs via `osascript -e <body>`. Multi-line scripts welcome.",
  shortcut:
    "Runs via `shortcuts run \"<name>\"`. Pick from the list of installed Shortcuts below.",
  shell: "Runs via `sh -c <body>`. ⚠ No sandboxing — anything in the body executes on your machine.",
};

export function Automations() {
  const [items, setItems] = useState<Automation[] | null>(null);
  const [shortcuts, setShortcuts] = useState<string[] | null>(null);
  const [editing, setEditing] = useState<Automation | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [lastRun, setLastRun] = useState<Record<number, AutomationRunResult>>({});

  /* ----- IO ---------------------------------------------------------- */

  const refresh = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      const list = await invoke<Automation[]>("list_automations");
      setItems(list);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const refreshShortcuts = useCallback(async () => {
    try {
      const list = await invoke<string[]>("list_shortcuts");
      setShortcuts(list);
    } catch {
      setShortcuts([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
    void refreshShortcuts();
  }, [refresh, refreshShortcuts]);

  async function persist(next: Automation[]) {
    setBusy(true);
    setErr(null);
    try {
      await invoke("save_automations", { list: next });
      setItems(next);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function runOne(id: number) {
    setBusy(true);
    setErr(null);
    try {
      const r = await invoke<AutomationRunResult>("run_automation", { id });
      setLastRun((prev) => ({ ...prev, [id]: r }));
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  /* ----- helpers ---------------------------------------------------- */

  function newAutomation(): Automation {
    const now = Date.now();
    return {
      id: now, // millis make a good monotonic id on a single machine
      name: "New automation",
      description: "",
      kind: "apple_script",
      payload: "",
      created_at: now,
      updated_at: now,
      marker_hid: null,
    };
  }

  function startCreate() {
    setEditing(newAutomation());
  }

  function startEdit(a: Automation) {
    setEditing({ ...a });
  }

  async function commitEdit(a: Automation) {
    if (items === null) return;
    const idx = items.findIndex((x) => x.id === a.id);
    const next: Automation = { ...a, updated_at: Date.now() };
    const list = idx >= 0 ? items.map((x) => (x.id === a.id ? next : x)) : [...items, next];
    await persist(list);
    setEditing(null);
  }

  async function deleteOne(id: number) {
    if (items === null) return;
    const list = items.filter((x) => x.id !== id);
    await persist(list);
  }

  /* --------------------------------------------------------- render -- */

  return (
    <>
      <PageHeader
        title="Automations"
        description="Run AppleScripts, macOS Shortcuts, or shell commands from this app. Manual trigger today — a global-hotkey listener for keyboard-side triggers ships in v0.7."
        action={
          <Button variant="primary" onClick={startCreate} disabled={busy}>
            + New automation
          </Button>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      {items === null ? (
        <p className="text-sm text-fg-2">Loading…</p>
      ) : items.length === 0 ? (
        <Card>
          <div className="py-6 text-center">
            <p className="text-sm text-fg-2">
              Your library is empty. Click <b>+ New automation</b> to add an
              AppleScript, a macOS Shortcut, or a shell command.
            </p>
          </div>
        </Card>
      ) : (
        <div className="space-y-3">
          {items.map((a) => (
            <AutomationRow
              key={a.id}
              automation={a}
              lastRun={lastRun[a.id]}
              busy={busy}
              onRun={() => runOne(a.id)}
              onEdit={() => startEdit(a)}
              onDelete={() => deleteOne(a.id)}
            />
          ))}
        </div>
      )}

      {editing && (
        <Editor
          value={editing}
          shortcuts={shortcuts ?? []}
          onCancel={() => setEditing(null)}
          onSave={(a) => void commitEdit(a)}
        />
      )}
    </>
  );
}

/* ------------------------------------------------------ list row -- */

function AutomationRow({
  automation: a,
  lastRun,
  busy,
  onRun,
  onEdit,
  onDelete,
}: {
  automation: Automation;
  lastRun: AutomationRunResult | undefined;
  busy: boolean;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [showOutput, setShowOutput] = useState(false);
  return (
    <Card>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <div className="mb-1 flex items-center gap-2">
            <h3 className="truncate text-base font-medium text-fg-0">{a.name}</h3>
            <Badge tone="neutral">{KIND_LABEL[a.kind]}</Badge>
            {lastRun && (
              <Badge tone={lastRun.success ? "good" : "bad"}>
                {lastRun.success ? "ok" : `exit ${lastRun.exit_code ?? "?"}`}
              </Badge>
            )}
          </div>
          {a.description && (
            <p className="mb-2 text-sm text-fg-2">{a.description}</p>
          )}
          <p className="line-clamp-2 break-all font-mono text-xs text-fg-3">
            {a.payload || <span className="italic text-fg-4">(empty)</span>}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button size="sm" variant="primary" onClick={onRun} disabled={busy}>
            Run
          </Button>
          <Button size="sm" variant="ghost" onClick={onEdit} disabled={busy}>
            Edit
          </Button>
          <Button size="sm" variant="danger" onClick={onDelete} disabled={busy}>
            Delete
          </Button>
        </div>
      </div>
      {lastRun && (
        <div className="mt-3 border-t border-line/60 pt-3">
          <button
            onClick={() => setShowOutput((v) => !v)}
            className="text-xs text-fg-2 hover:text-fg-0"
          >
            {showOutput ? "Hide" : "Show"} output
          </button>
          {showOutput && (
            <div className="mt-2 space-y-2">
              {lastRun.stdout && (
                <pre className="overflow-x-auto rounded-sm border border-line bg-surface-base p-2 font-mono text-xs text-fg-1">
                  {lastRun.stdout}
                </pre>
              )}
              {lastRun.stderr && (
                <pre className="overflow-x-auto rounded-sm border border-bad/40 bg-bad-soft p-2 font-mono text-xs text-fg-1">
                  {lastRun.stderr}
                </pre>
              )}
              {!lastRun.stdout && !lastRun.stderr && (
                <p className="text-xs text-fg-3">No output captured.</p>
              )}
            </div>
          )}
        </div>
      )}
    </Card>
  );
}

/* ------------------------------------------------------ editor modal -- */

function Editor({
  value,
  shortcuts,
  onCancel,
  onSave,
}: {
  value: Automation;
  shortcuts: string[];
  onCancel: () => void;
  onSave: (a: Automation) => void;
}) {
  const [draft, setDraft] = useState<Automation>(value);
  const isValid = useMemo(
    () => draft.name.trim().length > 0 && draft.payload.trim().length > 0,
    [draft],
  );
  const isShell = draft.kind === "shell";

  return (
    <div
      role="dialog"
      aria-modal
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4 py-8"
      onClick={onCancel}
    >
      <div
        className="w-full max-w-xl rounded-lg border border-line bg-surface-elevated shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="border-b border-line px-5 pb-3 pt-4">
          <h2 className="text-base font-medium text-fg-0">
            {value.created_at === value.updated_at && draft.name === "New automation"
              ? "Create automation"
              : "Edit automation"}
          </h2>
        </header>

        <div className="space-y-4 px-5 py-4">
          <Field label="Name">
            <input
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              placeholder="Open my dev folder"
              className="w-full rounded-md border border-line bg-surface-base px-3 py-2 text-sm text-fg-0 outline-none focus:border-accent-500/60"
            />
          </Field>

          <Field label="Description (optional)">
            <input
              value={draft.description}
              onChange={(e) => setDraft({ ...draft, description: e.target.value })}
              placeholder="What this does"
              className="w-full rounded-md border border-line bg-surface-base px-3 py-2 text-sm text-fg-0 outline-none focus:border-accent-500/60"
            />
          </Field>

          <Field label="Type">
            <div className="flex gap-2">
              {(["apple_script", "shortcut", "shell"] as AutomationKind[]).map((k) => (
                <Button
                  key={k}
                  size="sm"
                  variant={draft.kind === k ? "ghost-active" : "ghost"}
                  onClick={() => setDraft({ ...draft, kind: k })}
                >
                  {KIND_LABEL[k]}
                </Button>
              ))}
            </div>
            <p className="mt-2 text-xs text-fg-3">{KIND_HINT[draft.kind]}</p>
          </Field>

          {draft.kind === "shortcut" && (
            <Field label="Installed Shortcuts">
              {shortcuts.length === 0 ? (
                <p className="text-xs text-fg-3">
                  No Shortcuts detected. Make sure you're on macOS 12+ and have at
                  least one Shortcut saved in the Shortcuts.app.
                </p>
              ) : (
                <div className="max-h-32 overflow-y-auto rounded-md border border-line bg-surface-base p-2">
                  <ul className="space-y-px">
                    {shortcuts.map((s) => (
                      <li key={s}>
                        <button
                          type="button"
                          onClick={() => setDraft({ ...draft, payload: s })}
                          className={[
                            "w-full rounded-sm px-2 py-1 text-left text-xs transition",
                            s === draft.payload
                              ? "bg-accent-500/20 text-fg-0"
                              : "text-fg-1 hover:bg-surface-raised hover:text-fg-0",
                          ].join(" ")}
                        >
                          {s}
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </Field>
          )}

          <Field
            label={
              draft.kind === "apple_script"
                ? "AppleScript body"
                : draft.kind === "shortcut"
                  ? "Shortcut name"
                  : "Shell command"
            }
          >
            <textarea
              value={draft.payload}
              onChange={(e) => setDraft({ ...draft, payload: e.target.value })}
              rows={draft.kind === "apple_script" || draft.kind === "shell" ? 6 : 1}
              placeholder={
                draft.kind === "apple_script"
                  ? 'tell application "Finder" to open folder "Documents" of home'
                  : draft.kind === "shortcut"
                    ? "My Shortcut"
                    : 'open -a "Safari" "https://example.com"'
              }
              className="w-full rounded-md border border-line bg-surface-base px-3 py-2 font-mono text-xs text-fg-0 outline-none focus:border-accent-500/60"
            />
          </Field>

          {isShell && (
            <div className="rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-fg-1">
              <b>Heads-up:</b> shell commands have no sandboxing. Don't paste anything
              you wouldn't run in <Mono>Terminal</Mono> yourself.
            </div>
          )}
        </div>

        <footer className="flex items-center justify-end gap-2 border-t border-line px-5 py-3">
          <Button size="sm" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            size="sm"
            variant="primary"
            onClick={() => onSave(draft)}
            disabled={!isValid}
          >
            Save
          </Button>
        </footer>
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="kicker mb-1 block">{label}</label>
      {children}
    </div>
  );
}
