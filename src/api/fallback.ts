import type {
  EntryHeader,
  JournalEntry,
  ProviderInfo,
  SyncStatusPayload,
} from "../types";
import { byUpdatedAt } from "../utils/date";

// The browser fallback: an in-memory vault for `npm run dev` and the
// Playwright suite, where there is no Rust backend to talk to.
//
// This module is reached only through a dynamic `import()` that
// `src/api/entries.ts` guards with `import.meta.env.DEV`, so none of it —
// not the fabricated entries below, not the URL fault hook — exists in a
// production bundle. `make check-no-fallback` is what holds that: the
// guard is only as good as the bundler dropping this chunk. Keep the
// import dynamic, and keep every module-scope constant here rather than
// in `entries.ts`, or the two come back.

const seedEntries: JournalEntry[] = [
  {
    id: "5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01",
    title: "First day back",
    updated_at: "2026-09-01T08:15:00.000Z",
    markdown:
      "# First day back\n\nStarted journaling again. Feels good to write things down.\n\n- Coffee\n- Long walk\n- Read a chapter",
  },
  {
    id: "8a2e4d3b-2b7a-4f1e-8c3d-2e7a4b1d0a02",
    title: "Project ideas",
    updated_at: "2026-09-03T19:42:00.000Z",
    markdown:
      "# Project ideas\n\nSome things I want to build:\n\n1. A journaling app\n2. A recipe tracker\n\n> Ideas are cheap, execution is everything.",
  },
];

function toHeader(entry: JournalEntry): EntryHeader {
  return { id: entry.id, title: entry.title, updated_at: entry.updated_at };
}

// --- fault injection -------------------------------------------------------
//
// The fallback cannot fail on its own, and Playwright never crosses the
// `invoke` bridge, so backend failures have to be injectable from the URL:
//
//   ?vault=new          hasVault() resolves false, giving the create-vault UI
//   ?fail=<ops>         comma-separated: has, unlock, list, get, save, delete,
//                       connect, sync, resolve (see "simulated sync" below)
//   ?failMessage=<text> the rejection message (default "Injected fault.")
//   ?slow=<ops>         the named ops resolve after SLOW_MS
//   ?sync=on|conflict|busy  start connected to a provider (see below)
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
  | "connect"
  | "sync"
  | "resolve";

const SLOW_MS = 1200;

interface Faults {
  newVault: boolean;
  fail: Set<Op>;
  failMessage: string;
  slow: Set<Op>;
  sync: SyncMode;
}

type SyncMode = "off" | "on" | "conflict" | "busy";
const SYNC_MODES: readonly SyncMode[] = ["off", "on", "conflict", "busy"];

function parseFaults(search: string): Faults {
  const params = new URLSearchParams(search);
  const ops = (value: string | null) =>
    new Set((value ?? "").split(",").filter(Boolean) as Op[]);
  return {
    newVault: params.get("vault") === "new",
    fail: ops(params.get("fail")),
    failMessage: params.get("failMessage") ?? "Injected fault.",
    slow: ops(params.get("slow")),
    sync: SYNC_MODES.find((mode) => mode === params.get("sync")) ?? "off",
  };
}

const faults: Faults = parseFaults(window.location.search);
const fallbackEntries = new Map(seedEntries.map((entry) => [entry.id, entry]));

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
  if (locked && op !== "has" && op !== "unlock" && op !== "connect") {
    throw "vault is locked";
  }
  if (faults.slow.has(op)) {
    await new Promise((resolve) => setTimeout(resolve, SLOW_MS));
  }
  if (faults.fail.has(op)) {
    throw faults.failMessage;
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
  const headers = [...fallbackEntries.values()].map(toHeader).sort(byUpdatedAt);
  return run("list", () => headers);
}

export function getEntry(id: string): Promise<JournalEntry | null> {
  return run("get", () => fallbackEntries.get(id) ?? null);
}

export function saveEntry(entry: JournalEntry): Promise<void> {
  return run("save", () => {
    fallbackEntries.set(entry.id, entry);
    markPending(entry.id);
  });
}

export function deleteEntry(id: string): Promise<void> {
  return run("delete", () => {
    fallbackEntries.delete(id);
    markPending(id);
  });
}

// --- simulated sync --------------------------------------------------------
//
// The same sequence the real backend produces (`src-tauri/src/sync`): a save
// marks its entry pending at once, a debounced pass uploads it, and the mark
// clears with a `synced` status. Off by default, like an app nobody has
// connected. From the URL:
//
//   ?sync=on          starts connected and synced
//   ?sync=conflict    starts connected with one conflict parked, which the pass
//                     that unlocking starts settles as `resolver.rs` does: the
//                     newer version keeps the id, the older becomes a
//                     "(conflicted copy)"; ?fail=resolve keeps it parked, as
//                     the real resolver does with a version it cannot read
//   ?sync=busy        starts connected, and every pass reports that it brought
//                     changes from another device (`sync://changed`), so the
//                     list reloads after each one
//   ?fail=sync        every pass fails: the ids stay pending, and the state is
//                     `offline` with the fault message
//   ?fail=connect     connecting is refused
//   ?slow=sync        a pass starts SLOW_MS after a save, long enough to look
//                     at a pending mark, and then takes SLOW_MS, showing
//                     `syncing`; ?slow=connect slows connecting
//
// A pass also fails for `sync_now`, which then rejects as the real one does.

const providers: ProviderInfo[] = [{ id: "dropbox", name: "Dropbox" }];

/// How long after a save the simulated pass runs. The real one waits about two
/// seconds; a test has no use for that.
const PUSH_MS = 300;

/// What connecting finds in the cloud, as if another device had written it:
/// it reaches the list through `sync://changed`, not through a save here.
const cloudEntry: JournalEntry = {
  id: "c3d9a1f4-7b2e-4c8a-9e5d-3f1a6b2c0d03",
  title: "Written on the laptop",
  updated_at: "2026-09-05T21:10:00.000Z",
  markdown: "# Written on the laptop\n\nThis one came from another device.",
};

/// The other device's version of the entry `?sync=conflict` parks: older than
/// the seed's, so the seed keeps the id and this becomes the copy.
const theirVersion: JournalEntry = {
  ...seedEntries[0],
  updated_at: "2026-08-31T20:00:00.000Z",
  markdown: "# First day back\n\nWritten on the phone, offline.",
};

let connected: ProviderInfo | null = null;
const pending = new Set<string>();
const conflicted = new Set<string>();
/// A pass is running, and a save during it asks for another.
let running = false;
let rerun = false;
let lastSyncedAt: string | null = null;
/// Why the last pass failed, if it did.
let failure: string | null = null;
let pushTimer: ReturnType<typeof setTimeout> | undefined;
const statusListeners = new Set<(status: SyncStatusPayload) => void>();
const changedListeners = new Set<() => void>();

if (faults.sync !== "off") {
  connected = providers[0];
  lastSyncedAt = new Date().toISOString();
  if (faults.sync === "conflict") conflicted.add(seedEntries[0].id);
}

/// The status, derived in the order `derive_state` does in Rust (minus the
/// states this simulation never reaches): a running pass, then failure, then
/// conflicts, then pending, and `synced` when none applies.
function status(): SyncStatusPayload {
  const state = !connected
    ? "off"
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
    pending_ids: [...pending],
    conflicted_ids: [...conflicted],
    pending_other: 0,
    last_synced_at: lastSyncedAt,
    message: state === "offline" ? failure : null,
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
      const copy: JournalEntry = {
        ...loser,
        id: crypto.randomUUID(),
        title: `${loser.title} (conflicted copy)`,
      };
      fallbackEntries.set(copy.id, copy);
      fallbackEntries.set(id, winner);
    }
    conflicted.delete(id);
    settled += 1;
  }
  return settled;
}

/// One pass: uploads everything pending and settles what conflicts it can, or
/// fails, leaving all of it as it was. `syncing` for as long as it runs.
async function pass() {
  if (!connected) return;
  if (running) {
    rerun = true;
    return;
  }
  running = true;
  emitStatus();
  if (faults.slow.has("sync")) {
    await new Promise((resolve) => setTimeout(resolve, SLOW_MS));
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
    if (settled > 0 || faults.sync === "busy") {
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
    faults.slow.has("sync") ? SLOW_MS : PUSH_MS,
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
  return Promise.resolve(providers);
}

export function connectRemote(providerId: string): Promise<void> {
  return run("connect", () => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) throw `unknown sync provider "${providerId}"`;
    connected = provider;
    failure = null;
    lastSyncedAt = new Date().toISOString();
    // The first pass, which pulls what another device left in the cloud: the
    // journal's key on a device that has none, and its entries.
    vaultPresent = true;
    if (!fallbackEntries.has(cloudEntry.id)) {
      fallbackEntries.set(cloudEntry.id, cloudEntry);
      changedListeners.forEach((listener) => listener());
    }
    emitStatus();
  });
}

export function disconnectRemote(): Promise<void> {
  clearTimeout(pushTimer);
  rerun = false;
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
