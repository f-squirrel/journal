import type { EntryHeader, JournalEntry } from "../types";
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
//   ?fail=<ops>         comma-separated: has, unlock, list, get, save, delete
//   ?failMessage=<text> the rejection message (default "Injected fault.")
//   ?slow=<ops>         the named ops resolve after SLOW_MS
//
// The app uses HashRouter, so the query string goes *before* the hash:
// `page.goto("/?fail=save")`, not `/#/?fail=save`.

type Op = "has" | "unlock" | "list" | "get" | "save" | "delete";

const SLOW_MS = 1200;

interface Faults {
  newVault: boolean;
  fail: Set<Op>;
  failMessage: string;
  slow: Set<Op>;
}

function parseFaults(search: string): Faults {
  const params = new URLSearchParams(search);
  const ops = (value: string | null) =>
    new Set((value ?? "").split(",").filter(Boolean) as Op[]);
  return {
    newVault: params.get("vault") === "new",
    fail: ops(params.get("fail")),
    failMessage: params.get("failMessage") ?? "Injected fault.",
    slow: ops(params.get("slow")),
  };
}

const faults: Faults = parseFaults(window.location.search);
const fallbackEntries = new Map(seedEntries.map((entry) => [entry.id, entry]));

// Locked until `unlockVault`, like `VaultState`. Without it a write made after
// the lock succeeds here and is refused by the real backend, so nothing in the
// Playwright suite could catch a draft lost that way.
let locked = true;

/// Runs one fallback operation, honouring any fault injected via the URL.
/// Rejects with a bare string, the way `invoke` rejects with the Rust
/// command's `Err(String)`.
async function run<T>(op: Op, work: () => T): Promise<T> {
  // Checked up front, before any delay: the Rust commands clone the vault
  // out of `VaultState` at the start, so a write already running when the
  // lock lands still completes.
  if (locked && op !== "has" && op !== "unlock") {
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
  return run("has", () => !faults.newVault);
}

export function unlockVault(_passphrase: string): Promise<void> {
  return run("unlock", () => {
    locked = false;
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
  return run("list", () =>
    [...fallbackEntries.values()].map(toHeader).sort(byUpdatedAt),
  );
}

export function getEntry(id: string): Promise<JournalEntry | null> {
  return run("get", () => fallbackEntries.get(id) ?? null);
}

export function saveEntry(entry: JournalEntry): Promise<void> {
  return run("save", () => {
    fallbackEntries.set(entry.id, entry);
  });
}

export function deleteEntry(id: string): Promise<void> {
  return run("delete", () => {
    fallbackEntries.delete(id);
  });
}
