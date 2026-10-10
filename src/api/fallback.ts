import type {
  AdoptedJournal,
  ConnectOutcome,
  DeviceSettings,
  SyncProviders,
  EntryHeader,
  JournalEntry,
  JournalSettings,
  MediaId,
  MediaMeta,
  MediaVariant,
  ProviderInfo,
  SearchMatch,
  Settings,
  SettingsPatch,
  SyncStatusPayload,
} from "../types";
import { byEntryDate } from "../utils/date";
import { STALE_ENTRY_MESSAGE } from "../utils/errors";
import { isWebSafe, sniffImageFormat } from "../media/format";
import { findMediaRefs, mediaRef } from "../media/ref";
import { newEntryId } from "../utils/id";

// The browser fallback: an in-memory vault for `npm run dev` and the
// Playwright suite, where there is no Rust backend to talk to.
//
// This module is reached only through a dynamic `import()` that
// `src/api/backend.ts` guards with `import.meta.env.DEV`, so none of it —
// not the fabricated entries below, not the URL fault hook — exists in a
// production bundle. `make check-no-fallback` is what holds that: the
// guard is only as good as the bundler dropping this chunk. Keep the
// import dynamic, and keep every module-scope constant here rather than
// in `backend.ts`, or the two come back.

const seedEntries: JournalEntry[] = [
  {
    id: "5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01",
    title: "First day back",
    updated_at: "2026-09-01T08:15:00.000Z",
    // Written up in September, but about a day in June: the pair that proves
    // the list sorts and groups by `entry_date` and not by `updated_at`.
    entry_date: "2026-06-12T07:30:00.000Z",
    markdown:
      "# First day back\n\nStarted journaling again. Feels good to write things down.\n\n- Coffee\n- Long walk\n- Read a chapter",
  },
  {
    id: "8a2e4d3b-2b7a-4f1e-8c3d-2e7a4b1d0a02",
    title: "Project ideas",
    updated_at: "2026-09-03T19:42:00.000Z",
    entry_date: "2026-09-02T21:05:00.000Z",
    markdown:
      "# Project ideas\n\nSome things I want to build:\n\n1. A journaling app\n2. A recipe tracker\n\n> Ideas are cheap, execution is everything.",
  },
];

function toHeader(entry: JournalEntry): EntryHeader {
  return {
    id: entry.id,
    title: entry.title,
    updated_at: entry.updated_at,
    entry_date: entry.entry_date,
  };
}

// --- fault injection -------------------------------------------------------
//
// The fallback cannot fail on its own, and Playwright never crosses the
// `invoke` bridge, so backend failures have to be injectable from the URL:
//
//   ?vault=new          hasVault() resolves false, giving the create-vault UI
//   ?vault=arrives      the same, but the journal's key arrives from the cloud
//                       while the first unlock is creating one: that unlock
//                       rejects with the backend's message, and hasVault()
//                       resolves true from then on
//   ?fail=<ops>         comma-separated: has, unlock, list, get, save, delete,
//                       history, restore, search, export, connect, sync,
//                       save-media, get-media (originals and display copies),
//                       get-thumb (thumbnails), adopt-media, version
//                       (a history preview),
//                       resolve (see "simulated sync" below), settings
//                       (updateSettings rejects), settings-load (getSettings
//                       rejects)
//   ?failMessage=<text> the rejection message (default "Injected fault.")
//   ?failAfter=<n>      successful calls of each failing op before ?fail= bites
//                       (default 0: fail from the first call), so a listing
//                       can succeed and its reload fail (?fail=list&failAfter=1)
//   ?failTimes=<n>      how many consecutive calls (after ?failAfter=) actually
//                       fail before the op goes back to succeeding (default
//                       unbounded, i.e. every call from ?failAfter= on fails).
//                       ?fail=save&failTimes=1 is a one-shot fault: the next
//                       save of the same op succeeds.
//   ?slow=<ops>         the named ops resolve after SLOW_MS. settings-first
//                       delays only the first updateSettings call after page
//                       load, unlike settings, which delays every call
//   ?slowMs=<n>         non-negative delay for slow operations (default SLOW_MS)
//   ?thumbs=off         saving a thumbnail succeeds but stores nothing, as for
//                       an image added before thumbnails existed
//   ?dropboxKey=none    the build has no Dropbox app key, so Dropbox needs one
//                       from Settings (default: the build has one)
//   ?sync=<mode>        start connected to a provider: on, pull, conflict,
//                       busy, reauth, key_mismatch, other_journal (see below)
//
// The app uses HashRouter, so the query string goes *before* the hash:
// `page.goto("/?fail=save")`, not `/#/?fail=save`.

type Op =
  | "has"
  | "unlock"
  | "list"
  | "get"
  | "save"
  | "delete"
  | "history"
  | "version"
  | "restore"
  | "search"
  | "export"
  | "connect"
  | "adopt"
  | "cloud_has_journal"
  | "sync"
  | "save-media"
  | "get-media"
  | "get-thumb"
  | "adopt-media"
  | "resolve"
  | "settings"
  | "settings-load"
  | "settings-first";

const SLOW_MS = 1200;

interface Faults {
  newVault: boolean;
  keyArrives: boolean;
  fail: Set<Op>;
  failMessage: string;
  failAfter: number;
  failTimes: number;
  slow: Set<Op>;
  slowMs: number;
  thumbsOff: boolean;
  sync: SyncMode;
  noBuildKey: boolean;
}

type SyncMode =
  | "off"
  | "on"
  | "pull"
  | "conflict"
  | "busy"
  | "reauth"
  | "key_mismatch"
  | "other_journal";
const SYNC_MODES: readonly SyncMode[] = [
  "off",
  "on",
  "pull",
  "conflict",
  "busy",
  "reauth",
  "key_mismatch",
  "other_journal",
];

function parseFaults(search: string): Faults {
  const params = new URLSearchParams(search);
  const ops = (value: string | null) =>
    new Set((value ?? "").split(",").filter(Boolean) as Op[]);
  const rawSlowMs = params.get("slowMs");
  const requestedSlowMs =
    rawSlowMs === null || rawSlowMs.trim() === "" ? SLOW_MS : Number(rawSlowMs);
  return {
    newVault: ["new", "arrives"].includes(params.get("vault") ?? ""),
    keyArrives: params.get("vault") === "arrives",
    fail: ops(params.get("fail")),
    failMessage: params.get("failMessage") ?? "Injected fault.",
    failAfter: Number(params.get("failAfter") ?? 0),
    failTimes: params.has("failTimes")
      ? Number(params.get("failTimes"))
      : Infinity,
    slow: ops(params.get("slow")),
    slowMs:
      Number.isFinite(requestedSlowMs) && requestedSlowMs >= 0
        ? requestedSlowMs
        : SLOW_MS,
    thumbsOff: params.get("thumbs") === "off",
    sync: SYNC_MODES.find((mode) => mode === params.get("sync")) ?? "off",
    noBuildKey: params.get("dropboxKey") === "none",
  };
}

const faults: Faults = parseFaults(window.location.search);
// Calls seen so far by each op named in `?fail=`, for `?failAfter=`.
const failedCalls = new Map<Op, number>();
const fallbackEntries = new Map(seedEntries.map((entry) => [entry.id, entry]));

/// Media, keyed `<entry-id>/<media-id>/<variant>` like a blob name is. Lives
/// here, at module scope, so none of it exists in a production bundle.
const fallbackMedia = new Map<string, { meta: MediaMeta; bytes: Uint8Array }>();
const mediaKey = (entryId: string, mediaId: string, variant: MediaVariant) =>
  `${entryId}/${mediaId}/${variant}`;
const MEDIA_VARIANTS: readonly MediaVariant[] = [
  "original",
  "display",
  "thumb",
];

/// An entry's past versions, newest first, capped at `MAX_FALLBACK_HISTORY`.
/// Mirrors `history::MAX_SNAPSHOTS_PER_ENTRY` in `crates/vault` loosely — it
/// doesn't need to match exactly, just be documented (see below).
const fallbackHistory = new Map<string, JournalEntry[]>();
const MAX_FALLBACK_HISTORY = 20;

/// Pushes `previous` onto `id`'s history.
///
/// **Does not replicate the real backend's 30-minute throttle.** It pushes
/// unconditionally, on every save and every restore, so `e2e/` can exercise
/// the list/preview/restore UI without waiting out a real cadence — this
/// models enough of the feature to test the UI, not production's snapshot
/// policy.
function pushHistory(id: string, previous: JournalEntry) {
  const history = fallbackHistory.get(id) ?? [];
  history.unshift(previous);
  history.length = Math.min(history.length, MAX_FALLBACK_HISTORY);
  fallbackHistory.set(id, history);
}

// Locked until `unlockVault`, like `VaultState`. Without it a write made after
// the lock succeeds here and is refused by the real backend, so nothing in the
// Playwright suite could catch a draft lost that way.
let locked = true;

// Whether the vault exists, which connecting can change: on a new device the
// journal's key arrives from the cloud (see `connectRemote`).
let vaultPresent = !faults.newVault;

/// Runs one fallback operation, honouring any fault injected via the URL.
/// Rejects with a bare string, the way `invoke` rejects with the Rust
/// command's `Err(String)`.
async function run<T>(op: Op, work: () => T): Promise<T> {
  // Checked up front, before any delay: the Rust commands clone the vault
  // out of `VaultState` at the start, so a write already running when the
  // lock lands still completes.
  // Connecting needs no vault, and neither does any part of sync.
  // `get_settings` reads the device half unconditionally and the journal
  // half as defaults while locked (see `currentSettings`), so it is never
  // refused here either.
  if (
    locked &&
    op !== "has" &&
    op !== "unlock" &&
    op !== "connect" &&
    op !== "cloud_has_journal" &&
    op !== "settings-load"
  ) {
    throw "vault is locked";
  }
  if (faults.slow.has(op)) {
    await new Promise((resolve) => setTimeout(resolve, faults.slowMs));
  }
  if (faults.fail.has(op)) {
    const seen = failedCalls.get(op) ?? 0;
    failedCalls.set(op, seen + 1);
    if (
      seen >= faults.failAfter &&
      seen < faults.failAfter + faults.failTimes
    ) {
      throw faults.failMessage;
    }
  }
  return work();
}

// --- commands --------------------------------------------------------------
//
// One function per Tauri command in `entries.ts`, with the same signature.

export function hasVault(): Promise<boolean> {
  return run("has", () => vaultPresent);
}

export function unlockVault(_passphrase: string): Promise<void> {
  return run("unlock", () => {
    if (faults.keyArrives && !vaultPresent) {
      // `KeyError::KeyArrivedWhileCreating`, as `Keyring::create` reports it.
      vaultPresent = true;
      throw "This journal already exists — it arrived from the cloud while yours was being created. Sign in with the passphrase you chose when you first created it, not the new one.";
    }
    locked = false;
    // As `unlock` kicks a pass in Rust: a parked conflict waits for the key.
    if (connected && conflicted.size > 0) schedulePass();
  });
}

export function lockVault(): Promise<void> {
  locked = true;
  return Promise.resolve();
}

export function listEntries(): Promise<EntryHeader[]> {
  // Sorted because `Vault::list_entries` sorts. The Playwright suite only ever
  // sees this fallback, so a fallback that returned `Map` insertion order
  // would let an ordering bug in the app test green.
  //
  // Read when the call starts, not when it answers: the real listing reads the
  // disk while a save can still land after it, and a slow listing that read
  // late would hide every race with one.
  const headers = [...fallbackEntries.values()].map(toHeader).sort(byEntryDate);
  return run("list", () => headers);
}

export function getEntry(id: string): Promise<JournalEntry | null> {
  return run("get", () => fallbackEntries.get(id) ?? null);
}

/// `expected` is the `updated_at` the caller last saw, or `null` for a new
/// entry. Mirrors `Vault::save_entry_if`: refuses with `STALE_ENTRY_MESSAGE`
/// if the disk no longer holds that value, e.g. after `?sync=pull` rewrote it.
export function saveEntry(
  entry: JournalEntry,
  expected: string | null,
): Promise<void> {
  return run("save", () => {
    const onDisk = fallbackEntries.get(entry.id) ?? null;
    if ((onDisk?.updated_at ?? null) !== expected) throw STALE_ENTRY_MESSAGE;
    if (onDisk) pushHistory(entry.id, onDisk);
    fallbackEntries.set(entry.id, entry);
    markPending(entry.id);
  });
}

export function deleteEntry(id: string): Promise<void> {
  return run("delete", () => {
    fallbackEntries.delete(id);
    fallbackHistory.delete(id);
    for (const key of [...fallbackMedia.keys()]) {
      if (key.startsWith(`${id}/`)) fallbackMedia.delete(key);
    }
    markPending(id);
  });
}

// --- media -----------------------------------------------------------------
//
// Same pinned texts as the Rust side (`VaultError`).

const MEDIA_NOT_FOUND = "that image is no longer available";
const MEDIA_TYPE = "that kind of file cannot be added to an entry";
const MEDIA_TOO_LARGE = "that image is too large to store";
// Mirrors `MAX_ORIGINAL_BYTES`, `MAX_DISPLAY_BYTES` and `MAX_THUMB_BYTES`.
const MAX_MEDIA_BYTES: Record<MediaVariant, number> = {
  original: 50 * 1024 * 1024,
  display: 25 * 1024 * 1024,
  thumb: 1024 * 1024,
};

/// Mirrors `Vault::save_media`'s validation.
export function saveMedia(
  entryId: string,
  mediaId: MediaId,
  variant: MediaVariant,
  meta: MediaMeta,
  bytes: Uint8Array,
): Promise<void> {
  return run("save-media", () => {
    if (meta.kind !== "image" || meta.width === 0 || meta.height === 0)
      throw MEDIA_TYPE;
    if (
      (variant !== "original" && !isWebSafe(meta.format)) ||
      sniffImageFormat(bytes) !== meta.format
    )
      throw MEDIA_TYPE;
    if (bytes.length > MAX_MEDIA_BYTES[variant]) throw MEDIA_TOO_LARGE;
    if (variant === "thumb" && faults.thumbsOff) return;
    const key = mediaKey(entryId, mediaId, variant);
    // Immutable: an existing blob is a retry of the same bytes.
    if (!fallbackMedia.has(key)) fallbackMedia.set(key, { meta, bytes });
  });
}

export function getMedia(
  entryId: string,
  mediaId: MediaId,
  variant: MediaVariant,
): Promise<{ meta: MediaMeta; bytes: Uint8Array }> {
  return run(variant === "thumb" ? "get-thumb" : "get-media", () => {
    const found = fallbackMedia.get(mediaKey(entryId, mediaId, variant));
    if (!found) throw MEDIA_NOT_FOUND;
    return found;
  });
}

/// Mirrors `Vault::adopt_media`: for each foreign reference whose original
/// exists and whose thumbnail is here or whose original is web-safe, copies
/// every variant present under `entryId` and rewrites the link; other
/// references stay as they are.
export function adoptMedia(entryId: string, markdown: string): Promise<string> {
  return run("adopt-media", () => adoptMediaSync(entryId, markdown));
}

/// Mirrors `Vault::adoptable`.
function adoptable(entryId: string, mediaId: MediaId): boolean {
  const original = fallbackMedia.get(mediaKey(entryId, mediaId, "original"));
  if (!original) return false;
  if (fallbackMedia.has(mediaKey(entryId, mediaId, "thumb"))) return true;
  switch (original.meta.kind) {
    case "image":
      return isWebSafe(original.meta.format);
  }
}

function adoptMediaSync(entryId: string, markdown: string): string {
  let out = markdown;
  for (const ref of findMediaRefs(markdown).reverse()) {
    if (ref.entryId === entryId) continue;
    if (!adoptable(ref.entryId, ref.mediaId)) continue;
    for (const variant of MEDIA_VARIANTS) {
      const target = mediaKey(entryId, ref.mediaId, variant);
      const source = fallbackMedia.get(
        mediaKey(ref.entryId, ref.mediaId, variant),
      );
      if (source && !fallbackMedia.has(target))
        fallbackMedia.set(target, source);
    }
    out =
      out.slice(0, ref.start) +
      mediaRef(entryId, ref.mediaId) +
      out.slice(ref.end);
  }
  return out;
}

/// Headers of `id`'s past versions, most recent first.
export function listEntryHistory(id: string): Promise<EntryHeader[]> {
  const headers = (fallbackHistory.get(id) ?? []).map(toHeader);
  return run("history", () => headers);
}

/// One past version's full content, or `null` if none matches `at`.
export function getHistoryEntry(
  id: string,
  at: string,
): Promise<JournalEntry | null> {
  const found = (fallbackHistory.get(id) ?? []).find(
    (entry) => entry.updated_at === at,
  );
  return run("version", () => found ?? null);
}

/// Mirrors `Vault::restore_history`: refuses (as not-found) when there is no
/// live entry to restore into or no snapshot at `at`, and — like `saveEntry`
/// — checks the same staleness precondition against the live entry. Pushes
/// the pre-restore content into history first, unconditionally, mirroring
/// the "always snapshot on restore" rule, not the throttle
/// `pushHistory`'s callers otherwise skip anyway.
export function restoreEntryVersion(
  id: string,
  at: string,
  expected: string | null,
): Promise<JournalEntry> {
  return run("restore", () => {
    const live = fallbackEntries.get(id);
    if (!live) throw "that version of the entry is no longer available";
    if (live.updated_at !== expected) throw STALE_ENTRY_MESSAGE;
    const snapshot = (fallbackHistory.get(id) ?? []).find(
      (entry) => entry.updated_at === at,
    );
    if (!snapshot) throw "that version of the entry is no longer available";

    const restored: JournalEntry = {
      ...live,
      title: snapshot.title,
      markdown: snapshot.markdown,
      updated_at: new Date().toISOString(),
    };
    pushHistory(id, live);
    fallbackEntries.set(id, restored);
    markPending(id);
    return restored;
  });
}

/// The same matching `Vault::search` does, because the Playwright suite only
/// ever sees this one: case-insensitive substring over the title and the body,
/// an empty query matching nothing, results in the list's own order. A
/// fallback that matched titles alone would let the frontend pass the test
/// that exists to prove it does not have to.
export function searchEntries(query: string): Promise<SearchMatch[]> {
  const needle = query.trim().toLowerCase();
  const matches = !needle
    ? []
    : [...fallbackEntries.values()]
        .filter(
          (entry) =>
            entry.title.toLowerCase().includes(needle) ||
            entry.markdown.toLowerCase().includes(needle),
        )
        .sort(byEntryDate)
        .map((entry) => ({
          header: toHeader(entry),
          snippet: snippet(entry.markdown, needle),
        }));
  return run("search", () => matches);
}

/// What the native folder picker cannot be in a browser: a path to show, and
/// nothing behind it.
const EXPORT_DIRECTORY = "/home/you/Journal export";

export function chooseExportDirectory(): Promise<string | null> {
  return Promise.resolve(EXPORT_DIRECTORY);
}

/// Folders `exportEntries` was asked to write to, oldest first. Nothing is
/// written anywhere; this is how the Playwright suite tells an export that
/// ran from one that did not (`e2e/export.spec.ts`), which no page text can.
const exportedTo: string[] = [];

export function exportedDirectories(): readonly string[] {
  return exportedTo;
}

/// Resolves the count `export_entries` would: every entry there is, since it
/// writes the whole journal.
export function exportEntries(directory: string): Promise<number> {
  return run("export", () => {
    exportedTo.push(directory);
    return fallbackEntries.size;
  });
}

/// SNIPPET_CHARS characters of the body around the first match, whitespace
/// collapsed — `crates/vault/src/search.rs` in miniature.
function snippet(markdown: string, needle: string): string {
  const at = Math.max(0, markdown.toLowerCase().indexOf(needle));
  const start = Math.max(0, at - 24);
  const end = Math.min(markdown.length, start + 80);
  const text = markdown
    .slice(start, end)
    .split(/\s+/)
    .filter(Boolean)
    .join(" ");
  return `${start > 0 ? "…" : ""}${text}${end < markdown.length ? "…" : ""}`;
}

// --- simulated sync --------------------------------------------------------
//
// The same sequence the real backend produces (`src-tauri/src/sync`): a save
// marks its entry pending at once, a debounced pass uploads it, and the mark
// clears with a `synced` status. Off by default, like an app nobody has
// connected. From the URL:
//
//   ?sync=on          starts connected and synced
//   ?sync=pull        starts connected; the first successful pass only pulls in
//                     an edit to the first seed entry from "another device"
//                     (same id, the markdown "Rewritten on another device.", a
//                     fresh `updated_at`) and fires `sync://changed` — for GEN-1:
//                     a clean pane refreshes to show it, a dirty one meets Stale
//                     on its next save
//   ?sync=conflict    starts connected with one conflict parked, which the pass
//                     that unlocking starts settles as `resolver.rs` does: the
//                     newer version keeps the id, the older becomes a
//                     "(conflicted copy)"; ?fail=resolve keeps it parked, as
//                     the real resolver does with a version it cannot read
//   ?sync=busy        starts connected, and every pass reports that it brought
//                     changes from another device (`sync://changed`), so the
//                     list reloads after each one
//   ?sync=reauth      starts connected with a login the provider refuses: sync
//                     is halted until connecting again succeeds
//   ?sync=key_mismatch  starts connected to a cloud holding another journal's
//                     key: halted; disconnecting, or switching to the cloud's
//                     journal, ends it
//   ?sync=other_journal  starts off; connecting finds a cloud holding another
//                     journal, and asks before saving the login. Switching
//                     replaces the entries with the cloud's and locks
//   ?fail=adopt       switching to the cloud's journal is refused, and nothing
//                     changes; ?slow=adopt makes it resolve before the journal
//                     has arrived (it comes 1.5 s later), so the login screen
//                     can be seen waiting
//   ?fail=cloud_has_journal  the login screen's check of the cloud rejects
//                     while connected (nothing connected makes no call)
//   ?fail=sync        every pass fails: the ids stay pending, and the state is
//                     `offline` with the fault message
//   ?fail=connect     connecting is refused
//   ?slow=sync        a pass starts SLOW_MS after a save, long enough to look
//                     at a pending mark, and then takes SLOW_MS, showing
//                     `syncing`; ?slow=connect slows connecting
//
// A pass also fails for `sync_now`, which then rejects as the real one does.

const DROPBOX = { id: "dropbox", name: "Dropbox" };

/// Listed as coming later, as `PLANNED_PROVIDERS` does in Rust.
const PLANNED_PROVIDERS = [
  { id: "google_drive", name: "Google Drive" },
  { id: "icloud", name: "iCloud" },
  { id: "webdav", name: "WebDAV" },
];

/// A key resolves when the user entered one or the (simulated) build has one.
function dropboxConfigured(): boolean {
  return (
    deviceSettings.sync_providers.dropbox.app_key !== null || !faults.noBuildKey
  );
}

function providers(): ProviderInfo[] {
  return [
    { ...DROPBOX, available: true, configured: dropboxConfigured() },
    ...PLANNED_PROVIDERS.map((p) => ({
      ...p,
      available: false,
      configured: false,
    })),
  ];
}

/// How long after a save the simulated pass runs. The real one waits about two
/// seconds; a test has no use for that.
const PUSH_MS = 300;

/// What connecting finds in the cloud, as if another device had written it:
/// it reaches the list through `sync://changed`, not through a save here.
const cloudEntry: JournalEntry = {
  id: "c3d9a1f4-7b2e-4c8a-9e5d-3f1a6b2c0d03",
  title: "Written on the laptop",
  updated_at: "2026-09-05T21:10:00.000Z",
  entry_date: "2026-09-05T21:10:00.000Z",
  markdown: "# Written on the laptop\n\nThis one came from another device.",
};

/// The other device's version of the entry `?sync=conflict` parks: older than
/// the seed's, so the seed keeps the id and this becomes the copy.
const theirVersion: JournalEntry = {
  ...seedEntries[0],
  updated_at: "2026-08-31T20:00:00.000Z",
  markdown: "# First day back\n\nWritten on the phone, offline.",
};

let connected: { id: string; name: string } | null = null;
const pending = new Set<string>();
const conflicted = new Set<string>();
/// What halts sync until the user acts, with the words the backend would send
/// (`RemoteError::Unauthorized` and `SyncError::KeyMismatch`). A halted worker
/// runs no pass on its own; `sync_now` tries one, which fails the same way.
type Halt = "reauth" | "key_mismatch";
const HALT_MESSAGES: Record<Halt, string> = {
  reauth: "the remote rejected our credentials; reconnect to continue",
  key_mismatch:
    "the journal key on this device differs from the one in the cloud; sync is stopped until you choose which to keep",
};
let halted: Halt | null = null;
/// `?sync=other_journal`: a login granted but not saved, waiting for the user.
let pendingOther = false;
/// The cloud's journal is on its way after a `?slow=adopt` switch.
let arriving = false;
let arrivalTimer: ReturnType<typeof setTimeout> | undefined;
/// How long after a `?slow=adopt` switch the cloud's journal arrives.
const ARRIVAL_MS = 1500;

/// Whether `?sync=pull`'s one-shot remote edit has already landed, so a later
/// pass (a retry, `sync_now`, the debounce after a save) does not repeat it.
let pulled = false;

/// A pass is running, and a save during it asks for another.
let running = false;
let rerun = false;
let lastSyncedAt: string | null = null;
/// Why the last pass failed, if it did.
let failure: string | null = null;
let pushTimer: ReturnType<typeof setTimeout> | undefined;
const statusListeners = new Set<(status: SyncStatusPayload) => void>();
const changedListeners = new Set<() => void>();

if (faults.sync !== "off" && faults.sync !== "other_journal") {
  connected = DROPBOX;
  lastSyncedAt = new Date().toISOString();
  if (faults.sync === "conflict") conflicted.add(seedEntries[0].id);
  if (faults.sync === "reauth" || faults.sync === "key_mismatch") {
    halted = faults.sync;
  }
}

/// The status, derived in the order `derive_state` does in Rust (minus the
/// states this simulation never reaches): a running pass, then failure, then
/// conflicts, then pending, and `synced` when none applies.
function status(): SyncStatusPayload {
  const state = !connected
    ? "off"
    : halted
      ? halted
      : running
        ? "syncing"
        : failure
          ? "offline"
          : conflicted.size > 0
            ? "conflicts"
            : pending.size > 0
              ? "pending"
              : "synced";
  return {
    state,
    provider: connected?.name ?? null,
    provider_id: connected?.id ?? null,
    pending_ids: [...pending],
    conflicted_ids: [...conflicted],
    pending_other: 0,
    last_synced_at: lastSyncedAt,
    message: halted
      ? HALT_MESSAGES[halted]
      : state === "offline"
        ? failure
        : null,
  };
}

function emitStatus() {
  const current = status();
  statusListeners.forEach((listener) => listener(current));
}

/// Settles each parked conflict, as `VaultResolver` does, unless the vault is
/// locked (it needs the key) or `?fail=resolve` says it cannot read them.
/// Returns how many it settled.
function resolveConflicts(): number {
  if (locked || faults.fail.has("resolve")) return 0;
  let settled = 0;
  for (const id of conflicted) {
    const ours = fallbackEntries.get(id);
    if (ours) {
      const [winner, loser] =
        theirVersion.updated_at > ours.updated_at
          ? [theirVersion, ours]
          : [ours, theirVersion];
      const copyId = newEntryId();
      const copy: JournalEntry = {
        ...loser,
        id: copyId,
        title: `${loser.title} (conflicted copy)`,
        markdown: adoptMediaSync(copyId, loser.markdown),
      };
      fallbackEntries.set(copy.id, copy);
      fallbackEntries.set(id, winner);
    }
    conflicted.delete(id);
    settled += 1;
  }
  return settled;
}

/// `?sync=pull`'s one-shot simulated remote edit: another device rewrote the
/// first seed entry. Returns whether it did anything, which is what tells the
/// caller to fire `sync://changed`.
function pullRemoteEdit(): boolean {
  if (faults.sync !== "pull" || pulled) return false;
  const id = seedEntries[0].id;
  const current = fallbackEntries.get(id);
  if (!current) return false;
  pulled = true;
  fallbackEntries.set(id, {
    ...current,
    markdown: "Rewritten on another device.",
    updated_at: new Date().toISOString(),
  });
  return true;
}

/// One pass: uploads everything pending and settles what conflicts it can, or
/// fails, leaving all of it as it was. `syncing` for as long as it runs.
async function pass() {
  if (!connected || halted) return;
  if (running) {
    rerun = true;
    return;
  }
  running = true;
  emitStatus();
  if (faults.slow.has("sync")) {
    await new Promise((resolve) => setTimeout(resolve, faults.slowMs));
  }
  running = false;
  if (!connected) return;
  if (faults.fail.has("sync")) {
    failure = faults.failMessage;
  } else {
    failure = null;
    // A save during the pass is uploaded by the next one.
    if (!rerun) pending.clear();
    lastSyncedAt = new Date().toISOString();
    const settled = resolveConflicts();
    const pulledIn = pullRemoteEdit();
    if (settled > 0 || pulledIn || faults.sync === "busy") {
      changedListeners.forEach((listener) => listener());
    }
  }
  emitStatus();
  if (rerun) {
    rerun = false;
    schedulePass();
  }
}

/// Debounced, as the real one is: another save pushes the pass back.
function schedulePass() {
  clearTimeout(pushTimer);
  pushTimer = setTimeout(
    () => void pass(),
    faults.slow.has("sync") ? faults.slowMs : PUSH_MS,
  );
}

function markPending(id: string) {
  if (!connected) return;
  pending.add(id);
  emitStatus();
  if (running) {
    rerun = true;
  } else {
    schedulePass();
  }
}

export function syncStatus(): Promise<SyncStatusPayload> {
  return Promise.resolve(status());
}

export function listProviders(): Promise<ProviderInfo[]> {
  return Promise.resolve(providers());
}

export async function connectRemote(
  providerId: string,
): Promise<ConnectOutcome> {
  const outcome = await run("connect", (): ConnectOutcome => {
    const provider = providers().find((p) => p.id === providerId);
    if (!provider?.available) throw `unknown sync provider "${providerId}"`;
    if (!provider.configured) {
      throw "Dropbox has no app key: enter one in Settings › Sync";
    }
    // Nothing is saved: the choice is the user's (see `adoptRemoteJournal`).
    if (faults.sync === "other_journal" && !adopted) {
      pendingOther = true;
      return "different_journal";
    }
    connected = provider;
    failure = null;
    // A new login is what `reauth` waits for. A key mismatch is not about
    // the login, and a reconnect finds the same foreign key.
    if (halted === "reauth") halted = null;
    lastSyncedAt = new Date().toISOString();
    // The first pass, which pulls what another device left in the cloud: the
    // journal's key on a device that has none, and its entries.
    vaultPresent = true;
    if (!fallbackEntries.has(cloudEntry.id)) {
      fallbackEntries.set(cloudEntry.id, cloudEntry);
      changedListeners.forEach((listener) => listener());
    }
    emitStatus();
    return "connected";
  });
  if (outcome === "connected") {
    // Then a pass for what this device holds, as `connect` runs one before it
    // returns. Its failure is in the status; the connection itself succeeded.
    clearTimeout(pushTimer);
    await pass();
  }
  return outcome;
}

/// Whether the cloud's journal has replaced this one (`adoptRemoteJournal`).
let adopted = false;

export function adoptRemoteJournal(): Promise<AdoptedJournal> {
  return run("adopt", () => {
    if (!connected && !pendingOther) throw "Nothing is connected.";
    clearTimeout(pushTimer);
    clearTimeout(arrivalTimer);
    adopted = true;
    pendingOther = false;
    halted = null;
    failure = null;
    connected = DROPBOX;
    lastSyncedAt = new Date().toISOString();
    pending.clear();
    conflicted.clear();
    rerun = false;
    fallbackEntries.clear();
    fallbackEntries.set(cloudEntry.id, cloudEntry);
    // As the real backend: the vault is locked, and the user signs in with the
    // cloud journal's passphrase.
    locked = true;
    const ready = !faults.slow.has("adopt");
    if (!ready) {
      vaultPresent = false;
      arriving = true;
      arrivalTimer = setTimeout(() => {
        arriving = false;
        vaultPresent = true;
        changedListeners.forEach((listener) => listener());
      }, ARRIVAL_MS);
    } else {
      vaultPresent = true;
    }
    changedListeners.forEach((listener) => listener());
    emitStatus();
    return {
      backup_dir: "/fallback/entries.backup-20261001T000000Z",
      ready,
    };
  });
}

export function discardPendingConnection(): Promise<void> {
  pendingOther = false;
  return Promise.resolve();
}

export function cloudHasJournal(): Promise<boolean> {
  // Nothing connected: no cloud to ask, so nothing is on its way, and no
  // network call for a fault to hit.
  if (!connected) return Promise.resolve(false);
  return run("cloud_has_journal", () => {
    // Connecting always brings the key (see `connectRemote`), so a cloud with
    // no journal is only one that has nothing on its way.
    return arriving || vaultPresent;
  });
}

export function disconnectRemote(): Promise<void> {
  clearTimeout(pushTimer);
  // A journal still on its way is not coming any more.
  clearTimeout(arrivalTimer);
  arriving = false;
  pendingOther = false;
  rerun = false;
  halted = null;
  connected = null;
  failure = null;
  lastSyncedAt = null;
  pending.clear();
  conflicted.clear();
  emitStatus();
  return Promise.resolve();
}

export async function syncNow(): Promise<void> {
  if (!connected) throw "sync is not connected";
  if (halted) throw HALT_MESSAGES[halted];
  clearTimeout(pushTimer);
  await pass();
  if (failure) throw failure;
}

export function onSyncStatus(
  handler: (status: SyncStatusPayload) => void,
): () => void {
  statusListeners.add(handler);
  return () => statusListeners.delete(handler);
}

export function onSyncChanged(handler: () => void): () => void {
  changedListeners.add(handler);
  return () => changedListeners.delete(handler);
}

// --- settings ----------------------------------------------------------
//
// Mirrors the split in `journal-settings`: the device half
// persists to `localStorage["journal.fallback.device"]`, sanitised field by
// field on read so garbage in storage falls back to a default rather than
// breaking the app — the same tolerance `decode_tolerant` gives the real
// backend. The journal half lives only in memory, like `fallbackEntries`,
// and reads as defaults while `locked`, matching `get_settings`.

const DEVICE_STORAGE_KEY = "journal.fallback.device";

const DEFAULT_DEVICE_SETTINGS: DeviceSettings = {
  color_mode: "system",
  color_scheme: "parchment",
  idle_lock_minutes: 5,
  sync_interval_minutes: 1,
  shortcuts: {},
  sync_providers: { dropbox: { app_key: null } },
};

const DEFAULT_JOURNAL_SETTINGS: JournalSettings = {
  week_starts_on: "sunday",
  time_format: "locale",
  entry_template: "",
};

const COLOR_MODES = new Set(["system", "light", "dark"]);
const COLOR_SCHEMES = new Set([
  "parchment",
  "slate",
  "ink",
  "catppuccin",
  "contrast",
]);
const WEEK_STARTS = new Set(["sunday", "monday", "saturday"]);
const TIME_FORMATS = new Set(["locale", "h12", "h24"]);

/// `undefined` means "not present or not valid": the caller falls back to the
/// field's default rather than failing the whole read, the way a single bad
/// field in a stored blob does not take the others down with it.
function readMinutesOrDefault(
  value: unknown,
  max: number,
): number | null | undefined {
  if (value === null) return null;
  if (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 1 &&
    value <= max
  ) {
    return value;
  }
  return undefined;
}

const APP_KEY = /^[A-Za-z0-9]{1,64}$/;
const APP_KEY_ERROR = "the Dropbox app key must be 1 to 64 letters and digits";

/// `undefined` means "not present or not valid", as for the other readers.
function readSyncProviders(raw: unknown): SyncProviders | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const dropbox = (raw as Record<string, unknown>).dropbox;
  if (dropbox === undefined) return { dropbox: { app_key: null } };
  if (typeof dropbox !== "object" || dropbox === null) return undefined;
  const key = (dropbox as Record<string, unknown>).app_key ?? null;
  if (key === null) return { dropbox: { app_key: null } };
  if (typeof key !== "string" || !APP_KEY.test(key)) return undefined;
  return { dropbox: { app_key: key } };
}

/// `idle_lock_minutes`/`sync_interval_minutes`/`shortcuts`' size limits are
/// specified in bytes, not UTF-16 code
/// units — a chord is ASCII in practice, but an id or value that wasn't
/// would otherwise pass a `.length` check the real backend refuses.
function byteLength(s: string): number {
  return new TextEncoder().encode(s).length;
}

/// Tolerant, for `sanitizeDevice` (loading from storage): drops whatever
/// doesn't fit rather than failing the whole read, and caps at 64 entries
/// the same way the strict path does (see `assertShortcuts`), so a corrupted
/// or hand-edited `localStorage` value can't grow the map without bound.
function readShortcutsOrDefault(
  value: unknown,
): Record<string, string | null> | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return undefined;
  }
  const result: Record<string, string | null> = {};
  for (const [id, chord] of Object.entries(
    value as Record<string, unknown>,
  ).slice(0, 64)) {
    if (typeof id !== "string" || byteLength(id) > 64) continue;
    if (chord === null) {
      result[id] = null;
    } else if (typeof chord === "string" && byteLength(chord) <= 64) {
      result[id] = chord;
    }
  }
  return result;
}

/// Strict, for `applyPatch`: like Rust's `apply_patch`, an invalid entry
/// refuses the whole patch rather than being dropped — the 64-entry cap is
/// checked before looking at any entry, so a patch just over the limit is
/// refused outright rather than silently truncated.
function assertShortcuts(value: unknown): Record<string, string | null> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw "invalid value for shortcuts";
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length > 64) throw "shortcuts: at most 64 entries";
  const result: Record<string, string | null> = {};
  for (const [id, chord] of entries) {
    if (byteLength(id) > 64) {
      throw `shortcuts: "${id}" is too long`;
    }
    if (chord === null) {
      result[id] = null;
    } else if (typeof chord === "string" && byteLength(chord) <= 64) {
      result[id] = chord;
    } else {
      throw `shortcuts: invalid value for "${id}"`;
    }
  }
  return result;
}

/// Tolerant, like `decode_tolerant`: a missing or malformed field takes its
/// default, one field at a time, rather than the whole read failing.
function sanitizeDevice(raw: unknown): DeviceSettings {
  if (typeof raw !== "object" || raw === null) {
    return { ...DEFAULT_DEVICE_SETTINGS };
  }
  const obj = raw as Record<string, unknown>;
  const colorMode =
    typeof obj.color_mode === "string" && COLOR_MODES.has(obj.color_mode)
      ? (obj.color_mode as DeviceSettings["color_mode"])
      : DEFAULT_DEVICE_SETTINGS.color_mode;
  const colorScheme =
    typeof obj.color_scheme === "string" && COLOR_SCHEMES.has(obj.color_scheme)
      ? (obj.color_scheme as DeviceSettings["color_scheme"])
      : DEFAULT_DEVICE_SETTINGS.color_scheme;
  const idleLockMinutes = readMinutesOrDefault(obj.idle_lock_minutes, 240);
  const syncIntervalMinutes = readMinutesOrDefault(
    obj.sync_interval_minutes,
    1440,
  );
  const shortcuts =
    readShortcutsOrDefault(obj.shortcuts) ?? DEFAULT_DEVICE_SETTINGS.shortcuts;
  return {
    color_mode: colorMode,
    color_scheme: colorScheme,
    idle_lock_minutes:
      idleLockMinutes === undefined
        ? DEFAULT_DEVICE_SETTINGS.idle_lock_minutes
        : idleLockMinutes,
    sync_interval_minutes:
      syncIntervalMinutes === undefined
        ? DEFAULT_DEVICE_SETTINGS.sync_interval_minutes
        : syncIntervalMinutes,
    shortcuts,
    sync_providers:
      readSyncProviders(obj.sync_providers) ??
      DEFAULT_DEVICE_SETTINGS.sync_providers,
  };
}

function loadDeviceSettings(): DeviceSettings {
  if (typeof window === "undefined") return { ...DEFAULT_DEVICE_SETTINGS };
  try {
    const raw = window.localStorage.getItem(DEVICE_STORAGE_KEY);
    if (raw === null) return { ...DEFAULT_DEVICE_SETTINGS };
    return sanitizeDevice(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_DEVICE_SETTINGS };
  }
}

function saveDeviceSettings(device: DeviceSettings) {
  try {
    window.localStorage.setItem(DEVICE_STORAGE_KEY, JSON.stringify(device));
  } catch {
    // Best effort, as elsewhere in this file: the in-memory copy still
    // answers for the rest of the session, it just won't survive a reload.
  }
}

let deviceSettings: DeviceSettings = loadDeviceSettings();
let journalSettings: JournalSettings = { ...DEFAULT_JOURNAL_SETTINGS };

const DEVICE_KEYS = new Set<string>([
  "color_mode",
  "color_scheme",
  "idle_lock_minutes",
  "sync_interval_minutes",
  "shortcuts",
  "sync_providers",
]);
const JOURNAL_KEYS = new Set<string>([
  "week_starts_on",
  "time_format",
  "entry_template",
]);

/// Strict, like `apply_patch`: an unknown key or a value that fails
/// validation refuses the whole patch, and nothing is written.
function applyPatch(patch: SettingsPatch): {
  devicePatch: Partial<DeviceSettings> | null;
  journalPatch: Partial<JournalSettings> | null;
  /// True when the patch names any journal key, even one whose value would
  /// come out unchanged — the locked check goes by this, not by whether the
  /// journal half actually changed.
  namesJournal: boolean;
} {
  let devicePatch: Record<string, unknown> | null = null;
  let journalPatch: Record<string, unknown> | null = null;
  let namesJournal = false;

  for (const [key, value] of Object.entries(patch)) {
    if (JOURNAL_KEYS.has(key)) {
      namesJournal = true;
      journalPatch ??= {};
      if (key === "week_starts_on") {
        if (typeof value !== "string" || !WEEK_STARTS.has(value)) {
          throw `invalid value for week_starts_on: ${JSON.stringify(value)}`;
        }
        journalPatch[key] = value;
      } else if (key === "time_format") {
        if (typeof value !== "string" || !TIME_FORMATS.has(value)) {
          throw `invalid value for time_format: ${JSON.stringify(value)}`;
        }
        journalPatch[key] = value;
      } else if (key === "entry_template") {
        if (typeof value !== "string" || value.length > 10_000) {
          throw "the entry template must be at most 10000 characters";
        }
        journalPatch[key] = value;
      }
    } else if (DEVICE_KEYS.has(key)) {
      devicePatch ??= {};
      if (key === "color_mode") {
        if (typeof value !== "string" || !COLOR_MODES.has(value)) {
          throw `invalid value for color_mode: ${JSON.stringify(value)}`;
        }
        devicePatch[key] = value;
      } else if (key === "color_scheme") {
        if (typeof value !== "string" || !COLOR_SCHEMES.has(value)) {
          throw `invalid value for color_scheme: ${JSON.stringify(value)}`;
        }
        devicePatch[key] = value;
      } else if (key === "idle_lock_minutes") {
        if (value !== null && !readMinutesOrDefault(value, 240)) {
          throw "the idle lock must be between 1 and 240 minutes";
        }
        devicePatch[key] = value;
      } else if (key === "sync_interval_minutes") {
        if (value !== null && !readMinutesOrDefault(value, 1440)) {
          throw "the sync interval must be between 1 and 1440 minutes";
        }
        devicePatch[key] = value;
      } else if (key === "shortcuts") {
        devicePatch[key] = assertShortcuts(value);
      } else if (key === "sync_providers") {
        const valid = readSyncProviders(value);
        if (!valid) throw APP_KEY_ERROR;
        devicePatch[key] = valid;
      }
    } else {
      throw `unknown setting: "${key}"`;
    }
  }

  return {
    devicePatch: devicePatch as Partial<DeviceSettings> | null,
    journalPatch: journalPatch as Partial<JournalSettings> | null,
    namesJournal,
  };
}

function currentSettings(): Settings {
  return {
    ...deviceSettings,
    ...(locked ? DEFAULT_JOURNAL_SETTINGS : journalSettings),
  };
}

/// Count of `getSettings` calls, for `settings-store.spec.ts` to check that
/// `AppStateContext` actually reloads after a lock and after an unlock,
/// rather than just reading its own settled state — which would pass even
/// without those `reload()` calls, since the fallback's journal half never
/// leaves memory.
let settingsLoads = 0;

export function getSettings(): Promise<Settings> {
  settingsLoads++;
  return run("settings-load", currentSettings);
}

/// Whether the next `updateSettings` call is the first one since page load,
/// for `?slow=settings-first`. Module-scope, so it resets on a reload like
/// every other piece of state here.
let firstUpdateSettings = true;

/// Every `updateSettings` call that reached this function, in the order it
/// started running, with a real timestamp — not when `SettingsContext`
/// queued it, which under `?slow=settings-first` can be much later than when
/// the user acted. `settings-store.spec.ts` reads this (via
/// `__settingsTestHooks`, below) to check that a second update is not sent
/// until the first has settled.
const settingsCalls: { patch: SettingsPatch; at: number }[] = [];

export async function updateSettings(patch: SettingsPatch): Promise<Settings> {
  settingsCalls.push({ patch, at: Date.now() });
  const first = firstUpdateSettings;
  firstUpdateSettings = false;

  if (
    faults.slow.has("settings") ||
    (first && faults.slow.has("settings-first"))
  ) {
    await new Promise((resolve) => setTimeout(resolve, faults.slowMs));
  }
  if (faults.fail.has("settings")) {
    const seen = failedCalls.get("settings") ?? 0;
    failedCalls.set("settings", seen + 1);
    if (
      seen >= faults.failAfter &&
      seen < faults.failAfter + faults.failTimes
    ) {
      throw faults.failMessage;
    }
  }

  const { devicePatch, journalPatch, namesJournal } = applyPatch(patch);
  if (namesJournal && locked) throw "vault is locked";

  if (journalPatch) journalSettings = { ...journalSettings, ...journalPatch };
  if (devicePatch) {
    deviceSettings = { ...deviceSettings, ...devicePatch };
    saveDeviceSettings(deviceSettings);
  }
  return currentSettings();
}

// `settings-store.spec.ts` needs one thing no UI exposes, even now that
// the Settings page can write and read every setting through the ordinary
// controls: the backend's own settings, bypassing the frontend's optimistic
// overlay (`confirmed ⊕ pending` in `SettingsContext`) — needed because the
// Settings page sits behind unlock, so nothing on
// screen can show what the backend holds while the vault is locked. `calls`
// and `loads` back the same file's checks that the request queue and the
// lock/unlock reloads actually ran, which reading rendered text can't prove
// either. Lives only on `window`, and only in this DEV-only chunk — `make
// check-no-fallback` greps the production bundle for other constants in this
// file, so this never ships either.
declare global {
  interface Window {
    __settingsTestHooks?: {
      current(): Settings;
      calls: { patch: SettingsPatch; at: number }[];
      /// Live count of `getSettings` calls (a function, not a number, so it
      /// reflects calls made after this hook was installed).
      loads(): number;
    };
    /// Runs a sync pass without leaving the page: the Sync section is a
    /// route of its own, so a spec that needs a pass to land while an entry
    /// is open cannot press its button.
    __syncTestHooks?: { syncNow(): Promise<void> };
  }
}

if (typeof window !== "undefined") {
  window.__settingsTestHooks = {
    current: currentSettings,
    calls: settingsCalls,
    loads: () => settingsLoads,
  };
  window.__syncTestHooks = { syncNow };
}
