import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { ISO_DE_LAYOUT_ROWS as ISO_DE_LAYOUT, type PhysicalKey } from "../data/layouts";
import { hidName } from "../data/hid-usage-names";
import {
  ACTION_GROUPS,
  type Action,
  type ActionEntry,
  type ActionGroup,
} from "../data/action-catalog";
import { formatError } from "../errors";

// Mirrors `KeyAction` from `ak820_protocol::commands::keymap`.
type KeyAction =
  | { kind: "default" }
  | { kind: "keyboard"; usage: number }
  | { kind: "mouse"; button: number; value: number }
  | { kind: "consumer_key"; value: number }
  | { kind: "macro"; macro_id: number; param2: number; param3: number }
  | { kind: "tgl"; value: number }
  | { kind: "func"; value: number }
  | { kind: "func_v2"; param1: number; param2: number }
  | { kind: "raw"; page: number; param1: number; param2: number; param3: number };

interface MacroSummary {
  macro_id: number;
  actions: { delay_ms: number; key_code: number; is_press: boolean; kind: string }[];
}

interface AutomationSummary {
  id: number;
  name: string;
  kind: string;
  marker_hid: number | null;
}

interface Keymap {
  slots: KeyAction[];
}

type Layer = "base" | "fn";

const MAIN_WIDTH = 700;
const CELL = 42;
const ROW_GAP = 4;
const RIGHT_COL_WIDTH = CELL;
const LED_COL_WIDTH = 14;
const KEYBOARD_NATURAL_W = MAIN_WIDTH + 6 + LED_COL_WIDTH + 6 + RIGHT_COL_WIDTH + 4;
const KEYBOARD_NATURAL_H = 6 * CELL + 5 * ROW_GAP + 4;
const KEYBOARD_SCALE_MIN = 0.55;
const KEYBOARD_SCALE_MAX = 1.5;

const LABEL_OVERRIDES: Record<string, string> = {
  "L-Win": "Win",
  "L-Alt": "Alt",
  "L-Shift": "Shift",
  "R-Shift": "Shift",
};

function displayLabel(label: string): string {
  return LABEL_OVERRIDES[label] ?? label;
}

export function Keymap() {
  const [layer, setLayer] = useState<Layer>("base");
  const [baseRemote, setBaseRemote] = useState<Keymap | null>(null);
  const [fnRemote, setFnRemote] = useState<Keymap | null>(null);
  const [baseDraft, setBaseDraft] = useState<Keymap | null>(null);
  const [fnDraft, setFnDraft] = useState<Keymap | null>(null);
  const [selectedSlot, setSelectedSlot] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [macros, setMacros] = useState<MacroSummary[]>([]);
  const [automations, setAutomations] = useState<AutomationSummary[]>([]);

  // NB. Macros must NOT be fetched in a separate useEffect — that would race
  // the base-keymap load() below for the std::sync::Mutex on `ConnState`.
  // Two concurrent HID invokes from different tokio workers + the 4-second
  // probe_device poll == frozen runtime. We hit this in System.tsx Phase 2
  // and again here when the Keymap view first added the macro tab. The fetch
  // is chained off load() instead (see useEffect with `load("base")`).
  //
  // Automations are host-only — no HID, safe to fetch in parallel.
  useEffect(() => {
    invoke<AutomationSummary[]>("list_automations")
      .then(setAutomations)
      .catch(() => setAutomations([]));
  }, []);

  async function refreshAutomations() {
    try {
      const next = await invoke<AutomationSummary[]>("list_automations");
      setAutomations(next);
    } catch {
      /* swallow */
    }
  }

  const macroActionGroup = useMemo<ActionGroup | null>(() => {
    if (macros.length === 0) return null;
    return {
      id: "macro",
      name: "Macros",
      description:
        "Trigger a stored macro by slot. Create or edit macros in the Macros tab.",
      entries: macros.map<ActionEntry>((m) => ({
        label: `M${m.macro_id + 1}`,
        hint: `M${m.macro_id + 1}`,
        action: {
          kind: "macro",
          macro_id: m.macro_id,
          param2: 0,
          param3: 0,
        },
      })),
    };
  }, [macros]);

  const automationActionGroup = useMemo<ActionGroup | null>(() => {
    if (automations.length === 0) return null;
    return {
      id: "automation",
      name: "Automations",
      description:
        "Trigger a host-side automation when this key is pressed. Up to 12 automations can be keyboard-triggered at once (markers F13–F24).",
      entries: automations.map<ActionEntry>((a) => ({
        label: a.name,
        hint: a.name.length > 8 ? a.name.slice(0, 7) + "…" : a.name,
        action: {
          kind: "automation_ref",
          automation_id: a.id,
          name: a.name,
        },
      })),
    };
  }, [automations]);

  const remote = layer === "base" ? baseRemote : fnRemote;
  const draft = layer === "base" ? baseDraft : fnDraft;
  const setDraft = layer === "base" ? setBaseDraft : setFnDraft;

  async function load(which: Layer) {
    setBusy(true);
    setErr(null);
    try {
      const km = await invoke<Keymap>(which === "base" ? "get_keymap" : "get_fn_keymap");
      if (which === "base") {
        setBaseRemote(km);
        setBaseDraft(km);
        // Sequential, not parallel — both `get_keymap` and `get_macros` need
        // the same HID mutex. Best-effort: if the keyboard has no macros yet
        // or the call fails, we just render an empty Macros action group.
        try {
          const m = await invoke<MacroSummary[]>("get_macros");
          setMacros(m);
        } catch {
          setMacros([]);
        }
      } else {
        setFnRemote(km);
        setFnDraft(km);
      }
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (!draft) return;
    setBusy(true);
    setErr(null);
    try {
      await invoke(layer === "base" ? "set_keymap" : "set_fn_keymap", { keymap: draft });
      // Re-read to confirm the device accepted everything verbatim.
      const verify = await invoke<Keymap>(layer === "base" ? "get_keymap" : "get_fn_keymap");
      if (layer === "base") { setBaseRemote(verify); setBaseDraft(verify); }
      else { setFnRemote(verify); setFnDraft(verify); }
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  function discard() {
    if (!remote) return;
    setDraft(remote);
    setSelectedSlot(null);
  }

  /// Pull the firmware's factory-default keymap for the active layer and
  /// stage it as the draft. The user reviews + clicks Save to commit.
  /// We never write defaults directly — every change goes through Save.
  async function resetToFactory() {
    setBusy(true);
    setErr(null);
    try {
      const defaults = await invoke<Keymap>(
        layer === "base" ? "get_default_keymap" : "get_default_fn_keymap",
      );
      setDraft(defaults);
      setSelectedSlot(null);
    } catch (e) {
      setErr(`Couldn't read factory defaults: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  async function assignAction(slot: number, action: Action) {
    if (!draft) return;

    // automation_ref is a picker-only sentinel — translate into a real
    // KeyAction::Keyboard with the marker HID that the backend hands us.
    if (action.kind === "automation_ref") {
      setErr(null);
      try {
        const marker = await invoke<number>("assign_automation_marker", {
          id: action.automation_id,
          suggested: null,
        });
        await refreshAutomations();
        const next: Keymap = { slots: draft.slots.slice() };
        next.slots[slot] = { kind: "keyboard", usage: marker };
        setDraft(next);
      } catch (e) {
        setErr(`Couldn't bind automation: ${e}`);
      }
      return;
    }

    const next: Keymap = { slots: draft.slots.slice() };
    next.slots[slot] = action as KeyAction;
    setDraft(next);
  }

  useEffect(() => {
    void load("base");
  }, []);

  useEffect(() => {
    if (layer === "fn" && !fnRemote && !busy) {
      void load("fn");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layer]);

  const dirty = useMemo(() => {
    if (!remote || !draft) return new Set<number>();
    const s = new Set<number>();
    for (let i = 0; i < draft.slots.length; i++) {
      if (!sameAction(draft.slots[i], remote.slots[i])) s.add(i);
    }
    return s;
  }, [remote, draft]);

  const isDirty = dirty.size > 0;

  return (
    <>
      <PageHeader
        title="Keymap"
        description={
          layer === "fn"
            ? "Hold Fn to access this layer. Empty slots fall back to the base layer's mapping."
            : "Click a key, then pick a new action below to remap it. Save writes every change back to the keyboard at once."
        }
        action={
          <div className="flex items-center gap-2">
            <LayerSwitch value={layer} onChange={(l) => { setLayer(l); setSelectedSlot(null); }} />
            <Button onClick={() => load(layer)} disabled={busy}>
              {busy ? "Reading…" : "Reload"}
            </Button>
            <Button
              variant="ghost"
              onClick={resetToFactory}
              disabled={busy}
              title="Stage the firmware's factory-default keymap for the active layer. Review, then Save to commit."
            >
              Factory default
            </Button>
            <Button variant={isDirty ? "ghost-active" : "ghost"} onClick={discard} disabled={busy || !isDirty}>
              Discard
            </Button>
            <Button variant="primary" onClick={save} disabled={busy || !isDirty}>
              {busy ? "Saving…" : isDirty ? `Save (${dirty.size})` : "Saved"}
            </Button>
          </div>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <Card
        kicker={layer === "base" ? "Base layer" : "Fn layer"}
        title={
          !draft
            ? "Reading from keyboard…"
            : isDirty
              ? `${dirty.size} unsaved change${dirty.size === 1 ? "" : "s"} — click Save to commit`
              : remapCountLabel(draft)
        }
      >
        <KeyboardSurface
          layout={ISO_DE_LAYOUT}
          km={draft}
          dirty={dirty}
          selectedSlot={selectedSlot}
          automations={automations}
          onSelect={setSelectedSlot}
        />
      </Card>

      <Card title="Action picker" className="mt-6">
        {selectedSlot === null ? (
          <p className="text-sm text-fg-2">
            Click a key in the layout above to pick a new action for it.
          </p>
        ) : (
          <>
            {isFRowSlot(selectedSlot) && layer === "base" && (
              <div className="mb-4 rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-fg-1">
                <b>Heads-up:</b> in <span className="font-mono">Mac</span> mode (hardware switch on the back), the keyboard
                preempts the F-row base layer with media keys (brightness, volume, …). Remaps here may not fire on plain
                F-key press. Switch to the <span className="font-mono">Fn</span> layer above for F-row macros that should
                trigger via <span className="font-mono">Fn + F-key</span>.
              </div>
            )}
            <ActionPicker
              slot={selectedSlot}
              defaultHid={defaultHidForSlot(selectedSlot)}
              currentAction={draft?.slots[selectedSlot]}
              extraGroups={[macroActionGroup, automationActionGroup]}
              onAssign={(a) => void assignAction(selectedSlot, a)}
            />
          </>
        )}
      </Card>

      <Card title="Legend" className="mt-6">
        <div className="flex flex-wrap items-center gap-5 text-xs text-fg-2">
          <LegendDot tone="bg-surface-raised border-line" label="factory default" />
          <LegendDot tone="bg-accent-500/20 border-accent-500/60" label="remapped (this layer)" />
          <LegendDot tone="bg-warn/15 border-warn/40" label="advanced action (macro, layer, function, mouse…)" />
          <LegendDot tone="bg-accent-500/40 border-accent-300 animate-pulse" label="unsaved change" />
        </div>
      </Card>
    </>
  );
}

function remapCountLabel(km: Keymap): string {
  let n = 0;
  for (let i = 0; i < km.slots.length; i++) {
    const def = defaultHidForSlot(i);
    if (isRemapped(km.slots[i], def)) n += 1;
  }
  return n === 0 ? "Factory default — no overrides" : `${n} key${n === 1 ? "" : "s"} remapped`;
}

function defaultHidForSlot(slot: number): number | null {
  for (const row of ISO_DE_LAYOUT) {
    for (const k of row) if (k.slot === slot) return k.hid;
  }
  return null;
}

function isRemapped(a: KeyAction, defaultHid: number | null): boolean {
  if (a.kind === "default") return false;
  if (a.kind === "keyboard") return defaultHid !== null && a.usage !== defaultHid;
  return true;
}

function sameAction(a: KeyAction, b: KeyAction): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

// ---------------------------------------------------------------------------
// Keyboard surface

function KeyboardSurface({
  layout,
  km,
  dirty,
  selectedSlot,
  automations,
  onSelect,
}: {
  layout: PhysicalKey[][];
  km: Keymap | null;
  dirty: Set<number>;
  selectedSlot: number | null;
  automations: AutomationSummary[];
  onSelect: (slot: number | null) => void;
}) {
  const NAV_LABELS = new Set(["Ende", "Bild↑", "Bild↓"]);
  const mainRows = layout.map((row) =>
    NAV_LABELS.has(row[row.length - 1]?.label ?? "") ? row.slice(0, -1) : row,
  );
  const navByRow = layout.map((row) => {
    const last = row[row.length - 1];
    return last && NAV_LABELS.has(last.label) ? last : null;
  });

  const TFT_HEIGHT = CELL * 2 + ROW_GAP;

  return (
    <ResponsiveScale natural={{ w: KEYBOARD_NATURAL_W, h: KEYBOARD_NATURAL_H }}>
      <div
        className="flex items-start gap-1.5 p-0.5"
        style={{ width: KEYBOARD_NATURAL_W }}
      >
        <div className="flex flex-col gap-1">
          {mainRows.map((row, ri) => (
            <div key={ri} className="flex gap-1" style={{ width: MAIN_WIDTH }}>
              {row.map((k, ki) => (
                <Cap
                  key={ki}
                  pk={k}
                  km={km}
                  isLastInRow={ki === row.length - 1}
                  dirty={dirty.has(k.slot)}
                  selected={selectedSlot === k.slot}
                  automations={automations}
                  onSelect={onSelect}
                />
              ))}
            </div>
          ))}
        </div>

        <div className="flex flex-col gap-1" style={{ width: LED_COL_WIDTH }}>
          <Slot empty />
          <Slot empty />
          <Slot><LedLabel>C</LedLabel></Slot>
          <Slot><LedLabel>W</LedLabel></Slot>
          <Slot><LedDot /></Slot>
          <Slot><LedLabel>⚡</LedLabel></Slot>
        </div>

        <div className="flex flex-col gap-1" style={{ width: RIGHT_COL_WIDTH }}>
          <Knob />
          {navByRow[1] && (
            <Cap pk={navByRow[1]} km={km} isLastInRow={false}
              dirty={dirty.has(navByRow[1].slot)}
              selected={selectedSlot === navByRow[1].slot}
              automations={automations}
              onSelect={onSelect} />
          )}
          {navByRow[2] && (
            <Cap pk={navByRow[2]} km={km} isLastInRow={false}
              dirty={dirty.has(navByRow[2].slot)}
              selected={selectedSlot === navByRow[2].slot}
              automations={automations}
              onSelect={onSelect} />
          )}
          {navByRow[3] && (
            <Cap pk={navByRow[3]} km={km} isLastInRow={false}
              dirty={dirty.has(navByRow[3].slot)}
              selected={selectedSlot === navByRow[3].slot}
              automations={automations}
              onSelect={onSelect} />
          )}
          <TFTPlaceholder height={TFT_HEIGHT} />
        </div>
      </div>
    </ResponsiveScale>
  );
}

// ---------------------------------------------------------------------------
// Single keycap

interface CapStyle {
  width: number;
  height: number;
  flexGrow?: number;
  marginLeft?: number | "auto";
  marginRight?: number;
  marginTop?: number;
}

function capStyleFor(cls: string | undefined, label: string, isLastInRow: boolean): CapStyle {
  const c = cls ?? "";
  const flexGrow = c.includes("flex-1") && label === "Spacebar" ? 1 : undefined;
  const width = c.includes("w-27") ? 70
    : c.includes("w-24") ? 62
    : c.includes("w-18") ? 52
    : c.includes("w-15") ? 42
    : c.includes("flex-1") && label !== "Spacebar" ? 78
    : 38;
  const height = 42;

  let marginLeft: CapStyle["marginLeft"];
  if (c.includes("ml-auto") && /^F\d/.test(label)) marginLeft = 10;
  else if (c.includes("ml-auto")) marginLeft = undefined;
  if (c.includes("ml-5")) marginLeft = 14;
  if (label === "Entf") marginLeft = 10;
  if (isLastInRow && label === "↑") marginLeft = "auto";

  let marginRight: number | undefined;
  if (label === "↑") marginRight = 42;
  if (c.includes("mr-3")) marginRight = 8;

  return { width, height, flexGrow, marginLeft, marginRight, marginTop: undefined };
}

interface LabelParts { primary: string; alt?: string }

function splitLabel(label: string): LabelParts {
  if (/^F\d{1,2}$/.test(label)) return { primary: label };
  if (/^[A-Za-zÄÖÜäöü\-↑↓→←]+$/.test(label) && label.length > 1) return { primary: label };
  if (label.includes(" ")) {
    const [primary, ...rest] = label.split(" ");
    return { primary, alt: rest.join(" ") || undefined };
  }
  if ([...label].length === 1) return { primary: label };
  const arr = [...label];
  return { primary: arr[0], alt: arr.slice(1).join("") };
}

function Cap({
  pk, km, isLastInRow, dirty, selected, automations, onSelect,
}: {
  pk: PhysicalKey;
  km: Keymap | null;
  isLastInRow: boolean;
  dirty: boolean;
  selected: boolean;
  automations: AutomationSummary[];
  onSelect: (slot: number | null) => void;
}) {
  const action = km?.slots[pk.slot];
  const detail = describeAction(action, pk, automations);
  const remapped = detail.tone !== "default";
  const s = capStyleFor(pk.cls, pk.label, isLastInRow);
  const parts = splitLabel(displayLabel(pk.label));

  const toneClasses = selected
    ? "border-accent-300 bg-accent-500/30 text-fg-0 shadow-[0_0_0_2px_rgba(124,92,255,0.5)]"
    : dirty
      ? "border-accent-300 bg-accent-500/40 text-fg-0 animate-pulse"
      : remapped
        ? detail.tone === "accent"
          ? "border-accent-500/60 bg-accent-500/15 text-fg-0 shadow-[0_0_0_1px_rgba(124,92,255,0.15)]"
          : "border-warn/40 bg-warn/10 text-warn"
        : "border-line bg-surface-raised text-fg-1";

  return (
    <button
      type="button"
      title={detail.title}
      onClick={(e) => { e.stopPropagation(); onSelect(selected ? null : pk.slot); }}
      className={[
        "relative flex select-none flex-col rounded-md border text-xs font-medium",
        "transition-[transform,background-color,border-color,box-shadow] duration-150",
        "hover:border-line-strong active:translate-y-px",
        toneClasses,
      ].join(" ")}
      style={{
        flexGrow: s.flexGrow,
        flexBasis: s.flexGrow ? 0 : `${s.width}px`,
        minWidth: `${s.width}px`,
        height: `${s.height}px`,
        marginLeft: s.marginLeft,
        marginRight: s.marginRight,
      }}
    >
      <span className="relative flex flex-1 flex-col items-center justify-center px-1.5 leading-none">
        {parts.alt && (
          <span className="absolute left-1.5 top-1 text-[8.5px] font-normal tracking-wider text-fg-3">
            {parts.alt}
          </span>
        )}
        <span className="truncate text-center text-[11px]">{parts.primary}</span>
      </span>
      {remapped && (
        <span
          className={[
            "block w-full truncate rounded-b-[5px] px-1 py-0.5 text-center font-mono tabular text-[8.5px] uppercase tracking-wider",
            detail.tone === "accent"
              ? "bg-accent-500/25 text-accent-200"
              : "bg-warn/20 text-warn",
          ].join(" ")}
        >
          → {detail.short}
        </span>
      )}
    </button>
  );
}

interface ActionDetail { short: string; title: string; tone: "default" | "accent" | "warn" }

function describeAction(
  a: KeyAction | undefined,
  pk: PhysicalKey,
  automations: AutomationSummary[],
): ActionDetail {
  if (!a || a.kind === "default") {
    return { short: hidName(pk.hid), title: `Slot ${pk.slot} · default (${hidName(pk.hid)})`, tone: "default" };
  }
  switch (a.kind) {
    case "keyboard": {
      // Markers in HID 104..115 (F13..F24) may be bound to a host automation.
      // If so, surface the automation name on the cap so the user sees the
      // mapping rather than a meaningless F-key label.
      if (a.usage >= 104 && a.usage <= 115) {
        const auto = automations.find((au) => au.marker_hid === a.usage);
        if (auto) {
          const short = auto.name.length > 5 ? auto.name.slice(0, 4) + "…" : auto.name;
          return {
            short,
            title: `Slot ${pk.slot} · automation "${auto.name}" (marker F${a.usage - 91})`,
            tone: "accent",
          };
        }
      }
      if (a.usage === pk.hid) {
        return { short: hidName(a.usage), title: `Slot ${pk.slot} · default (${hidName(a.usage)})`, tone: "default" };
      }
      return {
        short: hidName(a.usage),
        title: `Slot ${pk.slot} · remapped to ${hidName(a.usage)} (HID 0x${a.usage.toString(16)})`,
        tone: "accent",
      };
    }
    case "mouse":
      return { short: `M${a.button}`, title: `Slot ${pk.slot} · mouse button ${a.button}`, tone: "warn" };
    case "consumer_key":
      return { short: "media", title: `Slot ${pk.slot} · consumer key 0x${a.value.toString(16)}`, tone: "warn" };
    case "macro":
      return { short: `M${a.macro_id + 1}`, title: `Slot ${pk.slot} · macro M${a.macro_id + 1}`, tone: "accent" };
    case "tgl":
      return { short: `L${a.value}`, title: `Slot ${pk.slot} · toggle layer ${a.value}`, tone: "warn" };
    case "func":
      return { short: "func", title: `Slot ${pk.slot} · FUNC 0x${a.value.toString(16).padStart(6, "0")}`, tone: "warn" };
    case "func_v2":
      return { short: "fn", title: `Slot ${pk.slot} · FUNC_V2 ${a.param1.toString(16)} / ${a.param2.toString(16)}`, tone: "warn" };
    case "raw":
      return { short: `0x${a.page.toString(16)}`, title: `Slot ${pk.slot} · raw page=0x${a.page.toString(16)} params=${a.param1},${a.param2},${a.param3}`, tone: "warn" };
  }
}

// ---------------------------------------------------------------------------
// Action picker (the catalog below the keyboard)

function ActionPicker({
  slot,
  defaultHid,
  currentAction,
  extraGroups,
  onAssign,
}: {
  slot: number;
  defaultHid: number | null;
  currentAction: KeyAction | undefined;
  extraGroups: (ActionGroup | null)[];
  onAssign: (a: Action) => void;
}) {
  const groups = useMemo<ActionGroup[]>(
    () => [
      ...ACTION_GROUPS,
      ...extraGroups.filter((g): g is ActionGroup => g !== null),
    ],
    [extraGroups],
  );
  const [groupId, setGroupId] = useState<string>(groups[0].id);
  const group = groups.find((g) => g.id === groupId) ?? groups[0];

  const slotMeta = layoutSlotMeta(slot);

  return (
    <div>
      <header className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <p className="kicker mb-1">Selected key</p>
          <div className="flex items-baseline gap-3">
            <span className="text-lg font-medium text-fg-0">{slotMeta?.label ?? `Slot ${slot}`}</span>
            {defaultHid !== null && (
              <Badge tone="neutral">default: {hidName(defaultHid)}</Badge>
            )}
            {currentAction && currentAction.kind !== "default" && (
              <Badge tone="accent">now: {actionPreview(currentAction)}</Badge>
            )}
          </div>
        </div>
        <GroupTabs groups={groups} value={groupId} onChange={setGroupId} />
      </header>

      {group.description && (
        <p className="mb-3 text-xs text-fg-2">{group.description}</p>
      )}

      <div className="flex flex-wrap gap-1.5">
        {group.entries.map((entry, i) => (
          <PickerButton
            key={i}
            entry={entry}
            current={currentAction}
            onClick={() => onAssign(entry.action)}
          />
        ))}
      </div>
    </div>
  );
}

function PickerButton({
  entry,
  current,
  onClick,
}: {
  entry: ActionEntry;
  current: KeyAction | undefined;
  onClick: () => void;
}) {
  const active = current && sameAction(entry.action as KeyAction, current);
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        "h-9 min-w-[42px] rounded-md border px-2.5 text-xs font-medium transition-colors",
        active
          ? "border-accent-500 bg-accent-500/20 text-fg-0"
          : "border-line bg-surface-elevated/40 text-fg-1 hover:border-line-strong hover:bg-surface-raised hover:text-fg-0",
      ].join(" ")}
    >
      {entry.hint ?? entry.label}
    </button>
  );
}

function GroupTabs({
  groups,
  value,
  onChange,
}: {
  groups: ActionGroup[];
  value: string;
  onChange: (id: string) => void;
}) {
  return (
    <nav className="flex flex-wrap gap-1">
      {groups.map((g) => (
        <button
          key={g.id}
          onClick={() => onChange(g.id)}
          className={[
            "rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
            value === g.id
              ? "bg-surface-raised text-fg-0"
              : "text-fg-2 hover:bg-surface-elevated/50 hover:text-fg-0",
          ].join(" ")}
        >
          {g.name}
        </button>
      ))}
    </nav>
  );
}

function actionPreview(a: KeyAction): string {
  switch (a.kind) {
    case "default": return "factory default";
    case "keyboard": return hidName(a.usage);
    case "mouse": return `mouse ${a.button}`;
    case "consumer_key": return `media 0x${a.value.toString(16)}`;
    case "macro": return `macro M${a.macro_id + 1}`;
    case "tgl": return `layer ${a.value}`;
    case "func": return `func 0x${a.value.toString(16)}`;
    case "func_v2": return "func v2";
    case "raw": return `raw 0x${a.page.toString(16)}`;
  }
}

function layoutSlotMeta(slot: number): PhysicalKey | null {
  for (const row of ISO_DE_LAYOUT) for (const k of row) if (k.slot === slot) return k;
  return null;
}

/** True when the slot is F1..F12 (slots 1..12 on the AK820 Pro matrix). */
function isFRowSlot(slot: number): boolean {
  return slot >= 1 && slot <= 12;
}

// ---------------------------------------------------------------------------
// Layout helpers + decorative right column

function Slot({ children, empty }: { children?: ReactNode; empty?: boolean }) {
  return (
    <div
      className={empty ? "" : "flex items-center justify-center text-fg-3"}
      style={{ height: CELL, width: "100%" }}
    >
      {children}
    </div>
  );
}

function LedLabel({ children }: { children: ReactNode }) {
  return <span className="text-[9px] uppercase tracking-wider">{children}</span>;
}

function LedDot() { return <span className="h-1 w-1 rounded-full bg-fg-4" />; }

function Knob() {
  return (
    <div
      title="Drehknopf · configuration coming soon"
      className="relative flex items-center justify-center rounded-full border border-line-strong bg-gradient-to-br from-surface-raised to-surface-elevated shadow-inner"
      style={{ width: RIGHT_COL_WIDTH, height: CELL }}
    >
      <div
        className="rounded-full border border-line bg-surface-base/60"
        style={{ width: RIGHT_COL_WIDTH - 14, height: CELL - 14 }}
      />
      <span className="absolute h-1.5 w-1.5 -translate-y-[10px] rounded-full bg-accent-300/80 shadow-[0_0_4px_rgba(124,92,255,0.6)]" />
    </div>
  );
}

function TFTPlaceholder({ height }: { height: number }) {
  return (
    <div
      title="0.85″ TFT display · custom GIFs land in phase 5"
      className="rounded-md border border-line bg-surface-base p-1"
      style={{ width: RIGHT_COL_WIDTH, height }}
    >
      <div className="flex h-full flex-col items-center justify-center gap-0.5 rounded-sm border border-line/40 bg-gradient-to-br from-[#0a1428] to-[#0e1c3a] text-[8.5px] uppercase tracking-wider text-fg-3">
        <span className="font-mono tabular text-fg-2">10:43</span>
        <span>05/14</span>
        <span className="font-mono tabular text-accent-300">100%</span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Misc

function LayerSwitch({ value, onChange }: { value: Layer; onChange: (v: Layer) => void }) {
  return (
    <div className="inline-flex overflow-hidden rounded-md border border-line bg-surface-base">
      {(["base", "fn"] as Layer[]).map((l) => (
        <button
          key={l}
          onClick={() => onChange(l)}
          className={[
            "px-3 py-1.5 text-xs font-medium transition-colors",
            value === l ? "bg-surface-raised text-fg-0" : "text-fg-2 hover:text-fg-0",
          ].join(" ")}
        >
          {l === "base" ? "Base" : "Fn layer"}
        </button>
      ))}
    </div>
  );
}

function LegendDot({ tone, label }: { tone: string; label: string }) {
  return (
    <span className="inline-flex items-center gap-2">
      <span className={`inline-block h-3 w-4 rounded-sm border ${tone}`} />
      {label}
    </span>
  );
}

function ResponsiveScale({
  natural,
  children,
}: { natural: { w: number; h: number }; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const w = entry.contentRect.width;
      const s = Math.max(KEYBOARD_SCALE_MIN, Math.min(KEYBOARD_SCALE_MAX, w / natural.w));
      setScale(s);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [natural.w]);

  return (
    <div ref={ref} className="relative w-full" style={{ height: natural.h * scale }}>
      <div
        className="absolute left-1/2 top-0 origin-top"
        style={{ width: natural.w, transform: `translateX(-50%) scale(${scale})` }}
      >
        {children}
      </div>
    </div>
  );
}
