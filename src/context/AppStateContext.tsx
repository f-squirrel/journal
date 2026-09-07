import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { EntryHeader, JournalEntry } from "../types";
import { getEntry, listEntries, saveEntry as saveEntryBackend } from "../api/entries";

const seedEntries: JournalEntry[] = [
  {
    id: "1",
    title: "First day back",
    updatedAt: "2026-09-01T08:15:00.000Z",
    markdown:
      "# First day back\n\nStarted journaling again. Feels good to write things down.\n\n- Coffee\n- Long walk\n- Read a chapter",
  },
  {
    id: "2",
    title: "Project ideas",
    updatedAt: "2026-09-03T19:42:00.000Z",
    markdown:
      "# Project ideas\n\nSome things I want to build:\n\n1. A journaling app\n2. A recipe tracker\n\n> Ideas are cheap, execution is everything.",
  },
];

function toHeader(entry: JournalEntry): EntryHeader {
  return { id: entry.id, title: entry.title, updatedAt: entry.updatedAt };
}

interface AppStateContextValue {
  authenticated: boolean;
  entries: EntryHeader[];
  login: () => void;
  logout: () => void;
  loadEntry: (id: string) => Promise<JournalEntry | undefined>;
  upsertEntry: (entry: JournalEntry) => void;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState(false);
  const [entries, setEntries] = useState<EntryHeader[]>(seedEntries.map(toHeader));

  // Fallback store for when `invoke` isn't available (e.g. `npm run dev` in a browser).
  const offlineEntries = useRef(new Map(seedEntries.map((entry) => [entry.id, entry])));

  useEffect(() => {
    listEntries()
      .then((loaded) => setEntries(loaded))
      .catch(() => {
        // Not running inside Tauri (e.g. `npm run dev` in a browser) — keep seed data.
      });
  }, []);

  const value = useMemo<AppStateContextValue>(
    () => ({
      authenticated,
      entries,
      login: () => setAuthenticated(true),
      logout: () => setAuthenticated(false),
      loadEntry: (id) =>
        getEntry(id)
          .then((entry) => entry ?? offlineEntries.current.get(id))
          .catch(() => offlineEntries.current.get(id)),
      upsertEntry: (entry) => {
        offlineEntries.current.set(entry.id, entry);
        setEntries((prev) => {
          const header = toHeader(entry);
          const index = prev.findIndex((e) => e.id === entry.id);
          if (index === -1) return [header, ...prev];
          const next = [...prev];
          next[index] = header;
          return next;
        });
        saveEntryBackend(entry).catch(() => {
          // Not running inside Tauri — entry only persists in memory for this session.
        });
      },
    }),
    [authenticated, entries],
  );

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}

export function useAppState(): AppStateContextValue {
  const ctx = useContext(AppStateContext);
  if (!ctx) {
    throw new Error("useAppState must be used within AppStateProvider");
  }
  return ctx;
}
