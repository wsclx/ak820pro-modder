/**
 * Theme handling — dark (default) vs light.
 *
 * Source of truth is `<html data-theme="dark|light">`. The CSS variables
 * in `src/index.css` cascade off that attribute; every Tailwind utility
 * built on top of them re-themes automatically.
 *
 * Resolution order at boot:
 *   1. Explicit `localStorage["ak820:theme"]` (`"dark"` | `"light"`)
 *   2. `prefers-color-scheme: light` system pref
 *   3. Fallback to dark
 *
 * If the user toggles the theme in-app, we write to localStorage so
 * the choice is sticky across launches but follow-the-system stays the
 * default for first-launch users.
 */

export type Theme = "dark" | "light";

const STORAGE_KEY = "ak820:theme";

function systemPreference(): Theme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function read(): Theme {
  if (typeof window === "undefined") return "dark";
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (stored === "dark" || stored === "light") return stored;
  return systemPreference();
}

function write(theme: Theme) {
  document.documentElement.setAttribute("data-theme", theme);
}

/**
 * Read the active theme straight off the DOM. Use this in components
 * via `useTheme()` rather than calling here directly.
 */
export function getTheme(): Theme {
  const attr = document.documentElement.getAttribute("data-theme");
  return attr === "light" ? "light" : "dark";
}

/**
 * Set + persist + apply a theme. Triggers a `themechange` custom event
 * so components subscribed via `useTheme()` re-render.
 */
export function setTheme(theme: Theme) {
  window.localStorage.setItem(STORAGE_KEY, theme);
  write(theme);
  window.dispatchEvent(new CustomEvent<Theme>("themechange", { detail: theme }));
}

export function toggleTheme() {
  setTheme(getTheme() === "dark" ? "light" : "dark");
}

/**
 * Apply the initial theme on app boot. Call this synchronously from
 * `main.tsx` before React mounts so first paint already has the right
 * colours (avoids the "dark flash on a light system" or vice versa).
 */
export function bootstrapTheme() {
  write(read());
}
