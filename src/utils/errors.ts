/// Renders a rejection as user-facing text.
///
/// Tauri rejects with the Rust command's `Err(String)` verbatim, and the
/// browser fallback mirrors that, so a plain string is the common case.
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

/// The exact text of `VaultError::Stale` (`crates/vault/src/lib.rs`), which
/// `save_entry` rejects with when the disk no longer holds the `updated_at`
/// the caller last saw. Pinned here rather than matched structurally: every
/// command rejects with a plain string (see `errorMessage`), so this is the
/// contract between the two sides.
export const STALE_ENTRY_MESSAGE =
  "this entry was changed on another device since it was opened";

/// Whether a rejection is the vault refusing a stale save, as opposed to any
/// other failure (a locked vault, a full disk, …), which the caller should
/// still show as an ordinary save error.
export function isStaleEntry(err: unknown): boolean {
  return errorMessage(err) === STALE_ENTRY_MESSAGE;
}
