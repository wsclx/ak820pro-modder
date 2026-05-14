/**
 * Active-layout state hook + persistence.
 *
 * The selected `LayoutId` is the source of truth for every view that
 * renders the on-screen keyboard surface (Keymap, CustomLightingPaint,
 * the macro recorder's key picker, …). We persist it to localStorage
 * so a contributor doesn't have to re-select their layout on every
 * launch, and broadcast a custom event on change so multiple mounted
 * consumers all rerender in lockstep when the user flips the picker
 * in the sidebar.
 */
import { useEffect, useState } from "react";
import { DEFAULT_LAYOUT_ID, LAYOUTS, resolveLayout } from "./index";
import type { KeyboardLayout, LayoutId } from "./types";

const STORAGE_KEY = "ak820:layout";
const CHANGE_EVENT = "ak820-layout-change";

function readStoredLayout(): LayoutId {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw && raw in LAYOUTS) return raw as LayoutId;
  } catch {
    /* private-browsing — fall through to default */
  }
  return DEFAULT_LAYOUT_ID;
}

/**
 * React hook returning the currently-selected layout + a setter that
 * persists + broadcasts the change to other mounted instances of the
 * hook.
 */
export function useLayout(): {
  layoutId: LayoutId;
  layout: KeyboardLayout;
  setLayoutId: (id: LayoutId) => void;
} {
  const [layoutId, setLayoutIdState] = useState<LayoutId>(readStoredLayout);

  useEffect(() => {
    const sync = () => setLayoutIdState(readStoredLayout());
    window.addEventListener(CHANGE_EVENT, sync);
    // Cross-tab sync via `storage` event for completeness — the app
    // is single-window today but this is cheap to wire.
    window.addEventListener("storage", (e) => {
      if (e.key === STORAGE_KEY) sync();
    });
    return () => {
      window.removeEventListener(CHANGE_EVENT, sync);
    };
  }, []);

  const setLayoutId = (id: LayoutId) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, id);
    } catch {
      /* private-browsing — ignore */
    }
    setLayoutIdState(id);
    window.dispatchEvent(new CustomEvent(CHANGE_EVENT));
  };

  return { layoutId, layout: resolveLayout(layoutId), setLayoutId };
}
