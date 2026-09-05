import type { JournalEntry } from "./types";

export interface AppState {
  authenticated: boolean;
  entries: JournalEntry[];
}

const seedEntries: JournalEntry[] = [
  {
    id: "1",
    title: "First day back",
    updatedAt: "2026-09-01",
    markdown:
      "# First day back\n\nStarted journaling again. Feels good to write things down.\n\n- Coffee\n- Long walk\n- Read a chapter",
  },
  {
    id: "2",
    title: "Project ideas",
    updatedAt: "2026-09-03",
    markdown:
      "# Project ideas\n\nSome things I want to build:\n\n1. A journaling app\n2. A recipe tracker\n\n> Ideas are cheap, execution is everything.",
  },
];

export const state: AppState = {
  authenticated: false,
  entries: seedEntries,
};

export function findEntry(id: string): JournalEntry | undefined {
  return state.entries.find((entry) => entry.id === id);
}

export function upsertEntry(entry: JournalEntry): void {
  const index = state.entries.findIndex((e) => e.id === entry.id);
  if (index === -1) {
    state.entries.unshift(entry);
  } else {
    state.entries[index] = entry;
  }
}
