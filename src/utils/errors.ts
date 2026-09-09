/// Renders a rejection as user-facing text.
///
/// Tauri rejects with the Rust command's `Err(String)` verbatim, and the
/// browser fallback mirrors that, so a plain string is the common case.
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
