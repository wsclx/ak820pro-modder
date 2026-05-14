/**
 * TFT Display view (Phase 5c/5d).
 *
 * Three top-level controls:
 *
 * 1. **Preset picker** — 12 entries: 2 diagnostic patterns at the top
 *    (Quadrants, Border) so a contributor verifying a fresh build sees
 *    them first, then 10 decorative test colours / gradients / cycles.
 * 2. **Custom image upload** — pick a PNG / JPEG / GIF from disk; the
 *    backend decodes, resizes (Fill / Contain / Stretch), quantises to
 *    RGB565, and uploads via the same chunked-write path the presets
 *    use. GIFs become multi-frame animations up to the device's
 *    `tftMaxFrames` budget (≈ 30 frames).
 * 3. **Factory Default** — restore the firmware's boot-time animation
 *    (`SET_TFT_BUILT_IN_INDEX(0)`). Useful when an upload looks broken
 *    and you want the panel back to a known state.
 *
 * The TFT upload runs against a different HID interface (0xFF67) than
 * the rest of the app, so an upload-in-flight doesn't contend with
 * Lighting / Keymap / Macros work on the control endpoint.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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

type FitMode = "fill" | "contain" | "stretch";

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
  const [fit, setFit] = useState<FitMode>("fill");

  useEffect(() => {
    invoke<TftPresetInfo[]>("list_tft_presets")
      .then((list) => {
        setPresets(list);
        if (list.length > 0) setSelected(list[0].id);
      })
      .catch((e) => setErr(formatError(e)));
  }, []);

  async function applyPreset() {
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

  async function uploadImage() {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      const path = await openDialog({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "Image",
            extensions: ["png", "jpg", "jpeg", "gif"],
          },
        ],
      });
      if (typeof path !== "string") {
        // User cancelled.
        return;
      }
      await invoke("apply_tft_image", { path, fit });
      const name = path.split("/").pop() ?? path;
      setInfo(`Uploaded ${name} (fit: ${fit}). Watch the TFT.`);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  async function factoryDefault() {
    setBusy(true);
    setErr(null);
    setInfo(null);
    try {
      await invoke("tft_factory_default");
      setInfo("Restored firmware-default animation.");
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
        description={
          "0.85\" 128×128 IPS panel above the knob. Pick a preset, upload a still / GIF, or restore the factory animation."
        }
        action={
          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={() => void factoryDefault()} disabled={busy}>
              Factory Default
            </Button>
            {current && (
              <Button variant="primary" onClick={() => void applyPreset()} disabled={busy || !selected}>
                {busy ? "Working…" : `Apply · ${current.display_name}`}
              </Button>
            )}
          </div>
        }
      />

      <ErrorBanner>{err}</ErrorBanner>

      <div className="grid gap-6">
        <Card
          title={
            <span className="inline-flex items-center gap-2">
              <span>Custom image</span>
              <Badge tone="neutral">Beta</Badge>
            </span>
          }
        >
          <p className="text-sm text-fg-2">
            Pick a <code>.png</code>, <code>.jpg</code>, or <code>.gif</code> from disk —
            it's decoded, resized to 128 × 128, quantised to RGB565, and uploaded.
            GIFs become multi-frame animations (truncated to ≈ 30 frames; the
            firmware's per-upload budget).
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <Button variant="primary" onClick={() => void uploadImage()} disabled={busy}>
              {busy ? "Working…" : "Choose image…"}
            </Button>
            <span className="text-xs uppercase tracking-wider text-fg-3">Fit:</span>
            <div className="inline-flex overflow-hidden rounded-md border border-line">
              {(["fill", "contain", "stretch"] as FitMode[]).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  onClick={() => setFit(mode)}
                  disabled={busy}
                  className={[
                    "px-3 py-1.5 text-xs font-medium uppercase tracking-wider transition",
                    fit === mode
                      ? "bg-accent-500/30 text-fg-0"
                      : "bg-surface-raised text-fg-2 hover:bg-surface-elevated",
                  ].join(" ")}
                >
                  {mode}
                </button>
              ))}
            </div>
          </div>
          <p className="mt-3 text-xs text-fg-3">
            <strong>Fill</strong> = centre-crop to a square (edge-to-edge,
            matches the AJAZZ web tool default). <strong>Contain</strong> =
            letterbox with black bars to preserve the whole image.
            <strong> Stretch</strong> = scale axes independently (only useful
            if you've already cropped to a square).
          </p>
        </Card>

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
                12 generated animations: 2 diagnostic patterns first (Quadrants,
                Border) for verifying the display renders the full 128 × 128
                area, then 10 decorative test colours / gradients / cycles.
                Start with <span className="font-medium text-fg-1">Diagnostic
                · Quadrants</span> on a fresh build — if all 4 colour quadrants
                are visible, the protocol stack and dimensions are right.
              </p>
              <div className="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">
                {presets.map((p) => {
                  const isActive = p.id === selected;
                  const isDiagnostic = p.id.startsWith("diagnostic-");
                  return (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => setSelected(p.id)}
                      className={[
                        "flex flex-col items-start rounded-lg border px-3 py-2.5 text-left transition",
                        isActive
                          ? "border-accent-500/60 bg-accent-500/15 text-fg-0"
                          : isDiagnostic
                          ? "border-warn/40 bg-warn/5 text-fg-1 hover:border-warn/60"
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
          </Card>
        )}

        <Card title="Next iteration">
          <p className="text-sm text-fg-2">
            Live system-info presets (battery percent, volume, CPU, clock,
            etc.) need a polling + on-host text-rasterisation pipeline that
            isn't built yet. Tracked as Phase 5e — see the project HANDOFF
            for the scope and reasons. The current build ships the upload
            path so contributors can hand-craft these images in any editor
            and drop them in via <em>Custom image</em>.
          </p>
        </Card>
      </div>
    </>
  );
}
