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
import type { Settings, SettingsPatch } from "../types";
import { getSettings, updateSettings } from "../api/settings";
import { onSyncChanged } from "../api/sync";

// One flat `Settings`, built by the backend from the device and journal
// halves. This context is the only
// thing in the app that calls `getSettings`/`updateSettings`; everything
// else reads `useSettings().settings` and writes through `update`.

/// The one TS copy of the defaults in the shipped code, used only when the
/// first `getSettings()` fails. Typed as `Settings` so `tsc` fails if a field
/// is added to either half and not here. Everywhere else the app reads what
/// the backend returned; a later successful `reload()` replaces this.
const LOAD_FAILED_SETTINGS: Settings = {
  color_mode: "dark", // matches the login page today
  color_scheme: "parchment",
  idle_lock_minutes: 5, // never null: keeps auto-lock on
  sync_interval_minutes: 1,
  shortcuts: {}, // default bindings
  sync_providers: { dropbox: { app_key: null } },
  week_starts_on: "sunday",
  time_format: "locale",
  entry_template: "",
};

interface SettingsContextValue {
  /// `confirmed ⊕ pending`: the last settled `Settings` with every queued or
  /// in-flight patch overlaid on top, in order, so a change shows at once
  /// and nothing flicks back while its request is in flight.
  settings: Settings;
  update: (patch: SettingsPatch) => Promise<void>;
  reload: () => Promise<void>;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

/// A queued or in-flight patch, tagged with an id so it can be removed once
/// its request settles — by id, not by object identity, so a patch that
/// happens to be `===` another (or a caller that mutates and resubmits the
/// same object) can never remove the wrong entry.
interface PendingPatch {
  id: number;
  patch: SettingsPatch;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [confirmed, setConfirmed] = useState<Settings | null>(null);
  const [pending, setPending] = useState<PendingPatch[]>([]);
  const [loaded, setLoaded] = useState(false);
  const nextPendingId = useRef(0);

  // One promise queue: every `update` and `reload` chains onto it, so the
  // backend handles them in the order they were made and only one request is
  // ever in flight. Dropping "stale" responses by a counter does not work
  // here.
  const queue = useRef<Promise<void>>(Promise.resolve());
  const enqueue = useCallback(<T,>(fn: () => Promise<T>): Promise<T> => {
    const run = queue.current.then(fn);
    // The queue itself must never reject, or every later entry would be
    // skipped; the rejection still reaches whoever awaits `run`.
    queue.current = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }, []);

  const reload = useCallback((): Promise<void> => {
    return enqueue(() =>
      getSettings().then(
        (next) => {
          setConfirmed(next);
        },
        (err: unknown) => {
          console.warn("Failed to load settings:", err);
          // Only the first failure installs the fallback constant; a later
          // one (from a rejected update's re-fetch) leaves `confirmed` as it
          // was, the last state the backend actually reported.
          setConfirmed((prev) => prev ?? LOAD_FAILED_SETTINGS);
        },
      ),
    );
  }, [enqueue]);

  const update = useCallback(
    (patch: SettingsPatch): Promise<void> => {
      const id = nextPendingId.current++;
      setPending((prev) => [...prev, { id, patch }]);
      return enqueue(() => updateSettings(patch)).then(
        (returned) => {
          setConfirmed(returned);
          setPending((prev) => prev.filter((p) => p.id !== id));
        },
        (err: unknown) => {
          // Dropped rather than restored from a snapshot: `confirmed ⊕
          // pending` never holds a failed optimistic value, so there is
          // nothing to roll back. `reload()` picks up anything a partly
          // applied patch did write.
          setPending((prev) => prev.filter((p) => p.id !== id));
          void reload();
          throw err;
        },
      );
    },
    [enqueue, reload],
  );

  useEffect(() => {
    void reload().then(() => setLoaded(true));
    // `reload` is stable (it only depends on `enqueue`, whose own deps are
    // empty), so listing it here doesn't make this run more than once.
  }, [reload]);

  // Reloads whenever a sync pass changed entries on disk, which includes a
  // pulled or merged settings blob. A listener that
  // finishes registering after this effect was cleaned up (StrictMode runs
  // it twice in development) stops itself at once, mirroring
  // `AppStateContext`'s `keep`.
  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | undefined;
    onSyncChanged(() => void reload())
      .then((unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      })
      .catch((err: unknown) =>
        console.error("Could not listen for sync:", err),
      );
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [reload]);

  const settings = useMemo<Settings>(() => {
    const base = confirmed ?? LOAD_FAILED_SETTINGS;
    return pending.reduce<Settings>(
      (acc, { patch }) => ({ ...acc, ...patch }),
      base,
    );
  }, [confirmed, pending]);

  const value = useMemo<SettingsContextValue>(
    () => ({ settings, update, reload }),
    [settings, update, reload],
  );

  // Renders children only once the first load settles, so the app never
  // flashes the wrong colour mode before the real one is known — this is a
  // local file read (or, in the fallback, `localStorage`), not a network
  // call, so the wait is not noticeable.
  if (!loaded) return null;

  return (
    <SettingsContext.Provider value={value}>
      {children}
    </SettingsContext.Provider>
  );
}

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) {
    throw new Error("useSettings must be used within SettingsProvider");
  }
  return ctx;
}
