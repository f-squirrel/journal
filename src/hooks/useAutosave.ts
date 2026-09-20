import { useCallback, useEffect, useRef, useState } from "react";
import { useAppState } from "../context/AppStateContext";
import { errorMessage } from "../utils/errors";

/// How long the typing has to stop before the entry is written.
const DEBOUNCE_MS = 800;

export type SaveStatus = "idle" | "saving" | "saved" | "error";

export interface Autosave {
  status: SaveStatus;
  /// The backend's own message for the last failed save, shown verbatim.
  error: string;
  /// `updated_at` of the last successful save, so the pane's date heading
  /// follows the entry rather than showing what it said when it opened.
  /// Null until this pane has saved anything.
  savedAt: string | null;
  /// Records an edit and schedules the write. `entryDate` travels with the
  /// draft because changing the date is an edit like any other.
  noteChange: (draft: Draft) => void;
  /// Writes now, without waiting out the debounce: the Retry action on a
  /// failed save, and the switch back to the rendered view, which must not
  /// show text the vault has not been given yet.
  saveNow: () => void;
  /// Drops any pending write, including the one the unmount flush would make.
  /// Deleting an entry has to call this, or a scheduled save resurrects it.
  /// Returns whether there was one, so a delete that fails can put it back
  /// without inventing a write the user never earned.
  cancel: () => boolean;
}

/// What the pane holds and what a save writes, minus `updated_at` — that is
/// the moment of the write itself, so `save` stamps it rather than the caller.
export interface Draft {
  title: string;
  markdown: string;
  /// ISO 8601, as it is on the wire and on disk.
  entryDate: string;
}

/// Saves the draft as the user types, the way Day One does: there is no Save
/// button, so no part of a session is lost by navigating away.
///
/// The live draft lives in a ref as well as in the pane's state, because the
/// unmount cleanup has to flush what the user last typed, not what the closure
/// captured when the effect ran. The timers reach `save` through a ref for the
/// same reason: `upsertEntry`'s identity changes with every `entries` update,
/// so a timer keyed on it would be torn down by unrelated saves.
export function useAutosave(id: string, initialDraft: Draft): Autosave {
  const { upsertEntry, registerFlush } = useAppState();
  const [status, setStatus] = useState<SaveStatus>("idle");
  const [error, setError] = useState("");
  const [savedAt, setSavedAt] = useState<string | null>(null);

  // Seeded rather than blank: a save triggered before the pane ever calls
  // `noteChange` — the idle lock's flush, say — must still write the entry's
  // own date and not an empty one. Only the first render's value is kept; the
  // pane owns the draft from then on.
  const draft = useRef({ ...initialDraft, dirty: false });
  const saving = useRef(false);
  /// Settles when the write in flight does; null when there is none. What
  /// `flush` waits on, since `save` itself returns at once if one is running.
  const inflight = useRef<Promise<void> | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const saveRef = useRef<() => Promise<void>>(async () => {});

  const schedule = useCallback(() => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void saveRef.current(), DEBOUNCE_MS);
  }, []);

  const save = useCallback(async () => {
    if (!draft.current.dirty) return;
    // A save is already in flight: leave `dirty` set and let its `finally`
    // reschedule, rather than racing two writes of the same entry.
    if (saving.current) return;

    const { title, markdown, entryDate } = draft.current;
    // An untouched draft is not an entry. Without this, opening "New entry"
    // and navigating away would leave an empty row in the journal.
    if (!title.trim() && !markdown.trim()) return;

    draft.current.dirty = false;
    saving.current = true;
    let settled!: () => void;
    inflight.current = new Promise<void>((resolve) => (settled = resolve));
    setStatus("saving");
    const updatedAt = new Date().toISOString();
    let failed = false;
    try {
      await upsertEntry({
        id,
        title: title.trim() || "Untitled entry",
        updated_at: updatedAt,
        entry_date: entryDate,
        markdown,
      });
      setSavedAt(updatedAt);
      setError("");
      setStatus("saved");
    } catch (err) {
      // `upsertEntry` has already rolled its optimistic update back, so the
      // list is honest again. The draft is still the user's work: keep it, keep
      // it dirty, and say what went wrong.
      failed = true;
      draft.current.dirty = true;
      setError(errorMessage(err));
      setStatus("error");
    } finally {
      saving.current = false;
      inflight.current = null;
      settled();
      // Anything typed while that write was in flight still needs a write.
      // Not after a failure, though: rescheduling there would retry a broken
      // backend every 800 ms forever. The next keystroke or Retry does it.
      if (draft.current.dirty && !failed) schedule();
    }
  }, [id, schedule, upsertEntry]);

  useEffect(() => {
    saveRef.current = save;
  });

  const noteChange = useCallback(
    (next: Draft) => {
      draft.current = { ...next, dirty: true };
      // "Saved" must not stay up over text that is not saved yet.
      setStatus((prev) => (prev === "saved" ? "idle" : prev));
      schedule();
    },
    [schedule],
  );

  const cancel = useCallback(() => {
    clearTimeout(timer.current);
    const pending = draft.current.dirty;
    draft.current.dirty = false;
    return pending;
  }, []);

  const saveNow = useCallback(() => {
    clearTimeout(timer.current);
    void saveRef.current();
  }, []);

  // What the lock waits on before it drops the vault. Unlike `saveNow` it
  // outlasts a write already in flight and reports whether the draft made it.
  // A blank draft counts as saved: `save` never writes one, so calling it
  // unsaved would block the lock forever over an entry that was never started.
  const flush = useCallback(async () => {
    if (inflight.current) await inflight.current;
    clearTimeout(timer.current);
    await saveRef.current();
    const { title, markdown, dirty } = draft.current;
    return !dirty || (!title.trim() && !markdown.trim());
  }, []);

  useEffect(() => registerFlush(flush), [registerFlush, flush]);

  // Flush on unmount — which, because `EntryPaneRoute` keys the pane on the
  // route id, is also what happens when the user picks another entry.
  useEffect(
    () => () => {
      clearTimeout(timer.current);
      void saveRef.current();
    },
    [],
  );

  return { status, error, savedAt, noteChange, saveNow, cancel };
}
