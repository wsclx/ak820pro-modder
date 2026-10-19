import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, ErrorBanner } from "../components/ui";
import { PageHeader } from "../components/Layout";
import { hidName } from "../data/hid-usage-names";
import { formatError } from "../errors";
import { CODE_TO_HID } from "./Macros";

type Remap = { source: number; destination: number };
type Step = { code: string; down: boolean; delay_ms: number };
type HostMacro = { id: number; name: string; shortcut: string; steps: Step[] };
type Status = { accessibility: boolean; running: boolean };
const INPUT = "rounded border border-line bg-surface-base px-3 py-2 text-sm text-fg-0";
const USAGES = [...Array.from({ length: 112 }, (_, i) => i + 4), ...Array.from({ length: 8 }, (_, i) => i + 224)];

export function HostKeymap() {
  const [remaps, setRemaps] = useState<Remap[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  async function read() {
    setBusy(true); setError(null);
    try { setRemaps(await invoke<Remap[]>("host_remaps_get")); }
    catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }
  useEffect(() => { void read(); }, []);
  async function apply(restore: boolean) {
    setBusy(true); setError(null); setNotice("");
    try {
      setRemaps(await invoke<Remap[]>("host_remaps_apply", { remaps, restore }));
      setNotice(restore ? "Previous Mac mappings restored." : "Mappings applied and read back from macOS.");
    } catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }
  return <div className="space-y-5">
    <PageHeader title="Key remapping on this Mac" description="For the USB AK820 Pro (0C45:800A). Changes affect this keyboard on this Mac." />
    <p className="text-sm text-fg-2">macOS forgets these mappings after unplugging or restarting. Reapply here when needed. Fn combinations and the knob are handled inside the keyboard and cannot be remapped through this Mac fallback. Key names are physical HID positions; typed characters follow your Mac input language.</p>
    {error && <ErrorBanner>{error}</ErrorBanner>}
    {notice && <p role="status" className="text-sm text-good">{notice}</p>}
    <Card title="USB key mappings">
      <div className="space-y-3">
        {remaps.map((r, i) => <div key={i} className="flex flex-wrap items-center gap-3">
          {(["source", "destination"] as const).map((field, j) => <label key={field} className="text-sm">{j === 0 ? "From " : "To "}<select aria-label={`${field} ${i + 1}`} className={INPUT} value={r[field]} disabled={busy} onChange={e => setRemaps(remaps.map((v, n) => n === i ? { ...v, [field]: Number(e.target.value) } : v))}>
            {USAGES.map(hid => <option key={hid} value={hid}>{hidName(hid)} ({hid})</option>)}
          </select></label>)}
          <Button disabled={busy} onClick={() => setRemaps(remaps.filter((_, n) => n !== i))}>Remove</Button>
        </div>)}
        {!remaps.length && <p className="text-sm text-fg-2">No Mac remapping for this keyboard.</p>}
        <div className="flex flex-wrap gap-2">
          <Button disabled={busy || remaps.length >= 128} onClick={() => setRemaps([...remaps, { source: USAGES.find(u => !remaps.some(r => r.source === u)) ?? 4, destination: 41 }])}>Add mapping</Button>
          <Button variant="primary" disabled={busy} onClick={() => void apply(false)}>Apply on this Mac</Button>
          <Button disabled={busy} onClick={() => void apply(true)}>Undo app changes</Button>
          <Button disabled={busy} onClick={() => void read()}>Read current</Button>
          <Button disabled={busy} onClick={() => { setError(null); void invoke<Remap[]>("host_remaps_saved").then(setRemaps).catch(e => setError(formatError(e))); }}>Load saved draft</Button>
        </div>
      </div>
    </Card>
  </div>;
}

export function HostMacros() {
  const [macros, setMacros] = useState<HostMacro[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [recording, setRecording] = useState(false);
  const [steps, setSteps] = useState<Step[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [pending, setBusy] = useState(false);
  const busy = pending || status?.running === true;
  const [dirty, setDirty] = useState(false);
  const held = useRef(new Set<string>());
  const last = useRef(0);
  const recorder = useRef<HTMLDivElement>(null);
  const macro = macros.find(m => m.id === selected);
  const update = (value: Partial<HostMacro>) => { setMacros(ms => ms.map(m => m.id === selected ? { ...m, ...value } : m)); setDirty(true); };
  useEffect(() => {
    void invoke<{ macros: HostMacro[] }>("host_macros_load").then(c => { setMacros(c.macros); setSelected(c.macros[0]?.id ?? null); }).catch(e => setError(formatError(e)));
    const tick = () => { void invoke<Status>("host_keyboard_status").then(setStatus).catch(() => {}); };
    tick(); const interval = window.setInterval(tick, 1500); return () => window.clearInterval(interval);
  }, []);
  function stopRecording() {
    if (!recording) return;
    const releases = [...held.current].map(code => ({ code, down: false, delay_ms: 0 }));
    held.current.clear();
    const complete = [...steps, ...releases];
    setSteps(complete); update({ steps: complete }); setRecording(false);
    setNotice("Recording captured. Review and save before running.");
  }
  async function save() {
    setError(null); setBusy(true); setNotice("");
    try { await invoke("host_macros_save", { config: { macros } }); setDirty(false); setNotice("Saved on this Mac. Shortcuts are active while the app is open."); }
    catch (e) { setError(formatError(e)); }
    finally { setBusy(false); }
  }
  async function run() {
    if (!macro) return;
    setError(null); setNotice("Starts in 3 seconds. Switch to the destination window; Stop cancels playback."); setBusy(true);
    try { await invoke("host_macro_run", { id: macro.id }); setNotice("Macro finished."); }
    catch (e) { const message = formatError(e); setNotice(message === "Macro stopped" ? "Macro stopped." : ""); setError(message === "Macro stopped" ? null : message); }
    finally { setBusy(false); }
  }
  return <div className="space-y-5">
    <PageHeader title="Keyboard macros on this Mac" description="Record key presses, releases and delays. Stored in the app; runs while the app is open." />
    <p className="text-sm text-fg-2">Playback types into the foreground app using your Mac input language. Shortcuts work from any keyboard on this Mac. Use a shortcut such as Control+Alt+M, or leave it blank and use Run. Release the shortcut keys before playback. Fn, mouse actions and onboard macro storage are unavailable in this fallback.</p>
    <p className="text-sm text-fg-2">Accessibility: {status?.accessibility ? "allowed" : "required for playback — enable AK820 Pro Modder in System Settings → Privacy & Security → Accessibility"}. After enabling Accessibility, quit and reopen the app once. Recording listens only inside the box below.</p>
    {status?.running && <p role="status" className="text-sm text-fg-1">Macro playback is running. Stop playback cancels it.</p>}
    {error && <ErrorBanner>{error}</ErrorBanner>}
    {notice && <p role="status" className="text-sm text-fg-1">{notice}</p>}
    <Card title="Mac macro library">
      <div className="flex flex-wrap gap-2 mb-4">
        <select aria-label="Mac macro" className={INPUT} value={selected ?? ""} disabled={busy || recording} onChange={e => setSelected(Number(e.target.value))}>
          <option value="" disabled>Select a macro</option>{macros.map(m => <option key={m.id} value={m.id}>{m.name}</option>)}
        </select>
        <Button disabled={busy || recording || macros.length >= 32} onClick={() => { const id = Date.now(); setMacros([...macros, { id, name: `Macro ${macros.length + 1}`, shortcut: "", steps: [] }]); setSelected(id); setDirty(true); }}>New</Button>
        <Button disabled={busy || recording || !macro} onClick={() => { setMacros(macros.filter(m => m.id !== selected)); setSelected(null); setDirty(true); }}>Delete</Button>
        <Button variant="primary" disabled={busy || recording || !dirty} onClick={() => void save()}>Save library</Button>
      </div>
      {macro && <div className="space-y-3">
        <label className="block text-sm">Name <input type="text" className={INPUT} disabled={busy || recording} value={macro.name} onChange={e => update({ name: e.target.value })} /></label>
        <label className="block text-sm">Mac shortcut <input type="text" className={INPUT} disabled={busy || recording} placeholder="Control+Alt+M" value={macro.shortcut} onChange={e => update({ shortcut: e.target.value })} /></label>
        <div ref={recorder} tabIndex={0} role="textbox" aria-label="Macro recording area" className="rounded border border-line p-5 outline-none focus:border-accent-400" onBlur={stopRecording} onKeyDown={e => {
          if (!recording) return; e.preventDefault(); e.stopPropagation();
          if (e.repeat || held.current.has(e.code)) return;
          if (!CODE_TO_HID[e.code] || ["CapsLock", "PrintScreen", "ScrollLock", "Pause", "Insert"].includes(e.code)) { setError(`Unsupported key: ${e.code}`); return; }
          if (steps.length >= 240) { stopRecording(); return; }
          const now = performance.now(); const step = { code: e.code, down: true, delay_ms: Math.min(10000, Math.round(now - last.current)) }; last.current = now; held.current.add(e.code); setSteps(s => [...s, step]);
        }} onKeyUp={e => {
          if (!recording) return; e.preventDefault(); e.stopPropagation(); if (!held.current.delete(e.code)) return;
          const now = performance.now(); const step = { code: e.code, down: false, delay_ms: Math.min(10000, Math.round(now - last.current)) }; last.current = now; setSteps(s => [...s, step]);
        }}>{recording ? `Recording ${steps.length} events — click outside to stop.` : "Click Record, then type here. Click outside to stop."}</div>
        <div className="flex flex-wrap gap-2">
          <Button disabled={busy || recording} onClick={() => { setSteps([]); held.current.clear(); last.current = performance.now(); setError(null); setRecording(true); recorder.current?.focus(); }}>Record</Button>
          <Button disabled={busy || recording || dirty || !macro.steps.length || !status?.accessibility} onClick={() => void run()}>Run in 3 seconds</Button>
          <Button variant="danger" onClick={() => { void invoke("host_macro_stop"); }}>Stop playback</Button>
        </div>
        <label className="block text-sm">Playback test area (optional)<textarea aria-label="Macro playback test area" className={`${INPUT} mt-2 block w-full`} placeholder="Click here during the countdown to test a macro inside this app." /></label>
        <details><summary className="cursor-pointer text-sm">Review {macro.steps.length} events</summary><pre className="mt-2 max-h-64 overflow-auto text-xs">{macro.steps.map(s => `${s.delay_ms} ms  ${s.code} ${s.down ? "down" : "up"}`).join("\n")}</pre></details>
      </div>}
    </Card>
  </div>;
}
