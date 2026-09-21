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
import type {
  EntryHeader,
  JournalEntry,
  ProviderInfo,
  SearchMatch,
  SyncStatusPayload,
} from "../types";
import { useIdleLock } from "../hooks/useIdleLock";
import { byEntryDate } from "../utils/date";
import {
  deleteEntry as deleteEntryBackend,
  getEntry,
  listEntries,
  lockVault,
  saveEntry as saveEntryBackend,
  searchEntries,
  unlockVault,
} from "../api/entries";
import { errorMessage } from "../utils/errors";
import {
  connectRemote,
  disconnectRemote,
  listProviders,
  onSyncChanged,
  onSyncStatus,
  syncNow as syncNowBackend,
  syncStatus,
} from "../api/sync";

/// What the app knows about sync before the backend has said: nothing is
/// connected, so no row is marked.
const SYNC_OFF: SyncStatusPayload = {
  state: "off",
  provider: null,
  pending_ids: [],
  conflicted_ids: [],
  pending_other: 0,
  last_synced_at: null,
  message: null,
};

function toHeader(entry: JournalEntry): EntryHeader {
  return {
    id: entry.id,
    title: entry.title,
    updated_at: entry.updated_at,
    entry_date: entry.entry_date,
  };
}

/// A local save or delete, as the list should show it: `header` is `null` for a
/// delete. `settled` is the tick at which the backend answered, or `null` while
/// it has not.
interface Mutation {
  header: EntryHeader | null;
  settled: number | null;
}

/// Short on purpose, so the lock is easy to see working. It becomes a setting.
const IDLE_LOCK_MS = 10_000;

/// How long the typing has to stop before a search runs. `search_entries`
/// decrypts the whole journal to answer, so running one per keystroke would
/// mean several passes over it for a single word.
const SEARCH_DEBOUNCE_MS = 200;

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
  /// The backend's latest word on sync. Sync runs whether or not the vault is
  /// unlocked, so this is live on the login screen too.
  sync: SyncStatusPayload;
  /// False until the backend has answered once, so the login screen does not
  /// offer to connect an account that turns out to be connected already.
  syncLoaded: boolean;
  /// Entries with local changes not uploaded yet, and entries with a parked
  /// conflict, for a row to look itself up in.
  pendingIds: ReadonlySet<string>;
  conflictedIds: ReadonlySet<string>;
  /// The cloud providers this build can connect to.
  providers: ProviderInfo[];
  /// Waits for the user in their browser, so it can take minutes. Rejects with
  /// the reason it could not connect.
  connectSync: (provider: string) => Promise<void>;
  /// Forgets the account. The journal on this device is untouched.
  disconnectSync: () => Promise<void>;
  /// Runs a pass now; rejects with why it failed.
  syncNow: () => Promise<void>;
  /// The search field's text, held here rather than in `EntryList` so that it
  /// survives opening a result — the list column unmounts nothing, but the
  /// query outliving a navigation is the behaviour, not an accident.
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  /// What the backend matched, or `null` when no search is running: an empty
  /// array means "no results", which is a different screen from "not
  /// searching". Also `null` when a search failed, so the list falls back to
  /// showing the journal rather than going blank under an error.
  searchResults: SearchMatch[] | null;
  searching: boolean;
  searchError: string;
  /// The backend's own message for the last failed `list_entries`, or "".
  listError: string;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState(false);
  const [entries, setEntries] = useState<EntryHeader[]>([]);
  const [sync, setSync] = useState<SyncStatusPayload>(SYNC_OFF);
  const [syncLoaded, setSyncLoaded] = useState(false);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchMatch[] | null>(
    null,
  );
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  const [listError, setListError] = useState("");
  // Bumped when a pass changed entries on disk, to re-run the list effect.
  const [listVersion, setListVersion] = useState(0);
  // Set by the first `sync://status` event, so the seed read below cannot land
  // after a newer status and put an older one back.
  const heardStatus = useRef(false);
  const flushes = useRef(new Set<Flush>());
  // A listing reads the disk once, and a `sync://changed` reload can start
  // while a save is in flight or land after one finished: replacing the list
  // with what it read put the row's older title back. So every local save and
  // delete is kept here, by id, until no listing that could have missed it is
  // still open, and laid over each listing's result. `tick` orders the two: a
  // listing notes it when it starts, and a mutation when the backend answers.
  const tick = useRef(0);
  const mutations = useRef(new Map<string, Mutation>());
  const openListings = useRef(new Set<number>());

  // A mutation matters to a listing that started before it settled. Once none
  // of those is open, the disk has it and it can go.
  const trimMutations = useCallback(() => {
    const oldest = Math.min(...openListings.current);
    for (const [id, m] of mutations.current) {
      if (m.settled !== null && m.settled < oldest)
        mutations.current.delete(id);
    }
  }, []);

  // Returns a `settle(ok)` for when the backend answers: kept on success, and
  // dropped on failure, where the caller's rollback already restored the row.
  const recordMutation = useCallback(
    (id: string, header: EntryHeader | null) => {
      const mutation: Mutation = { header, settled: null };
      mutations.current.set(id, mutation);
      return (ok: boolean) => {
        // A later save of the same entry has replaced this one; it is the
        // one that counts.
        if (mutations.current.get(id) !== mutation) return;
        if (ok) {
          mutation.settled = ++tick.current;
        } else {
          mutations.current.delete(id);
        }
        trimMutations();
      };
    },
    [trimMutations],
  );

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
    // Decrypted titles must not outlive the session in the webview heap. The
    // results go with them, and they carry snippets of the entries' text.
    setEntries([]);
    setSearchQuery("");
    setSearchResults(null);
    setSearchError("");
    // A backend message can carry a path; it must not outlive the session.
    setListError("");
    mutations.current.clear();
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
    const started = ++tick.current;
    openListings.current.add(started);
    const close = () => {
      openListings.current.delete(started);
      trimMutations();
    };
    listEntries()
      .then((loaded) => {
        if (cancelled) return;
        // Every save or delete the listing may have missed: one still in
        // flight, or one that settled after it started.
        const missed = [...mutations.current].filter(
          ([, m]) => m.settled === null || m.settled > started,
        );
        const ids = new Set(missed.map(([id]) => id));
        const merged = loaded.filter((header) => !ids.has(header.id));
        for (const [, m] of missed) if (m.header) merged.push(m.header);
        setEntries(merged.sort(byEntryDate));
        setListError("");
      })
      .catch((err) => {
        if (cancelled) return;
        // `entries` is left as it was, so a failed reload keeps the rows on
        // screen; `listError` is what stops an empty list reading as "no
        // entries". The log stays: without it this is an unhandled rejection.
        console.error("Failed to list entries:", err);
        setListError(errorMessage(err));
      })
      .finally(close);
    return () => {
      cancelled = true;
      close();
    };
  }, [authenticated, listVersion, trimMutations]);

  // Searching is a command, not a filter over `entries`: those are headers
  // with no markdown, so only the backend can match what an entry says (see
  // `Vault::search`). Debounced, because answering means decrypting the
  // journal.
  //
  // It re-runs on `listVersion` — the same bump that reloads the list when a
  // sync pass brought entries in — so results do not go on describing a
  // journal that has changed underneath them. Not on `entries`, which
  // autosave changes every couple of seconds while the user types.
  useEffect(() => {
    const query = searchQuery.trim();
    if (!authenticated || !query) {
      setSearchResults(null);
      setSearching(false);
      setSearchError("");
      return;
    }

    let cancelled = false;
    setSearching(true);
    const timer = setTimeout(() => {
      searchEntries(query)
        .then((found) => {
          if (cancelled) return;
          setSearchResults(found);
          setSearchError("");
        })
        .catch((err) => {
          if (cancelled) return;
          // Results go back to `null`, so the column shows the journal again
          // under the error rather than an empty pane that reads as "nothing
          // matched" — which is the one thing a failed search does not know.
          setSearchResults(null);
          setSearchError(errorMessage(err));
        })
        .finally(() => {
          if (!cancelled) setSearching(false);
        });
    }, SEARCH_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [authenticated, searchQuery, listVersion]);

  // Sync is not gated on the vault: it runs while locked, and connecting is
  // how a second device gets its key, so this is live from the first render.
  useEffect(() => {
    let cancelled = false;
    const stops: Array<() => void> = [];
    // A listener that finishes registering after the effect was cleaned up
    // (StrictMode runs it twice in development) stops itself at once.
    const keep = (registering: Promise<() => void>) =>
      registering
        .then((stop) => (cancelled ? stop() : stops.push(stop)))
        .catch((err) => console.error("Could not listen for sync:", err));

    void keep(
      onSyncStatus((status) => {
        heardStatus.current = true;
        setSync(status);
        setSyncLoaded(true);
      }),
    );
    // The entry open in the pane is left alone: autosave owns its draft, and a
    // remote edit to it has already become a conflicted copy on the backend.
    void keep(onSyncChanged(() => setListVersion((n) => n + 1)));

    syncStatus()
      .then((status) => {
        if (cancelled) return;
        if (!heardStatus.current) setSync(status);
        setSyncLoaded(true);
      })
      .catch((err) => console.error("Failed to read the sync status:", err));
    listProviders()
      .then((listed) => {
        if (!cancelled) setProviders(listed);
      })
      .catch((err) => console.error("Failed to list sync providers:", err));

    return () => {
      cancelled = true;
      stops.forEach((stop) => stop());
    };
  }, []);

  const pendingIds = useMemo(() => new Set<string>(sync.pending_ids), [sync]);
  const conflictedIds = useMemo(
    () => new Set<string>(sync.conflicted_ids),
    [sync],
  );

  // Read after every command as well as waiting for the event: the events are
  // throttled, and what a caller awaits should be reflected when it returns.
  const refreshSync = useCallback(
    () =>
      syncStatus()
        .then(setSync)
        .catch((err) => console.error("Failed to read the sync status:", err)),
    [],
  );

  const connectSync = useCallback(
    async (provider: string) => {
      try {
        await connectRemote(provider);
      } finally {
        await refreshSync();
      }
    },
    [refreshSync],
  );

  const disconnectSync = useCallback(async () => {
    try {
      await disconnectRemote();
    } finally {
      await refreshSync();
    }
  }, [refreshSync]);

  const syncNow = useCallback(async () => {
    try {
      await syncNowBackend();
    } finally {
      await refreshSync();
    }
  }, [refreshSync]);

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
      sync,
      syncLoaded,
      pendingIds,
      conflictedIds,
      providers,
      connectSync,
      disconnectSync,
      syncNow,
      searchQuery,
      setSearchQuery,
      searchResults,
      searching,
      searchError,
      listError,
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
        const settle = recordMutation(entry.id, header);
        // Read inside the updater, not from `entries` in this closure, so the
        // rollback restores the row as it actually was. Assigned rather than
        // returned because that is the only way out of a state updater; it is
        // idempotent, which is what StrictMode's double invocation needs.
        let replaced: EntryHeader | undefined;
        setEntries((prev) => {
          replaced = prev.find((e) => e.id === entry.id);
          return [header, ...prev.filter((e) => e.id !== entry.id)].sort(
            byEntryDate,
          );
        });
        try {
          await saveEntryBackend(entry);
          settle(true);
        } catch (err) {
          settle(false);
          setEntries((prev) => {
            const without = prev.filter((e) => e.id !== entry.id);
            return replaced
              ? [replaced, ...without].sort(byEntryDate)
              : without;
          });
          throw err;
        }
      },
      removeEntry: async (id) => {
        let removed: EntryHeader | undefined;
        const settle = recordMutation(id, null);
        setEntries((prev) => {
          removed = prev.find((entry) => entry.id === id);
          return prev.filter((entry) => entry.id !== id);
        });
        try {
          await deleteEntryBackend(id);
          settle(true);
        } catch (err) {
          settle(false);
          setEntries((prev) => {
            if (!removed) return prev;
            // Filtered first, so a re-added entry cannot end up duplicated.
            const without = prev.filter((entry) => entry.id !== id);
            return [removed, ...without].sort(byEntryDate);
          });
          throw err;
        }
      },
    }),
    [
      authenticated,
      entries,
      flushAll,
      lockSession,
      registerFlush,
      recordMutation,
      loadEntry,
      sync,
      syncLoaded,
      pendingIds,
      conflictedIds,
      providers,
      connectSync,
      disconnectSync,
      syncNow,
      searchQuery,
      searchResults,
      searching,
      searchError,
      listError,
    ],
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
