/**
 * TFT Display view (Phase 5c).
 *
 * The AK820 Pro's 0.85" 128 × 128 TFT panel can play a per-device user
 * animation. v0.7.0-beta shipped the upload pipeline at the wire level
 * but a single-byte off-by-one in the chunk header meant the firmware
 * accepted uploads silently without ever switching the display from
 * its built-in animation. 0.8.x corrects the magic; the rest of this
 * view assumes the fix lands a visible result on the panel.
 *
 * What this view does **now**:
 * - Lists the 10 curated presets from the backend's `tft_presets` module.
 * - Lets the user pick one, see its frame count + total duration, and
 *   click `Apply` to upload + play it.
 *
 * What this view **doesn't do yet** (planned for 5d):
 * - Drag-and-drop GIF / PNG → frame extract → resize/dither → upload.
 * - Set the TFT to date/time or system-stats overlay (cmd 52).
 *
 * The TFT upload runs against a different HID interface (0xFF67) than
 * the rest of the app, so an upload-in-flight doesn't contend with
 * Lighting / Keymap / Macros commands on the control endpoint.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { formatError } from "../errors";

interface TftPresetInfo {
  id: string;
  display_name: string;
  description: string;
  frame_count: number;
  total_ms: number;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)} s`;
  const m = Math.floor(s / 60);
  return `${m}m ${Math.round(s - m * 60)}s`;
}

export function Tft() {
  const [presets, setPresets] = useState<TftPresetInfo[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  useEffect(() => {
    invoke<TftPresetInfo[]>("list_tft_presets")
      .then((list) => {
        setPresets(list);
        if (list.length > 0) setSelected(list[0].id);
      })
      .catch((e) => setErr(formatError(e)));
  }, []);

  async function apply() {
    if (!selected) return;
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      await invoke("apply_tft_preset", { id: selected });
      const p = presets?.find((x) => x.id === selected);
      setInfo(
        p
          ? `Uploaded ${p.display_name} (${p.frame_count} frame${p.frame_count === 1 ? "" : "s"}, ${formatDuration(p.total_ms)}). Watch the TFT.`
          : "Uploaded. Watch the TFT.",
      );
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  const current = presets?.find((p) => p.id === selected);

  return (
    <>
      <PageHeader
        title="TFT Display"
        description={"0.85\" 128×128 IPS panel above the knob. Pick a curated preset and apply — Phase 5c."}
        action={
          current && (
            <Button variant="primary" onClick={() => void apply()} disabled={busy || !selected}>
              {busy ? "Uploading…" : `Apply · ${current.display_name}`}
            </Button>
          )
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6">
        <Card
          title={
            <span className="inline-flex items-center gap-2">
              <span>Presets</span>
              <Badge tone="neutral">Beta</Badge>
            </span>
          }
        >
          {presets === null ? (
            <p className="text-sm text-fg-3">Loading presets…</p>
          ) : (
            <>
              <p className="text-sm text-fg-2">
                10 curated animations, generated programmatically (no embedded
                pixel data — the binary stays small, contributors can tweak a
                single function to redesign a preset). Start with{" "}
                <span className="font-medium text-fg-1">Magenta</span> or{" "}
                <span className="font-medium text-fg-1">Cyan</span> to verify
                your TFT accepts uploads — if it goes pink or cyan, the rest
                will too.
              </p>
              <div className="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">
                {presets.map((p) => {
                  const isActive = p.id === selected;
                  return (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => setSelected(p.id)}
                      className={[
                        "flex flex-col items-start rounded-lg border px-3 py-2.5 text-left transition",
                        isActive
                          ? "border-accent-500/60 bg-accent-500/15 text-fg-0"
                          : "border-line bg-surface-raised text-fg-1 hover:border-line-strong",
                      ].join(" ")}
                    >
                      <span className="flex w-full items-center justify-between">
                        <span className="font-medium">{p.display_name}</span>
                        <span className="text-[10px] uppercase tracking-wider text-fg-3">
                          {p.frame_count} fr · {formatDuration(p.total_ms)}
                        </span>
                      </span>
                      <span className="mt-1 text-xs text-fg-2">{p.description}</span>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </Card>

        {info && (
          <Card title="Status">
            <p className="text-sm text-fg-1">{info}</p>
            <p className="mt-2 text-xs text-fg-3">
              The upload reaches the firmware on every AK820 Pro variant, but
              the actual display switch from the built-in animation to user
              content depends on the chunk-header magic the firmware
              validates against. 0.8.x corrected an off-by-one in that
              constant compared to 0.7.0-beta — if you're upgrading, this
              is the first build where the display should actually flip.
            </p>
          </Card>
        )}
      </div>
    </>
  );
}
