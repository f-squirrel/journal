import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { EntryHeader, JournalEntry } from "../types";
import {
  deleteEntry as deleteEntryBackend,
  getEntry,
  listEntries,
  lockVault,
  saveEntry as saveEntryBackend,
  unlockVault,
} from "../api/entries";

function toHeader(entry: JournalEntry): EntryHeader {
  return { id: entry.id, title: entry.title, updated_at: entry.updated_at };
}

interface AppStateContextValue {
  authenticated: boolean;
  entries: EntryHeader[];
  login: (passphrase: string) => Promise<void>;
  logout: () => void;
  loadEntry: (id: string) => Promise<JournalEntry | undefined>;
  upsertEntry: (entry: JournalEntry) => Promise<void>;
  removeEntry: (id: string) => Promise<void>;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState(false);
  const [entries, setEntries] = useState<EntryHeader[]>([]);

  useEffect(() => {
    if (!authenticated) return;
    listEntries()
      .then((loaded) => setEntries(loaded))
      .catch((err) => {
        // No surface to show this on yet; without the log it would be an
        // unhandled rejection, which is how the failures P0-1 is about hid.
        console.error("Failed to list entries:", err);
      });
  }, [authenticated]);

  // Stable identity: both EditorPage and PreviewPage key a `useEffect` on
  // `loadEntry`, so rebuilding it whenever `entries` changes would re-run
  // those effects and reset the editor's draft mid-save.
  const loadEntry = useCallback((id: string) => getEntry(id).then((entry) => entry ?? undefined), []);

  const value = useMemo<AppStateContextValue>(
    () => ({
      authenticated,
      entries,
      // Rejects with the backend's own message rather than returning a status:
      // every failure used to arrive at `LoginPage` as `false` and be rendered
      // as "Incorrect passphrase.", including a missing key file, a damaged
      // one, or a full disk. Every method on this context signals failure the
      // same way, so no caller has to remember which one returns what.
      login: async (passphrase) => {
        await unlockVault(passphrase);
        setAuthenticated(true);
      },
      logout: () => {
        setAuthenticated(false);
        // Decrypted titles must not outlive the session in the webview heap.
        setEntries([]);
        lockVault().catch(() => {
          // Best-effort; nothing to lock into a UI state for.
        });
      },
      loadEntry,
      // `upsertEntry` and `removeEntry` update React state optimistically and
      // then reject if the backend refuses, rolling the optimistic edit back
      // first: showing an entry as saved while nothing reached disk is how it
      // silently disappears on the next launch.
      upsertEntry: async (entry) => {
        const previous = entries;
        setEntries((prev) => {
          const header = toHeader(entry);
          const index = prev.findIndex((e) => e.id === entry.id);
          if (index === -1) return [header, ...prev];
          const next = [...prev];
          next[index] = header;
          return next;
        });
        try {
          await saveEntryBackend(entry);
        } catch (err) {
          setEntries(previous);
          throw err;
        }
      },
      removeEntry: async (id) => {
        const previous = entries;
        setEntries((prev) => prev.filter((entry) => entry.id !== id));
        try {
          await deleteEntryBackend(id);
        } catch (err) {
          setEntries(previous);
          throw err;
        }
      },
    }),
    [authenticated, entries, loadEntry],
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
