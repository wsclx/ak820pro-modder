import type { PropsWithChildren, ReactNode } from "react";

// ---------------------------------------------------------------------------
// Card — elevated surface with rim-light, used for every block of content.

export function Card({
  children,
  className = "",
  title,
  action,
  kicker,
}: PropsWithChildren<{
  className?: string;
  title?: ReactNode;
  action?: ReactNode;
  kicker?: ReactNode;
}>) {
  const hasHeader = title || action || kicker;
  return (
    <section
      className={
        "relative rounded-lg border border-line bg-surface-elevated/85 shadow-card " +
        className
      }
    >
      {hasHeader && (
        <header className="flex items-baseline justify-between gap-4 px-5 pt-4 pb-3">
          <div>
            {kicker && <p className="kicker mb-1">{kicker}</p>}
            {title && (
              <h2 className="text-base font-medium tracking-tight text-fg-0">{title}</h2>
            )}
          </div>
          {action && <div className="shrink-0">{action}</div>}
        </header>
      )}
      <div className={(hasHeader ? "px-5 pb-5" : "p-5")}>{children}</div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Button

type BtnVariant = "primary" | "ghost" | "ghost-active" | "subtle" | "danger";

const BTN_BASE =
  "relative inline-flex items-center justify-center gap-2 font-medium select-none " +
  "transition-[transform,background-color,border-color,box-shadow,color] duration-150 ease-out " +
  "active:translate-y-px disabled:cursor-not-allowed disabled:opacity-45 disabled:active:translate-y-0";

const BTN_VARIANTS: Record<BtnVariant, string> = {
  primary:
    "bg-accent-500 text-white border border-accent-400 hover:bg-accent-400 hover:border-accent-300 " +
    "shadow-[0_1px_0_rgba(255,255,255,0.08)_inset,0_8px_24px_-8px_rgba(124,92,255,0.55)] " +
    "active:shadow-press",
  ghost:
    "border border-line bg-surface-elevated/40 text-fg-1 hover:text-fg-0 hover:bg-surface-raised hover:border-line-strong",
  "ghost-active":
    "border border-accent-500/60 bg-accent-glow text-fg-0 shadow-[0_0_0_1px_rgba(124,92,255,0.15)] hover:bg-accent-500/15",
  subtle:
    "text-fg-1 hover:text-fg-0 hover:bg-surface-raised",
  danger:
    "border border-bad/40 bg-bad-soft text-bad hover:bg-bad/20",
};

const BTN_SIZES = {
  sm: "h-7 px-2.5 rounded-sm text-xs",
  md: "h-9 px-3.5 rounded-[8px] text-sm",
  lg: "h-10 px-4 rounded-[8px] text-sm",
};

export function Button({
  variant = "ghost",
  size = "md",
  className = "",
  type = "button",
  children,
  ...rest
}: PropsWithChildren<{
  variant?: BtnVariant;
  size?: keyof typeof BTN_SIZES;
  className?: string;
  type?: "button" | "submit";
  onClick?: () => void;
  disabled?: boolean;
  title?: string;
}>) {
  return (
    <button
      type={type}
      className={`${BTN_BASE} ${BTN_VARIANTS[variant]} ${BTN_SIZES[size]} ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}

// ---------------------------------------------------------------------------
// KVList — definition list with consistent rhythm

export function KVList({
  rows,
}: {
  rows: { label: string; value: ReactNode; mono?: boolean }[];
}) {
  return (
    <dl className="grid grid-cols-[max-content_1fr] items-center gap-x-6 gap-y-3 text-sm">
      {rows.map((r) => (
        <Row key={r.label} {...r} />
      ))}
    </dl>
  );
}

function Row({ label, value, mono }: { label: string; value: ReactNode; mono?: boolean }) {
  return (
    <>
      <dt className="text-fg-2">{label}</dt>
      <dd className={"m-0 text-fg-0 " + (mono ? "font-mono tabular text-[13px]" : "")}>
        {value}
      </dd>
    </>
  );
}

// ---------------------------------------------------------------------------
// Badge

type BadgeTone = "neutral" | "good" | "warn" | "bad" | "accent";

const BADGE_TONES: Record<BadgeTone, string> = {
  neutral: "bg-surface-raised text-fg-1 border-line",
  good: "bg-good-soft text-good border-good/40",
  warn: "bg-warn-soft text-warn border-warn/40",
  bad: "bg-bad-soft text-bad border-bad/40",
  accent: "bg-accent-glow text-accent-200 border-accent-500/35",
};

export function Badge({
  tone = "neutral",
  children,
  className = "",
}: PropsWithChildren<{ tone?: BadgeTone; className?: string }>) {
  return (
    <span
      className={
        `inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-2xs font-medium ${BADGE_TONES[tone]} ${className}`
      }
    >
      {children}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Toggle (switch + label)

export function Toggle({
  checked,
  onChange,
  children,
}: PropsWithChildren<{ checked: boolean; onChange: (v: boolean) => void }>) {
  return (
    <label className="inline-flex cursor-pointer select-none items-center gap-2.5 text-sm text-fg-1 hover:text-fg-0">
      <span
        className={[
          "relative inline-flex h-5 w-9 items-center rounded-full border transition-colors duration-150",
          checked ? "border-accent-500/60 bg-accent-500" : "border-line bg-surface-raised",
        ].join(" ")}
      >
        <span
          className={[
            "block h-3.5 w-3.5 transform rounded-full bg-white shadow-[0_2px_4px_rgba(0,0,0,0.35)] transition-transform duration-150 ease-spring",
            checked ? "translate-x-[18px]" : "translate-x-0.5",
          ].join(" ")}
        />
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          className="absolute inset-0 cursor-pointer opacity-0"
        />
      </span>
      <span>{children}</span>
    </label>
  );
}

// ---------------------------------------------------------------------------
// Slider with value readout — uses CSS var for the rail fill

export function Slider({
  label,
  value,
  min = 0,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min?: number;
  max: number;
  onChange: (v: number) => void;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <span className="kicker">{label}</span>
        <span className="font-mono tabular text-sm text-fg-0">
          {value} <span className="text-fg-3">/ {max}</span>
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={1}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full"
        style={{ ["--pct" as never]: pct + "%" }}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Error banner

export function ErrorBanner({ children }: PropsWithChildren) {
  if (!children) return null;
  return (
    <div className="mb-5 flex items-start gap-3 rounded-md border border-bad/40 bg-bad-soft px-4 py-3 text-sm text-bad">
      <span className="mt-0.5 h-2 w-2 shrink-0 rounded-full bg-bad" />
      <span className="font-mono tabular leading-relaxed">{children}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Battery indicator — sleek capsule with state-aware fill

export function BatteryBar({
  level,
  charging,
  showLabel = true,
  compact = false,
}: {
  level: number;
  charging?: boolean;
  showLabel?: boolean;
  compact?: boolean;
}) {
  const pct = Math.max(0, Math.min(100, level));
  const tone = charging
    ? "from-accent-400 to-accent-500"
    : pct < 20
      ? "from-bad to-bad"
      : pct < 50
        ? "from-warn to-warn"
        : "from-good to-good";
  return (
    <div className={"flex items-center " + (compact ? "gap-2" : "gap-2.5")}>
      <div
        className={
          "relative overflow-hidden rounded-full border border-line bg-surface-base " +
          (compact ? "h-1.5 w-20" : "h-2 w-28")
        }
      >
        <div
          className={`h-full bg-gradient-to-r ${tone} transition-all duration-300 ease-out`}
          style={{ width: `${pct}%` }}
        />
      </div>
      {showLabel && (
        <span className="font-mono tabular text-2xs text-fg-1">
          {pct}%
          {charging && <span className="ml-1 text-accent-300">⚡</span>}
        </span>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Mono / Hex helpers

export function Mono({ children, className = "" }: PropsWithChildren<{ className?: string }>) {
  return <span className={`font-mono tabular ${className}`}>{children}</span>;
}

export function hex4(n: number) {
  return "0x" + n.toString(16).padStart(4, "0");
}

/**
 * Format an integer with a narrow no-break space as thousands separator —
 * locale-agnostic, no risk of dotted decimals on a de_DE system.
 */
export function formatInt(n: number): string {
  return n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, " ");
}

/** Pretty-print the keyboard's reported product name. */
export function prettyProduct(name: string | null | undefined): string {
  if (!name) return "Unknown device";
  // The device reports just "AK820" even for the Pro model.
  if (name.trim() === "AK820") return "AK820 Pro";
  return name;
}

// ---------------------------------------------------------------------------
// Color swatch (the spec card on the lighting screen)

export function ColorSwatch({ hex }: { hex: string }) {
  return (
    <span
      className="inline-block h-4 w-4 rounded-sm border border-line"
      style={{ background: "#" + hex }}
    />
  );
}
