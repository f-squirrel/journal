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
  return date.toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
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

/// Most-recent entry date first: the order `Vault::list_entries` returns, and
/// the order `EntryList` renders. Deliberately not `updated_at` — the list is
/// the journal's chronology, so fixing a typo in an old entry must not move it.
///
/// Parsed rather than compared as text. `Header.entry_date` is a chrono
/// `DateTime<Utc>` serialized with `SecondsFormat::AutoSi`, so a whole-second
/// timestamp emits `...:00Z` and a sub-second one `...:00.123Z` — and `"."`
/// sorts before `"Z"`, making the later instant compare *less*. It is a
/// one-in-a-thousand collision, which is what makes it worth avoiding: it
/// would present as one entry stuck in the wrong place and never reproduce.
export function byEntryDate(a: EntryHeader, b: EntryHeader): number {
  return Date.parse(b.entry_date) - Date.parse(a.entry_date);
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
    const date = new Date(entry.entry_date);
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

/// How long ago, for a tooltip: "just now", "5 min ago", "3 h ago", "2 d ago".
/// `now` is a parameter so the caller decides when it re-reads the clock.
export function formatAgo(timestamp: string, now: number): string {
  const then = Date.parse(timestamp);
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.round(hours / 24)} d ago`;
}
