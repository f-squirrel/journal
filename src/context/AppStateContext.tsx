import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { JournalEntry } from "../types";
import { listEntries, saveEntry as saveEntryBackend } from "../api/entries";

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

interface AppStateContextValue {
  authenticated: boolean;
  entries: JournalEntry[];
  login: () => void;
  logout: () => void;
  findEntry: (id: string) => JournalEntry | undefined;
  upsertEntry: (entry: JournalEntry) => void;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState(false);
  const [entries, setEntries] = useState<JournalEntry[]>(seedEntries);

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
      findEntry: (id) => entries.find((entry) => entry.id === id),
      upsertEntry: (entry) => {
        setEntries((prev) => {
          const index = prev.findIndex((e) => e.id === entry.id);
          if (index === -1) return [entry, ...prev];
          const next = [...prev];
          next[index] = entry;
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
