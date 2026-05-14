/**
 * Presets view — curated cross-cutting profiles (lighting + keymap
 * overrides + automation seeds) for common use-cases like FPS gaming,
 * dev workflows, office days.
 *
 * Apply flow:
 *   1. User picks a preset card.
 *   2. Modal opens with per-component toggles (lighting / keymap / fn /
 *      automations) so the user can take just the parts they want.
 *   3. Confirm → backend executes via `apply_preset` and returns a report.
 *   4. Report is rendered inline so the user sees exactly what changed.
 *
 * Applying is **additive**: it patches the current state, doesn't wipe
 * anything first. Keymap overrides only touch the listed slots — the
 * user's other remaps survive. Automation seeds are skipped if a same-
 * named entry already exists in the user's library.
 */

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import type {
  ApplyPresetOptions,
  ApplyPresetReport,
  Preset,
} from "../types";

export function Presets() {
  const [presets, setPresets] = useState<Preset[] | null>(null);
  const [activeCategory, setActiveCategory] = useState<string>("All");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<Preset | null>(null);
  const [lastReport, setLastReport] = useState<{ preset: Preset; report: ApplyPresetReport } | null>(null);

  useEffect(() => {
    invoke<Preset[]>("list_presets")
      .then(setPresets)
      .catch((e) => setErr(String(e)));
  }, []);

  const categories = useMemo(() => {
    if (!presets) return ["All"];
    const set = new Set(presets.map((p) => p.category));
    return ["All", ...Array.from(set)];
  }, [presets]);

  const visible = useMemo(() => {
    if (!presets) return [];
    if (activeCategory === "All") return presets;
    return presets.filter((p) => p.category === activeCategory);
  }, [presets, activeCategory]);

  async function commit(preset: Preset, opts: ApplyPresetOptions) {
    setBusy(true);
    setErr(null);
    try {
      const report = await invoke<ApplyPresetReport>("apply_preset", {
        id: preset.id,
        options: opts,
      });
      setLastReport({ preset, report });
      setReviewing(null);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title="Presets"
        description="One-click cross-cutting profiles. Each preset patches lighting, key remaps, and seeds the Automations library for a specific use case. Applied additively — your other settings are preserved."
      />

      <ErrorBanner>{err}</ErrorBanner>

      {lastReport && <ReportBanner report={lastReport.report} preset={lastReport.preset} onDismiss={() => setLastReport(null)} />}

      {presets === null ? (
        <p className="text-sm text-fg-2">Loading presets…</p>
      ) : (
        <>
          <div className="mb-5 flex flex-wrap gap-2">
            {categories.map((c) => (
              <Button
                key={c}
                size="sm"
                variant={activeCategory === c ? "ghost-active" : "ghost"}
                onClick={() => setActiveCategory(c)}
              >
                {c}
              </Button>
            ))}
          </div>

          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {visible.map((p) => (
              <PresetCard key={p.id} preset={p} onPick={() => setReviewing(p)} disabled={busy} />
            ))}
          </div>
        </>
      )}

      {reviewing && (
        <ApplyModal
          preset={reviewing}
          busy={busy}
          onCancel={() => setReviewing(null)}
          onApply={(opts) => void commit(reviewing, opts)}
        />
      )}
    </>
  );
}

/* ---------------------------------------------------- card -- */

function PresetCard({
  preset,
  onPick,
  disabled,
}: {
  preset: Preset;
  onPick: () => void;
  disabled: boolean;
}) {
  const componentBadges: string[] = [];
  if (preset.lighting) componentBadges.push("Lighting");
  if (preset.keymap_overrides.length > 0) componentBadges.push(`Keymap (${preset.keymap_overrides.length})`);
  if (preset.fn_keymap_overrides.length > 0) componentBadges.push(`Fn (${preset.fn_keymap_overrides.length})`);
  if (preset.automation_seeds.length > 0) componentBadges.push(`Automations (${preset.automation_seeds.length})`);

  return (
    <Card>
      <div className="flex items-start gap-3">
        <span className="text-2xl" aria-hidden>{preset.icon}</span>
        <div className="min-w-0 flex-1">
          <div className="mb-1 flex items-baseline gap-2">
            <h3 className="truncate text-base font-medium text-fg-0">{preset.name}</h3>
            <Badge tone="neutral">{preset.category}</Badge>
          </div>
          <p className="mb-3 text-sm text-fg-2">{preset.description}</p>
          <div className="mb-3 flex flex-wrap gap-1.5">
            {componentBadges.map((b) => (
              <span key={b} className="rounded-sm border border-line/60 bg-surface-base px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-fg-3">
                {b}
              </span>
            ))}
          </div>
          <Button size="sm" variant="primary" onClick={onPick} disabled={disabled}>
            Review &amp; apply
          </Button>
        </div>
      </div>
    </Card>
  );
}

/* ---------------------------------------------------- apply modal -- */

function ApplyModal({
  preset,
  busy,
  onCancel,
  onApply,
}: {
  preset: Preset;
  busy: boolean;
  onCancel: () => void;
  onApply: (opts: ApplyPresetOptions) => void;
}) {
  const [opts, setOpts] = useState<ApplyPresetOptions>({
    lighting: preset.lighting !== null,
    keymap: preset.keymap_overrides.length > 0,
    fn_keymap: preset.fn_keymap_overrides.length > 0,
    automations: preset.automation_seeds.length > 0,
  });
  const nothingPicked = !opts.lighting && !opts.keymap && !opts.fn_keymap && !opts.automations;

  return (
    <div
      role="dialog"
      aria-modal
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4 py-8"
      onClick={onCancel}
    >
      <div
        className="w-full max-w-lg rounded-lg border border-line bg-surface-elevated shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="border-b border-line px-5 pb-3 pt-4">
          <div className="flex items-baseline gap-2">
            <span className="text-xl">{preset.icon}</span>
            <h2 className="text-base font-medium text-fg-0">Apply: {preset.name}</h2>
          </div>
          <p className="mt-1 text-xs text-fg-3">{preset.description}</p>
        </header>

        <div className="space-y-3 px-5 py-4">
          <p className="text-xs text-fg-2">Pick what to apply — unchecked parts of your keyboard stay untouched.</p>
          <Option
            label="Lighting"
            available={preset.lighting !== null}
            checked={opts.lighting}
            onToggle={(v) => setOpts({ ...opts, lighting: v })}
            detail={preset.lighting ? `Mode ${preset.lighting.mode}, #${preset.lighting.color}, brightness ${preset.lighting.brightness}/5` : "Preset doesn't define lighting"}
          />
          <Option
            label="Base-layer key overrides"
            available={preset.keymap_overrides.length > 0}
            checked={opts.keymap}
            onToggle={(v) => setOpts({ ...opts, keymap: v })}
            detail={preset.keymap_overrides.length > 0 ? `${preset.keymap_overrides.length} slot(s) will be patched. Unchanged slots stay as they are.` : "Preset doesn't change base-layer keys"}
          />
          <Option
            label="Fn-layer key overrides"
            available={preset.fn_keymap_overrides.length > 0}
            checked={opts.fn_keymap}
            onToggle={(v) => setOpts({ ...opts, fn_keymap: v })}
            detail={preset.fn_keymap_overrides.length > 0 ? `${preset.fn_keymap_overrides.length} slot(s) will be patched on the Fn layer.` : "Preset doesn't change Fn-layer keys"}
          />
          <Option
            label="Add automations"
            available={preset.automation_seeds.length > 0}
            checked={opts.automations}
            onToggle={(v) => setOpts({ ...opts, automations: v })}
            detail={preset.automation_seeds.length > 0 ? `Adds ${preset.automation_seeds.length} entry/entries to your Automations library. Existing entries with the same name are kept (no overwrite).` : "Preset doesn't seed automations"}
          />
        </div>

        <footer className="flex items-center justify-end gap-2 border-t border-line px-5 py-3">
          <Button size="sm" variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button size="sm" variant="primary" onClick={() => onApply(opts)} disabled={busy || nothingPicked}>
            {busy ? "Applying…" : "Apply"}
          </Button>
        </footer>
      </div>
    </div>
  );
}

function Option({
  label,
  available,
  checked,
  detail,
  onToggle,
}: {
  label: string;
  available: boolean;
  checked: boolean;
  detail: string;
  onToggle: (v: boolean) => void;
}) {
  return (
    <label
      className={[
        "flex cursor-pointer items-start gap-3 rounded-md border px-3 py-2",
        available ? "border-line bg-surface-base hover:border-line-strong" : "border-line/40 bg-surface-base/40 opacity-50",
      ].join(" ")}
    >
      <input
        type="checkbox"
        checked={available && checked}
        disabled={!available}
        onChange={(e) => onToggle(e.target.checked)}
        className="mt-0.5 h-4 w-4 accent-accent-500"
      />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-fg-0">{label}</p>
        <p className="mt-0.5 text-xs text-fg-3">{detail}</p>
      </div>
    </label>
  );
}

/* ---------------------------------------------------- report -- */

function ReportBanner({
  report,
  preset,
  onDismiss,
}: {
  report: ApplyPresetReport;
  preset: Preset;
  onDismiss: () => void;
}) {
  const parts: string[] = [];
  if (report.lighting_applied) parts.push("lighting applied");
  if (report.keymap_slots_changed > 0) parts.push(`${report.keymap_slots_changed} base-layer key${report.keymap_slots_changed === 1 ? "" : "s"} remapped`);
  if (report.fn_keymap_slots_changed > 0) parts.push(`${report.fn_keymap_slots_changed} Fn-layer key${report.fn_keymap_slots_changed === 1 ? "" : "s"} remapped`);
  if (report.automations_added > 0) parts.push(`${report.automations_added} automation${report.automations_added === 1 ? "" : "s"} added`);
  if (report.automations_skipped_existing > 0) parts.push(`${report.automations_skipped_existing} automation${report.automations_skipped_existing === 1 ? "" : "s"} skipped (already present)`);
  const summary = parts.length === 0 ? "Nothing applied (none of the components were selected)." : parts.join(", ") + ".";

  return (
    <div className="mb-4 flex items-start gap-3 rounded-md border border-good/40 bg-good/10 px-3 py-2">
      <span className="text-lg" aria-hidden>{preset.icon}</span>
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-fg-0">Applied: {preset.name}</p>
        <p className="mt-0.5 text-xs text-fg-2">{summary}</p>
      </div>
      <button onClick={onDismiss} className="shrink-0 text-fg-3 hover:text-fg-0" aria-label="Dismiss">
        ✕
      </button>
    </div>
  );
}
