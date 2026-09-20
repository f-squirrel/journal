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

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/// The local calendar day of a `Date`, as "YYYY-MM-DD" — what the calendar
/// grid hands back, keyed the same way as `dayKey` so the two meet.
export function dayKeyOf(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/// The local calendar day of a timestamp, or null if it will not parse. Local,
/// like the list's month sections and the pane's heading: the day an entry is
/// *about* is the one on the reader's wall clock.
export function dayKey(timestamp: string): string | null {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return null;
  return dayKeyOf(date);
}

/// Every day that has entries, each list in the input order (newest first, as
/// the context holds them).
///
/// An entry whose timestamp will not parse is left out — the opposite of
/// `groupByMonth`, which keeps it. A list can show an "Unknown date" row; a
/// month grid has no cell to put one in, and inventing a day would be worse
/// than the entry being absent from a view that is not the only way to it.
export function entriesByDay(
  entries: EntryHeader[],
): Map<string, EntryHeader[]> {
  const days = new Map<string, EntryHeader[]>();
  for (const entry of entries) {
    const key = dayKey(entry.entry_date);
    if (key === null) continue;
    const day = days.get(key);
    if (day) {
      day.push(entry);
    } else {
      days.set(key, [entry]);
    }
  }
  return days;
}

export interface OnThisDayGroup {
  year: number;
  /// Always at least 1: this year's own entries are in the list, not memories.
  yearsAgo: number;
  entries: EntryHeader[];
}

/// The entries written on `now`'s local month and day in earlier years,
/// grouped by year, nearest year first. Future-dated entries and unparseable
/// ones are skipped. It is an exact month-and-day match, so a Feb 29 entry
/// only resurfaces on Feb 29.
export function onThisDay(entries: EntryHeader[], now: Date): OnThisDayGroup[] {
  const groups = new Map<number, EntryHeader[]>();
  for (const entry of entries) {
    const date = new Date(entry.entry_date);
    if (Number.isNaN(date.getTime())) continue;
    if (date.getMonth() !== now.getMonth()) continue;
    if (date.getDate() !== now.getDate()) continue;
    if (date.getFullYear() >= now.getFullYear()) continue;
    const year = date.getFullYear();
    const group = groups.get(year);
    if (group) {
      group.push(entry);
    } else {
      groups.set(year, [entry]);
    }
  }
  return [...groups.entries()]
    .sort(([a], [b]) => b - a)
    .map(([year, entries]) => ({
      year,
      yearsAgo: now.getFullYear() - year,
      entries,
    }));
}

/// "1 year ago" / "3 years ago", for an On this day group's heading.
export function formatYearsAgo(yearsAgo: number): string {
  return yearsAgo === 1 ? "1 year ago" : `${yearsAgo} years ago`;
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
