import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { PageHeader } from "../components/Layout";
import { Badge, Button, Card, ErrorBanner, Mono } from "../components/ui";
import { formatError } from "../errors";

/* ----------------------------------------------------------------- types -- */

type ActionKind = "keyboard" | "mouse";

interface MacroAction {
  delay_ms: number;
  key_code: number;
  is_press: boolean;
  kind: ActionKind;
}

interface Macro {
  macro_id: number;
  name: string | null;
  actions: MacroAction[];
}

interface MacroLimits {
  slot_count: number;
  byte_limit: number;
  max_actions_per_macro: number;
}

/* --------------------------------------------------- browser → HID maps -- */

/**
 * Subset of `KeyboardEvent.code` → HID Keyboard Usage Page values.
 * Keep this list pragmatic — anything not here gets recorded as keycode 0 with
 * a warning so the user can manually fix the event later.
 */
const CODE_TO_HID: Record<string, number> = {
  // Letters A–Z (HID 4..29)
  ...Object.fromEntries(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map((c, i) => [`Key${c}`, 4 + i]),
  ),
  // Digits 1..9 then 0 (HID 30..39)
  ...Object.fromEntries(
    "123456789".split("").map((d, i) => [`Digit${d}`, 30 + i]),
  ),
  Digit0: 39,
  // Whitespace & editing
  Enter: 40,
  Escape: 41,
  Backspace: 42,
  Tab: 43,
  Space: 44,
  Minus: 45,
  Equal: 46,
  BracketLeft: 47,
  BracketRight: 48,
  Backslash: 49,
  Semicolon: 51,
  Quote: 52,
  Backquote: 53,
  Comma: 54,
  Period: 55,
  Slash: 56,
  CapsLock: 57,
  // F1..F12 (HID 58..69)
  ...Object.fromEntries(
    Array.from({ length: 12 }, (_, i) => [`F${i + 1}`, 58 + i]),
  ),
  PrintScreen: 70,
  ScrollLock: 71,
  Pause: 72,
  Insert: 73,
  Home: 74,
  PageUp: 75,
  Delete: 76,
  End: 77,
  PageDown: 78,
  ArrowRight: 79,
  ArrowLeft: 80,
  ArrowDown: 81,
  ArrowUp: 82,
  // Modifiers (HID 224..231)
  ControlLeft: 224,
  ShiftLeft: 225,
  AltLeft: 226,
  MetaLeft: 227,
  ControlRight: 228,
  ShiftRight: 229,
  AltRight: 230,
  MetaRight: 231,
};

/** Lookup for the action editor's label column. */
const HID_TO_LABEL: Record<number, string> = Object.fromEntries(
  Object.entries(CODE_TO_HID).map(([code, hid]) => [hid, prettyCode(code)]),
);
function prettyCode(c: string): string {
  if (c.startsWith("Key")) return c.slice(3);
  if (c.startsWith("Digit")) return c.slice(5);
  if (c.startsWith("F") && /^F\d{1,2}$/.test(c)) return c;
  return c.replace(/([A-Z])/g, " $1").trim();
}

/* ----------------------------------------------------------- component -- */

export function Macros() {
  const [remote, setRemote] = useState<Macro[] | null>(null);
  const [draft, setDraft] = useState<Macro[]>([]);
  const [names, setNames] = useState<Record<number, string>>({});
  const [limits, setLimits] = useState<MacroLimits | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const recordRef = useRef<{ lastAt: number; actions: MacroAction[] } | null>(
    null,
  );

  /* ---------- IO ---------- */

  // Stable callback (no deps that change after first mount). The previous
  // version captured `limits` in the dep array; setting limits after the
  // first call recreated `refresh`, which re-fired the mount effect and
  // queued a SECOND concurrent `get_macros` invoke. Two simultaneous HID
  // invokes both grab the sync `ConnState` mutex from different tokio
  // workers and — together with the 4-second probe_device poll — exhaust
  // the runtime. We hit the exact same deadlock on the System tab in Phase 2.
  const refresh = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      const l = await invoke<MacroLimits>("macro_limits");
      setLimits(l);
      const m = await invoke<Macro[]>("get_macros");
      setRemote(m);
      setDraft(m.map(cloneMacro));
      // Names are host-side only — preserve through reads. Drop entries for
      // slots that no longer exist on the device.
      setNames((prev) => {
        const next: Record<number, string> = {};
        for (const macro of m) {
          if (prev[macro.macro_id]) next[macro.macro_id] = prev[macro.macro_id];
        }
        return next;
      });
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }, []);

  // Guard against the React 19 + Strict-Mode double-invocation pattern as
  // well: if the effect is somehow scheduled twice in quick succession,
  // only let the first call talk to the device.
  const didInit = useRef(false);
  useEffect(() => {
    if (didInit.current) return;
    didInit.current = true;
    refresh();
  }, [refresh]);

  /* ---------- recorder ---------- */

  useEffect(() => {
    if (!recording || selectedId === null) return;
    recordRef.current = { lastAt: performance.now(), actions: [] };

    function captureEvent(e: KeyboardEvent, isPress: boolean) {
      // Don't let the browser handle these — they'd e.g. open DevTools.
      e.preventDefault();
      e.stopPropagation();
      if (recordRef.current === null) return;
      const now = performance.now();
      const delay = Math.min(
        65535,
        Math.max(0, Math.round(now - recordRef.current.lastAt)),
      );
      const hid = CODE_TO_HID[e.code] ?? 0;
      recordRef.current.actions.push({
        delay_ms: delay,
        key_code: hid,
        is_press: isPress,
        kind: "keyboard",
      });
      recordRef.current.lastAt = now;
    }

    const down = (e: KeyboardEvent) => captureEvent(e, true);
    const up = (e: KeyboardEvent) => captureEvent(e, false);
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
    };
  }, [recording, selectedId]);

  function stopRecording() {
    setRecording(false);
    if (recordRef.current === null || selectedId === null) {
      recordRef.current = null;
      return;
    }
    const captured = recordRef.current.actions;
    recordRef.current = null;
    if (captured.length === 0) return;
    setDraft((d) =>
      d.map((m) =>
        m.macro_id === selectedId
          ? { ...m, actions: [...m.actions, ...captured] }
          : m,
      ),
    );
  }

  /* ---------- editing helpers ---------- */

  const selected = useMemo(
    () => draft.find((m) => m.macro_id === selectedId) ?? null,
    [draft, selectedId],
  );

  function withSelected(
    update: (m: Macro) => Macro,
  ): void {
    if (selectedId === null) return;
    setDraft((d) => d.map((m) => (m.macro_id === selectedId ? update(m) : m)));
  }

  function updateDelay(idx: number, value: number) {
    withSelected((m) => {
      const next = [...m.actions];
      next[idx] = { ...next[idx], delay_ms: Math.max(0, Math.min(65535, value)) };
      return { ...m, actions: next };
    });
  }

  function removeAction(idx: number) {
    withSelected((m) => ({ ...m, actions: m.actions.filter((_, i) => i !== idx) }));
  }

  function clearActions() {
    withSelected((m) => ({ ...m, actions: [] }));
  }

  function addMacroSlot() {
    if (limits === null) return;
    // Pick the lowest unused id in [0, slot_count).
    const used = new Set(draft.map((m) => m.macro_id));
    let candidate = 0;
    while (used.has(candidate) && candidate < limits.slot_count) candidate += 1;
    if (candidate >= limits.slot_count) {
      setErr(`All ${limits.slot_count} macro slots are taken.`);
      return;
    }
    const fresh: Macro = { macro_id: candidate, name: null, actions: [] };
    setDraft((d) => [...d, fresh].sort((a, b) => a.macro_id - b.macro_id));
    setSelectedId(candidate);
  }

  function deleteMacro(id: number) {
    setDraft((d) => d.filter((m) => m.macro_id !== id));
    if (selectedId === id) setSelectedId(null);
  }

  /* ---------- name editing (host-only) ---------- */

  function setName(id: number, name: string) {
    setNames((n) => ({ ...n, [id]: name }));
  }

  /* ---------- save / discard ---------- */

  const dirty = useMemo(() => {
    if (remote === null) return false;
    if (remote.length !== draft.length) return true;
    return JSON.stringify(stripNames(remote)) !== JSON.stringify(stripNames(draft));
  }, [remote, draft]);

  async function save() {
    if (limits === null) return;
    // Validate per-macro byte limit on the client too — backend will reject
    // but we can surface a friendlier message.
    for (const m of draft) {
      if (m.actions.length === 0) continue;
      const size = 4 + m.actions.length * 4;
      if (size > limits.byte_limit) {
        setErr(
          `Macro #${m.macro_id} has ${m.actions.length} actions (${size} B) — exceeds ${limits.byte_limit} B device limit.`,
        );
        return;
      }
    }
    setBusy(true);
    setErr(null);
    try {
      // Wire format wants the strict list (id+actions). Names stay host-side.
      const payload = draft.map((m) => ({
        macro_id: m.macro_id,
        name: null as string | null,
        actions: m.actions,
      }));
      await invoke("set_macros", { macros: payload });
      await refresh();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  function discard() {
    if (remote === null) return;
    setDraft(remote.map(cloneMacro));
  }

  /* --------------------------------------------------------- render -- */

  return (
    <>
      <PageHeader
        title="Macros"
        description="Record key sequences once, replay them with a single key press. Up to 100 slots, 79 actions per macro."
        action={
          <div className="flex gap-2">
            {dirty && (
              <Button variant="ghost" size="sm" onClick={discard} disabled={busy}>
                Discard
              </Button>
            )}
            <Button variant="primary" size="sm" onClick={save} disabled={busy || !dirty}>
              {busy ? "Writing…" : dirty ? "Save to device" : "Saved"}
            </Button>
          </div>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6 lg:grid-cols-[280px_1fr]">
        {/* ----- slot list ----- */}
        <Card title="Slots" action={
          <Button size="sm" variant="ghost" onClick={addMacroSlot} disabled={busy || limits === null}>
            + New
          </Button>
        }>
          {remote === null ? (
            <p className="text-sm text-fg-2">Reading…</p>
          ) : draft.length === 0 ? (
            <p className="text-sm text-fg-2">No macros yet. Click <b>+ New</b> to create one.</p>
          ) : (
            <ul className="-mx-2 space-y-px">
              {draft.map((m) => {
                const isSel = m.macro_id === selectedId;
                const displayName = names[m.macro_id] || `Macro M${m.macro_id + 1}`;
                return (
                  <li key={m.macro_id}>
                    <button
                      onClick={() => setSelectedId(m.macro_id)}
                      className={[
                        "flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition",
                        isSel
                          ? "bg-surface-raised text-fg-0"
                          : "text-fg-1 hover:bg-surface-elevated/60 hover:text-fg-0",
                      ].join(" ")}
                    >
                      <span className="truncate">{displayName}</span>
                      <span className="ml-2 shrink-0 text-2xs text-fg-3">
                        {m.actions.length}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </Card>

        {/* ----- editor ----- */}
        <Card
          title={selected ? `Macro M${selected.macro_id + 1}` : "Editor"}
          action={
            selected && (
              <Button
                size="sm"
                variant="danger"
                onClick={() => deleteMacro(selected.macro_id)}
                disabled={busy}
              >
                Delete
              </Button>
            )
          }
        >
          {selected === null ? (
            <p className="text-sm text-fg-2">
              Select a macro on the left, or create a new one to start recording.
            </p>
          ) : (
            <MacroEditor
              macro={selected}
              limits={limits}
              name={names[selected.macro_id] ?? ""}
              onNameChange={(n) => setName(selected.macro_id, n)}
              recording={recording}
              onStartRecording={() => setRecording(true)}
              onStopRecording={stopRecording}
              onClear={clearActions}
              onUpdateDelay={updateDelay}
              onRemove={removeAction}
            />
          )}
        </Card>
      </div>
    </>
  );
}

/* ------------------------------------------------------ child: editor -- */

function MacroEditor({
  macro,
  limits,
  name,
  onNameChange,
  recording,
  onStartRecording,
  onStopRecording,
  onClear,
  onUpdateDelay,
  onRemove,
}: {
  macro: Macro;
  limits: MacroLimits | null;
  name: string;
  onNameChange: (n: string) => void;
  recording: boolean;
  onStartRecording: () => void;
  onStopRecording: () => void;
  onClear: () => void;
  onUpdateDelay: (idx: number, value: number) => void;
  onRemove: (idx: number) => void;
}) {
  const sizeBytes = macro.actions.length === 0 ? 0 : 4 + macro.actions.length * 4;
  const sizeBudget = limits?.byte_limit ?? 320;
  const sizeRatio = Math.min(1, sizeBytes / sizeBudget);

  return (
    <div className="space-y-5">
      {/* name */}
      <div>
        <label className="kicker mb-1 block">Name (local)</label>
        <input
          value={name}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder={`Macro M${macro.macro_id + 1}`}
          className="w-full rounded-md border border-line bg-surface-elevated/40 px-3 py-2 text-sm text-fg-0 outline-none focus:border-accent-500/60"
        />
        <p className="mt-1 text-xs text-fg-3">
          Names are stored on this Mac only — the keyboard firmware doesn't keep them.
        </p>
      </div>

      {/* recorder + meter */}
      <div className="flex flex-wrap items-center gap-3">
        {recording ? (
          <Button variant="danger" size="sm" onClick={onStopRecording}>
            ⏹ Stop recording
          </Button>
        ) : (
          <Button variant="primary" size="sm" onClick={onStartRecording}>
            ⏺ Record
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={onClear} disabled={macro.actions.length === 0 || recording}>
          Clear actions
        </Button>
        <div className="ml-auto flex items-center gap-3 text-xs text-fg-2">
          <span>
            <Mono>{macro.actions.length}</Mono> actions
          </span>
          <span className="flex items-center gap-2">
            <span className="relative h-1.5 w-24 overflow-hidden rounded-full bg-surface-base">
              <span
                className={[
                  "absolute inset-y-0 left-0 transition-all",
                  sizeRatio > 0.9 ? "bg-warn" : "bg-accent-500",
                ].join(" ")}
                style={{ width: `${sizeRatio * 100}%` }}
              />
            </span>
            <Mono>{sizeBytes}/{sizeBudget} B</Mono>
          </span>
        </div>
      </div>

      {recording && (
        <div className="rounded-md border border-accent-500/40 bg-accent-glow px-3 py-2 text-sm text-fg-1">
          Recording — every keypress in this window goes into the macro.
          <br />
          <span className="text-xs text-fg-3">
            Inter-event delays are captured as wall-clock milliseconds. Click <b>Stop</b> when done.
          </span>
        </div>
      )}

      {/* timeline */}
      {macro.actions.length === 0 ? (
        <p className="text-sm text-fg-3">
          Empty macro. Press <b>Record</b> and type to capture events.
        </p>
      ) : (
        <div className="overflow-hidden rounded-md border border-line">
          <table className="w-full text-sm">
            <thead className="bg-surface-elevated/40 text-xs uppercase tracking-wide text-fg-3">
              <tr>
                <th className="w-10 px-3 py-2 text-left">#</th>
                <th className="px-3 py-2 text-left">Action</th>
                <th className="px-3 py-2 text-left">Key</th>
                <th className="px-3 py-2 text-left">Delay (ms)</th>
                <th className="w-10" />
              </tr>
            </thead>
            <tbody>
              {macro.actions.map((a, i) => (
                <tr key={i} className="border-t border-line/60 hover:bg-surface-elevated/30">
                  <td className="px-3 py-1.5 text-fg-3">{i + 1}</td>
                  <td className="px-3 py-1.5">
                    <Badge tone={a.is_press ? "accent" : "neutral"}>
                      {a.is_press ? "press" : "release"}
                    </Badge>
                  </td>
                  <td className="px-3 py-1.5 font-mono text-fg-1">
                    {HID_TO_LABEL[a.key_code] ?? `0x${a.key_code.toString(16).padStart(2, "0")}`}
                    <span className="ml-2 text-xs text-fg-3">[{a.kind}]</span>
                  </td>
                  <td className="px-3 py-1.5">
                    <input
                      type="number"
                      min={0}
                      max={65535}
                      value={a.delay_ms}
                      onChange={(e) =>
                        onUpdateDelay(i, Number.parseInt(e.target.value || "0", 10))
                      }
                      className="w-20 rounded-sm border border-line bg-surface-base px-2 py-0.5 text-right font-mono text-xs text-fg-0 outline-none focus:border-accent-500/60"
                    />
                  </td>
                  <td className="px-3 py-1.5 text-right">
                    <button
                      onClick={() => onRemove(i)}
                      className="text-fg-3 hover:text-bad"
                      aria-label={`Remove action ${i + 1}`}
                    >
                      ×
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------ helpers -- */

function cloneMacro(m: Macro): Macro {
  return { macro_id: m.macro_id, name: m.name, actions: m.actions.map((a) => ({ ...a })) };
}

function stripNames(list: Macro[]): Macro[] {
  return list.map((m) => ({ macro_id: m.macro_id, name: null, actions: m.actions }));
}
