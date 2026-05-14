import type { PropsWithChildren, ReactNode } from "react";
import { BatteryBar, prettyProduct } from "./ui";
import { APP_CREDIT, APP_HOMEPAGE } from "../version";
import { resolveLayout, DEFAULT_LAYOUT_ID } from "../data/layouts";

export interface NavItem<T extends string> {
  id: T;
  label: string;
  icon: ReactNode;
  comingSoon?: boolean;
}

export function Layout<T extends string>({
  brand = "AK820 Pro",
  phaseLabel,
  nav,
  active,
  onSelect,
  connection,
  battery,
  onReconnect,
  wide,
  children,
}: PropsWithChildren<{
  brand?: string;
  phaseLabel?: string;
  nav: readonly NavItem<T>[];
  active: T;
  onSelect: (id: T) => void;
  connection?: { connected: boolean; product?: string | null };
  battery?: { level: number; charging: boolean };
  onReconnect?: () => void;
  /** Lets a view (e.g. Keymap) opt out of the standard ≤960 px content cap. */
  wide?: boolean;
}>) {
  return (
    <div className="flex h-screen w-screen overflow-hidden">
      {/* Sidebar */}
      <aside className="flex w-60 shrink-0 flex-col border-r border-line bg-surface-surface/80">
        {/* Brand */}
        <div className="px-5 pt-5 pb-4">
          <div className="flex items-center gap-2.5">
            <Logo />
            <div className="leading-none">
              <div className="text-sm font-semibold tracking-tight text-fg-0">{brand}</div>
              {phaseLabel && <div className="mt-1 kicker">{phaseLabel}</div>}
            </div>
          </div>
        </div>

        {/* Nav */}
        <nav className="flex-1 px-2 pb-3">
          <ul className="space-y-px">
            {nav.map((item) => {
              const isActive = item.id === active;
              return (
                <li key={item.id}>
                  <button
                    onClick={() => !item.comingSoon && onSelect(item.id)}
                    disabled={item.comingSoon}
                    className={[
                      "group relative flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm transition-all duration-150 ease-out",
                      isActive
                        ? "bg-surface-raised text-fg-0"
                        : "text-fg-2 hover:bg-surface-elevated/60 hover:text-fg-0",
                      item.comingSoon ? "cursor-not-allowed opacity-40 hover:bg-transparent hover:text-fg-2" : "",
                    ].join(" ")}
                  >
                    {/* Active rail */}
                    {isActive && (
                      <span className="absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-r-full bg-accent-500" />
                    )}
                    <span
                      className={[
                        "flex h-4 w-4 items-center justify-center",
                        isActive ? "text-accent-300" : "text-fg-3 group-hover:text-fg-1",
                      ].join(" ")}
                    >
                      {item.icon}
                    </span>
                    <span className="flex-1">{item.label}</span>
                    {item.comingSoon && (
                      <span className="rounded-sm bg-surface-base px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-fg-3">
                        soon
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        {/* Footer status */}
        <footer className="border-t border-line px-4 py-3.5">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <StatusDot connected={!!connection?.connected} />
              <span className="truncate text-xs text-fg-1">
                {connection?.connected
                  ? prettyProduct(connection.product)
                  : "Disconnected"}
              </span>
            </div>
            {!connection?.connected && onReconnect && (
              <button
                onClick={onReconnect}
                className="rounded-sm border border-line bg-surface-elevated/40 px-1.5 py-0.5 text-2xs font-medium text-fg-1 transition hover:border-accent-500/60 hover:bg-accent-glow hover:text-fg-0"
              >
                Reconnect
              </button>
            )}
          </div>
          {connection?.connected && battery && (
            <BatteryBar level={battery.level} charging={battery.charging} compact />
          )}
          <div className="mt-3 flex items-center justify-between gap-2 border-t border-line/60 pt-2.5">
            <a
              href={APP_HOMEPAGE}
              target="_blank"
              rel="noopener noreferrer"
              className="text-[10px] leading-tight text-fg-3 transition-colors hover:text-fg-1"
              title="Open the project on GitHub"
            >
              {APP_CREDIT}
            </a>
            <span
              className="rounded-sm border border-line bg-surface-base px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wider text-fg-3"
              title={`Active keyboard layout. v0.5.0-beta is built for ${resolveLayout(DEFAULT_LAYOUT_ID).displayName} only — other variants are on the roadmap.`}
            >
              {resolveLayout(DEFAULT_LAYOUT_ID).displayName}
            </span>
          </div>
        </footer>
      </aside>

      {/* Main */}
      <main className="relative flex-1 overflow-y-auto">
        <div
          className={[
            "mx-auto px-10 pb-16 pt-10",
            wide ? "max-w-none" : "max-w-[960px]",
          ].join(" ")}
        >
          {children}
        </div>
      </main>
    </div>
  );
}

// ----- page header (in-content title) -------------------------------------

export function PageHeader({
  title,
  description,
  action,
  kicker,
}: {
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  kicker?: string;
}) {
  return (
    <header className="mb-8 flex items-end justify-between gap-6">
      <div>
        {kicker && <p className="kicker mb-2">{kicker}</p>}
        <h1 className="text-3xl font-semibold tracking-tight text-fg-0">{title}</h1>
        {description && (
          <p className="mt-2 max-w-prose text-sm text-fg-2">{description}</p>
        )}
      </div>
      {action && <div className="shrink-0 pb-1">{action}</div>}
    </header>
  );
}

// ----- decoration ----------------------------------------------------------

function Logo() {
  return (
    <span className="relative flex h-8 w-8 items-center justify-center rounded-md bg-gradient-to-br from-accent-500 to-accent-700 shadow-[0_4px_12px_-2px_rgba(124,92,255,0.5)]">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden>
        <path
          d="M5 8h14v8H5z M3 12h2 M19 12h2 M9 6v-2 M15 6v-2 M9 20v-2 M15 20v-2"
          stroke="white"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <rect x="7.5" y="10.5" width="3" height="3" fill="white" />
      </svg>
    </span>
  );
}

function StatusDot({ connected }: { connected: boolean }) {
  return (
    <span className="relative flex h-2 w-2 items-center justify-center">
      <span
        className={[
          "absolute inset-0 rounded-full",
          connected ? "bg-good shadow-[0_0_10px_rgba(61,213,137,0.7)]" : "bg-fg-4",
        ].join(" ")}
      />
      {connected && (
        <span className="absolute inset-0 animate-ping rounded-full bg-good opacity-25" />
      )}
    </span>
  );
}

// ----- icons (Lucide re-export so views import from one place) ------------

export {
  Cable as Plug,
  Sun as Bulb,
  Settings,
  Keyboard,
  Zap as Macro,
  Monitor as Screen,
  RefreshCw,
  Check,
  AlertCircle,
} from "lucide-react";
