# ak820pro — Projekt-Hinweise für Claude

Bewusst **schlank** gehalten. Globales `~/.claude/CLAUDE.md` (~26 K Token SuperClaude-Framework) wird hier **nicht** importiert, damit Sub-Agent-Spawns nicht in den "Prompt is too long"-Hang laufen.

## Einstieg

Bevor du an diesem Repo arbeitest, lies einmal:

- **[../docs/HANDOFF.md](../docs/HANDOFF.md)** — Vision, Hardware-Facts, Wire-Protokoll, Repo-Layout, Phasen-Status, Foot-Guns, IPC-Command-Map, Backlog.

## MCPs

Nur **`playwright`** ist für dieses Projekt aktiv (siehe `settings.json`). Begründung: RE der AJAZZ online driver bundles via Browser-Automation. Alles andere (Notion, Gmail, Cloudflare, …) bringt für AK820-Arbeit nichts und bläht jeden Subagent-Prompt auf.

## Dev-Workflow

- App starten: `pnpm tauri dev` (nutzt `frontendDist: "../dist"` + `pnpm build` als `beforeDevCommand` — **niemals** auf `devUrl` umstellen, das hängt WKWebView zuverlässig).
- CLI testen: `cargo run -p ak820-cli -- probe` etc.
- Reload in der App: ⌘+R funktioniert nur dank des nativen `View → Reload`-Menüeintrags in `src-tauri/src/lib.rs`.

## Wenn du SuperClaude doch mal brauchst

Statt es global zu laden, einmalig im Prompt anfordern oder per `@~/.claude/PERSONAS.md` o.ä. selektiv ziehen.
