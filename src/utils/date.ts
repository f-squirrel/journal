import type { EntryHeader } from "../types";

export function formatUpdatedAt(updatedAt: string): string {
  const date = new Date(updatedAt);
  if (Number.isNaN(date.getTime())) return updatedAt;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/// Most-recently-updated first: the order `Vault::list_entries` returns, and
/// the order `EntryListPage` renders.
///
/// Parsed rather than compared as text. `Header.updated_at` is a chrono
/// `DateTime<Utc>` serialized with `SecondsFormat::AutoSi`, so a whole-second
/// timestamp emits `...:00Z` and a sub-second one `...:00.123Z` — and `"."`
/// sorts before `"Z"`, making the later instant compare *less*. It is a
/// one-in-a-thousand collision, which is what makes it worth avoiding: it
/// would present as one entry stuck in the wrong place and never reproduce.
export function byUpdatedAt(a: EntryHeader, b: EntryHeader): number {
  return Date.parse(b.updated_at) - Date.parse(a.updated_at);
}
