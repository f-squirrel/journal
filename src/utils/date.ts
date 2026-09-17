import type { EntryHeader } from "../types";

export function formatUpdatedAt(updatedAt: string): string {
  const date = new Date(updatedAt);
  if (Number.isNaN(date.getTime())) return updatedAt;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/// The entry pane's heading: "Wednesday, September 17, 2026".
export function formatEntryHeading(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  return date.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

export function formatTime(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/// The numeral in a list row's date block.
export function formatDayNumber(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "–";
  return date.toLocaleDateString(undefined, { day: "numeric" });
}

/// The weekday under it, abbreviated: "Wed".
export function formatWeekday(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString(undefined, { weekday: "short" });
}

/// Most-recently-updated first: the order `Vault::list_entries` returns, and
/// the order `EntryList` renders.
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

export interface EntryMonth {
  /// Stable across renders and unique per month, so it can key a list section.
  key: string;
  label: string;
  entries: EntryHeader[];
}

/// Splits an already-sorted list into the month sections the list column shows
/// under its sticky headers. Entries whose timestamp will not parse are kept
/// rather than dropped — a row with a dash in its date block is a bug someone
/// can see, a silently missing entry is not.
export function groupByMonth(entries: EntryHeader[]): EntryMonth[] {
  const months: EntryMonth[] = [];
  for (const entry of entries) {
    const date = new Date(entry.updated_at);
    const valid = !Number.isNaN(date.getTime());
    const key = valid ? `${date.getFullYear()}-${date.getMonth()}` : "unknown";
    const label = valid
      ? date.toLocaleDateString(undefined, { month: "long", year: "numeric" })
      : "Unknown date";
    const last = months[months.length - 1];
    if (last && last.key === key) {
      last.entries.push(entry);
    } else {
      months.push({ key, label, entries: [entry] });
    }
  }
  return months;
}
