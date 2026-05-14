/**
 * Now-Playing card — Phase 6 preview surface.
 *
 * Reads the host's currently-playing track via the `get_now_playing` IPC
 * (which shells out to `osascript -l JavaScript` on macOS and probes
 * Music.app + Spotify). Polls every 2 seconds while mounted.
 *
 * This is foundation work for streaming the track string to the
 * keyboard's TFT display once Phase 5b3 (TFT activation sequence) is
 * unblocked. For now the card just renders the data so we can validate
 * the read-path independently.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Card, Mono } from "./ui";
import type { NowPlaying } from "../types";

const POLL_MS = 2000;

export function NowPlayingCard() {
  const [np, setNp] = useState<NowPlaying | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const alive = useRef(true);

  const poll = useCallback(async () => {
    try {
      const r = await invoke<NowPlaying>("get_now_playing");
      if (alive.current) {
        setNp(r);
        setErr(null);
      }
    } catch (e) {
      if (alive.current) setErr(String(e));
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void poll();
    const id = window.setInterval(poll, POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [poll]);

  const isPlaying = np?.is_playing === true;
  const source = np?.source ?? "none";

  return (
    <Card
      kicker="Phase 6 preview"
      title="Now playing"
      action={
        isPlaying ? (
          <Badge tone="good">{source}</Badge>
        ) : (
          <Badge tone="neutral">idle</Badge>
        )
      }
    >
      {err && (
        <p className="mb-3 rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-fg-1">
          {err}
        </p>
      )}

      {np === null ? (
        <p className="text-sm text-fg-2">Probing…</p>
      ) : isPlaying ? (
        <div className="space-y-1">
          <p className="text-base font-medium text-fg-0 truncate" title={np.title ?? ""}>
            {np.title || "—"}
          </p>
          {np.artist && (
            <p className="text-sm text-fg-1 truncate" title={np.artist}>
              {np.artist}
            </p>
          )}
          {np.album && (
            <p className="text-xs text-fg-3 truncate" title={np.album}>
              {np.album}
            </p>
          )}
        </div>
      ) : (
        <p className="text-sm text-fg-3">
          Nothing playing. Start something in <Mono>Music</Mono> or{" "}
          <Mono>Spotify</Mono> to see it here.
        </p>
      )}

      <p className="mt-4 border-t border-line/60 pt-3 text-xs text-fg-3">
        Polled every {POLL_MS / 1000}s via <Mono>osascript</Mono>. Will stream to
        the keyboard's TFT display once the upload-activation sequence is
        unlocked (Phase 5b3). Currently macOS-only — Music.app + Spotify
        desktop. Browser-tab media support is on the roadmap.
      </p>
    </Card>
  );
}
