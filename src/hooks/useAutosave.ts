import { useCallback, useEffect, useRef, useState } from "react";
import { adoptMedia } from "../api/media";
import { useAppState } from "../context/AppStateContext";
import { formatEntryHeading } from "../utils/date";
import { errorMessage, isStaleEntry } from "../utils/errors";
import { newEntryId } from "../utils/id";

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
  /// Set after a save was refused as stale and the draft was kept as a
  /// conflicted copy, so the pane can show what happened. Cleared by the next
  /// edit, like `error`.
  notice: string;
  /// Tells the hook what the vault currently holds for this entry, so its next
  /// save's precondition matches: the pane's load sets it to the `updated_at`
  /// it just read, and a clean-pane refresh (`sync://changed`) sets it again.
  /// `null` for a draft that has never reached the vault.
  setBase: (updatedAt: string | null) => void;
  /// What the vault holds for this entry as far as the pane knows, read at
  /// call time: the precondition for a write made outside autosave (a
  /// restore). Unlike `savedAt` it follows loads, refreshes and restores too.
  getBase: () => string | null;
  /// Whether there is anything this pane has not written yet: a dirty draft,
  /// or a write already in flight. The clean-pane refresh skips itself unless
  /// this is false, so it never discards unsaved text.
  hasUnsaved: () => boolean;
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
  /// Settles when the write already in flight does, or at once if there is
  /// none. Named `whenSettled`, not `settled`: `save` declares a local
  /// `settled`, and with no linter configured the shadowing would go uncaught.
  whenSettled: () => Promise<void>;
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
export function useAutosave(
  id: string,
  initialDraft: Draft,
  /// True for an entry that has never reached the vault, so an empty draft is
  /// "nothing was started" rather than "this was cleared on purpose".
  isNew = false,
  /// The entry template a new draft was pre-filled with (`""` for one that
  /// opened blank, and always `""` for an existing entry). A body that still
  /// reads exactly as the template — the user typed nothing, or typed and then
  /// deleted back to it — counts as "nothing was started" too, in the "nothing
  /// was started" guard below: only whitespace differs from an untouched
  /// draft, but the *content* is still the one the user never actually wrote.
  template = "",
  /// The same idea as `template`, for the title: what a new draft's title
  /// field is pre-filled with (its date; `""` for an existing entry). A
  /// title that still reads exactly as this counts as "no title given" too,
  /// so typing only into the body doesn't turn the untouched default into a
  /// title the user never actually chose.
  defaultTitle = "",
  /// Called after a stale save was kept as a conflicted copy, once the copy
  /// itself has been written. The pane uses it to reload the entry from disk
  /// and reset `base`. Held in a ref, not a dependency: the pane's own
  /// callback closes over `loadEntry` and the setters, which must not force
  /// `save`'s identity to change every render.
  onStale?: () => void,
): Autosave {
  const { upsertEntry, registerFlush } = useAppState();
  const [status, setStatus] = useState<SaveStatus>("idle");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [savedAt, setSavedAt] = useState<string | null>(null);
  const onStaleRef = useRef(onStale);
  useEffect(() => {
    onStaleRef.current = onStale;
  });

  // Seeded rather than blank: a save triggered before the pane ever calls
  // `noteChange` — the idle lock's flush, say — must still write the entry's
  // own date and not an empty one. Only the first render's value is kept; the
  // pane owns the draft from then on.
  const draft = useRef({ ...initialDraft, dirty: false });
  /// The `updated_at` the vault holds for this entry as far as the pane
  /// knows: what the last load or refresh read, or what the last successful
  /// save just wrote. `null` for a draft that has never been written — a new
  /// entry starts here, and the pane's load effect sets the real value once
  /// it has read the entry, before any save can happen. Every save's
  /// precondition is this value, so a write the vault has moved past (a
  /// pulled edit) is refused rather than silently overwritten.
  const base = useRef<string | null>(null);
  /// Whether the entry is in the vault: true from the start for one that was
  /// opened, and once the first save of a new one lands.
  const written = useRef(!isNew);
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
    // A draft that was never written and holds nothing is not an entry.
    // Without this, opening "New entry" and navigating away would leave an
    // empty row in the journal. Emptying an entry that *is* on disk is an edit
    // like any other, and is written. A body that still reads exactly as the
    // template it opened with, or was cleared entirely, counts as empty too —
    // the user typed and then deleted back to the template, or deleted past
    // it to nothing, and neither must write an entry holding only the
    // template (or nothing at all). A title is still enough on its own to
    // make the draft real, but not one that still reads exactly as the date
    // it was pre-filled with — the user typing only into the body must not
    // turn that untouched default into a title they never actually chose.
    if (
      !written.current &&
      (!title.trim() || title.trim() === defaultTitle.trim()) &&
      (!markdown.trim() || markdown.trim() === template.trim())
    ) {
      return;
    }

    draft.current.dirty = false;
    saving.current = true;
    let settled!: () => void;
    inflight.current = new Promise<void>((resolve) => (settled = resolve));
    setStatus("saving");
    const updatedAt = new Date().toISOString();
    let failed = false;
    try {
      await upsertEntry(
        {
          id,
          title: title.trim() || formatEntryHeading(entryDate),
          updated_at: updatedAt,
          entry_date: entryDate,
          markdown,
        },
        base.current,
      );
      written.current = true;
      base.current = updatedAt;
      setSavedAt(updatedAt);
      setError("");
      setStatus("saved");
    } catch (err) {
      if (isStaleEntry(err)) {
        // The vault has moved on since this pane last saw it — another device
        // (or a sync pass) wrote a version this draft cannot order itself
        // against. Rather than overwrite it, keep the draft's latest text —
        // including anything typed while this write was in flight — as a new
        // entry, under a fresh id so it never collides with anything.
        const copyId = newEntryId();
        const copy = {
          id: copyId,
          title: `${draft.current.title.trim() || formatEntryHeading(draft.current.entryDate)} (conflicted copy)`,
          updated_at: new Date().toISOString(),
          entry_date: draft.current.entryDate,
          markdown: draft.current.markdown,
        };
        try {
          // The copy owns the media its text shows, so deleting the original
          // later cannot take the copy's images with it.
          copy.markdown = await adoptMedia(copyId, copy.markdown);
          await upsertEntry(copy, null);
          draft.current.dirty = false;
          setNotice(
            `This entry was changed elsewhere while you had it open. Your edit was kept as "${copy.title}".`,
          );
          setError("");
          setStatus("idle");
          // Deferred a tick: `saving.current` is still true until the outer
          // `finally` below runs, and `onStale` (`refreshFromDisk`) checks
          // `hasUnsaved()` — which reads `saving.current` too — before doing
          // anything. Calling it inline would see this very save as still in
          // flight and refuse to reload.
          queueMicrotask(() => onStaleRef.current?.());
        } catch (copyErr) {
          // Behaves like any other failed save: the draft is still the user's
          // work, so keep it dirty and do not reschedule. Retry meets Stale
          // again on the original id and tries the copy again.
          failed = true;
          draft.current.dirty = true;
          setError(errorMessage(copyErr));
          setStatus("error");
        }
      } else {
        // `upsertEntry` has already rolled its optimistic update back, so the
        // list is honest again. The draft is still the user's work: keep it,
        // keep it dirty, and say what went wrong.
        failed = true;
        draft.current.dirty = true;
        setError(errorMessage(err));
        setStatus("error");
      }
    } finally {
      saving.current = false;
      inflight.current = null;
      settled();
      // Anything typed while that write was in flight still needs a write.
      // Not after a failure, though: rescheduling there would retry a broken
      // backend every 800 ms forever. The next keystroke or Retry does it.
      if (draft.current.dirty && !failed) schedule();
    }
  }, [id, schedule, template, defaultTitle, upsertEntry]);

  useEffect(() => {
    saveRef.current = save;
  });

  const noteChange = useCallback(
    (next: Draft) => {
      draft.current = { ...next, dirty: true };
      // "Saved" must not stay up over text that is not saved yet.
      setStatus((prev) => (prev === "saved" ? "idle" : prev));
      // A stale notice describes what an earlier save did; it must not linger
      // over a new edit the user is now making.
      setNotice("");
      schedule();
    },
    [schedule],
  );

  const setBase = useCallback((updatedAt: string | null) => {
    base.current = updatedAt;
  }, []);

  const getBase = useCallback(() => base.current, []);

  const hasUnsaved = useCallback(
    () => draft.current.dirty || saving.current,
    [],
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
  // A blank draft that was never written counts as saved: `save` skips it, so
  // calling it unsaved would block the lock forever over an entry that was
  // never started.
  const flush = useCallback(async () => {
    if (inflight.current) await inflight.current;
    clearTimeout(timer.current);
    await saveRef.current();
    const { title, markdown, dirty } = draft.current;
    return (
      !dirty ||
      (!written.current &&
        (!title.trim() || title.trim() === defaultTitle.trim()) &&
        (!markdown.trim() || markdown.trim() === template.trim()))
    );
  }, [template, defaultTitle]);

  const whenSettled = useCallback(async () => {
    if (inflight.current) await inflight.current;
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

  return {
    status,
    error,
    savedAt,
    notice,
    setBase,
    getBase,
    hasUnsaved,
    noteChange,
    saveNow,
    cancel,
    whenSettled,
  };
}
