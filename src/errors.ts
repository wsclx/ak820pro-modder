/**
 * Format anything that lands in a `catch` block into a readable string.
 *
 * **Why this exists**: our Rust `AppError` is `#[derive(Serialize)]` with
 * `#[serde(tag = "kind", content = "message")]`, so Tauri rejects with an
 * object shaped `{ kind: "Protocol", message: "device not found" }`.
 * `String({...})` on that object yields the infamous `"[object Object]"` —
 * which is exactly what the System, Macros, and Keymap views were showing.
 *
 * Order of resolution:
 *   1. Already a string → return as-is.
 *   2. Has a `message` field (our `AppError` shape, or a native `Error`) →
 *      prepend the `kind` (if any) so the user sees both layer and detail.
 *   3. Has a `kind` field but no message → use the kind.
 *   4. Fallback → `JSON.stringify`. Never `String(obj)`.
 */
export function formatError(e: unknown): string {
  if (e == null) return "Unknown error";
  if (typeof e === "string") return e;

  if (typeof e === "object") {
    const obj = e as Record<string, unknown>;
    const kind = typeof obj.kind === "string" ? obj.kind : undefined;
    const message = typeof obj.message === "string" ? obj.message : undefined;

    if (message) {
      // Hide the "Protocol" kind since it's the only one and adds no info.
      return kind && kind !== "Protocol" ? `${kind}: ${message}` : message;
    }
    if (kind) return kind;

    try {
      return JSON.stringify(e);
    } catch {
      return "Unknown error";
    }
  }

  return String(e);
}
