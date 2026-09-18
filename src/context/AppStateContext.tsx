import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { EntryHeader, JournalEntry } from "../types";
import { useIdleLock } from "../hooks/useIdleLock";
import { byUpdatedAt } from "../utils/date";
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

/// Short on purpose, so the lock is easy to see working. It becomes a setting.
const IDLE_LOCK_MS = 10_000;

/// Writes whatever the caller has not saved yet. Resolves `true` when nothing
/// is left unsaved, `false` when a write failed and the work is still only in
/// memory.
type Flush = () => Promise<boolean>;

interface AppStateContextValue {
  authenticated: boolean;
  entries: EntryHeader[];
  login: (passphrase: string) => Promise<void>;
  logout: () => void;
  /// Lets a pane hand the lock its unsaved work. Returns the unregister
  /// function, so it fits directly into an effect.
  registerFlush: (flush: Flush) => () => void;
  loadEntry: (id: string) => Promise<JournalEntry | undefined>;
  upsertEntry: (entry: JournalEntry) => Promise<void>;
  removeEntry: (id: string) => Promise<void>;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState(false);
  const [entries, setEntries] = useState<EntryHeader[]>([]);
  const flushes = useRef(new Set<Flush>());

  const registerFlush = useCallback((flush: Flush) => {
    flushes.current.add(flush);
    return () => {
      flushes.current.delete(flush);
    };
  }, []);

  // Locking used to be `setAuthenticated(false)` plus `lockVault()`, which
  // unmounts the pane, whose unmount flush then writes to a vault that is
  // already locked and is refused: an in-flight or previously failed save lost
  // its draft. Everything that locks writes first, through here.
  const flushAll = useCallback(async () => {
    const saved = await Promise.all(
      [...flushes.current].map((flush) => flush().catch(() => false)),
    );
    return saved.every(Boolean);
  }, []);

  const lockSession = useCallback(() => {
    setAuthenticated(false);
    // Decrypted titles must not outlive the session in the webview heap.
    setEntries([]);
    lockVault().catch(() => {
      // Best-effort; nothing to lock into a UI state for.
    });
  }, []);

  // Unlike a manual log out, an idle lock is not something the user asked for,
  // so it does not go through over unsaved work: the pane is showing the error
  // and the draft is the only copy. It declines and is retried after the next
  // quiet period. It also stands down if the user came back while the flush
  // was running, since what they typed then is not in the flush's result.
  const lockWhenIdle = useCallback(
    async (stillIdle: () => boolean) => {
      const saved = await flushAll();
      if (!saved || !stillIdle()) return false;
      lockSession();
      return true;
    },
    [flushAll, lockSession],
  );

  useIdleLock(authenticated, lockWhenIdle, IDLE_LOCK_MS);

  useEffect(() => {
    if (!authenticated) return;
    // Commands run off the main thread, so a `list_entries` can still be in
    // flight when the user logs out. Without this guard its result lands after
    // `logout`'s `setEntries([])` and puts the previous session's decrypted
    // titles back — on the login screen, and again on the next unlock.
    let cancelled = false;
    listEntries()
      .then((loaded) => {
        if (cancelled) return;
        setEntries(loaded);
      })
      .catch((err) => {
        if (cancelled) return;
        // No surface to show this on yet; without the log it would be an
        // unhandled rejection, which is how the failures P0-1 is about hid.
        console.error("Failed to list entries:", err);
      });
    return () => {
      cancelled = true;
    };
  }, [authenticated]);

  // Stable identity: both EditorPage and PreviewPage key a `useEffect` on
  // `loadEntry`, so rebuilding it whenever `entries` changes would re-run
  // those effects and reset the editor's draft mid-save.
  const loadEntry = useCallback(
    (id: string) => getEntry(id).then((entry) => entry ?? undefined),
    [],
  );

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
      // Locks whether or not the flush succeeded: the user asked, and a broken
      // backend must not be able to keep the journal open.
      logout: () => {
        void flushAll().then(lockSession);
      },
      registerFlush,
      loadEntry,
      // `upsertEntry` and `removeEntry` update React state optimistically and
      // then reject if the backend refuses, rolling the optimistic edit back
      // first: showing an entry as saved while nothing reached disk is how it
      // silently disappears on the next launch.
      //
      // Both re-sort, because the list is the backend's order
      // (`Vault::list_entries`, most-recently-updated first) and an edit
      // changes where its entry belongs in it. Replacing the header in place
      // left an entry showing a fresh timestamp under entries it was now
      // newer than, until the next unlock re-fetched.
      //
      // Both roll back with a functional update that removes or restores only
      // the one entry, rather than restoring a whole array captured before the
      // save. That array is stale by definition — anything else that landed
      // while the save was in flight would be discarded with it. Today the UI
      // serializes mutations by disabling buttons, so this is a trap set for
      // whoever adds autosave.
      upsertEntry: async (entry) => {
        const header = toHeader(entry);
        // Read inside the updater, not from `entries` in this closure, so the
        // rollback restores the row as it actually was. Assigned rather than
        // returned because that is the only way out of a state updater; it is
        // idempotent, which is what StrictMode's double invocation needs.
        let replaced: EntryHeader | undefined;
        setEntries((prev) => {
          replaced = prev.find((e) => e.id === entry.id);
          return [header, ...prev.filter((e) => e.id !== entry.id)].sort(
            byUpdatedAt,
          );
        });
        try {
          await saveEntryBackend(entry);
        } catch (err) {
          setEntries((prev) => {
            const without = prev.filter((e) => e.id !== entry.id);
            return replaced
              ? [replaced, ...without].sort(byUpdatedAt)
              : without;
          });
          throw err;
        }
      },
      removeEntry: async (id) => {
        let removed: EntryHeader | undefined;
        setEntries((prev) => {
          removed = prev.find((entry) => entry.id === id);
          return prev.filter((entry) => entry.id !== id);
        });
        try {
          await deleteEntryBackend(id);
        } catch (err) {
          setEntries((prev) => {
            if (!removed) return prev;
            // Filtered first, so a re-added entry cannot end up duplicated.
            const without = prev.filter((entry) => entry.id !== id);
            return [removed, ...without].sort(byUpdatedAt);
          });
          throw err;
        }
      },
    }),
    [authenticated, entries, flushAll, lockSession, registerFlush, loadEntry],
  );

  return (
    <AppStateContext.Provider value={value}>
      {children}
    </AppStateContext.Provider>
  );
}

export function useAppState(): AppStateContextValue {
  const ctx = useContext(AppStateContext);
  if (!ctx) {
    throw new Error("useAppState must be used within AppStateProvider");
  }
  return ctx;
}
