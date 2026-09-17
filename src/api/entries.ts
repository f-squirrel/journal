import { invoke, isTauri } from "@tauri-apps/api/core";
import type { EntryHeader, JournalEntry } from "../types";

// This module is the only place that knows whether a Rust backend exists.
//
// Inside Tauri every call goes over `invoke` and rejections propagate
// untouched. Outside it (`npm run dev` in a browser, and the Playwright
// suite) there is no vault to talk to, so the module serves the seeded
// in-memory entries below. Callers therefore never branch on `isTauri()`:
// any rejection they see is a real backend failure.

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

// --- fault injection (browser fallback only) -------------------------------
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
// `page.goto("/?fail=save")`, not `/#/?fail=save`. This is parsed once at
// module load and only when there is no Tauri backend, so a shipped build
// cannot be steered by a URL.

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
  const ops = (value: string | null) => new Set((value ?? "").split(",").filter(Boolean) as Op[]);
  return {
    newVault: params.get("vault") === "new",
    fail: ops(params.get("fail")),
    failMessage: params.get("failMessage") ?? "Injected fault.",
    slow: ops(params.get("slow")),
  };
}

const backend = isTauri();
const faults: Faults = backend
  ? { newVault: false, fail: new Set(), failMessage: "", slow: new Set() }
  : parseFaults(window.location.search);
const fallbackEntries = new Map(seedEntries.map((entry) => [entry.id, entry]));

/// Runs one fallback operation, honouring any fault injected via the URL.
/// Rejects with a bare string, the way `invoke` rejects with the Rust
/// command's `Err(String)`.
async function fallback<T>(op: Op, run: () => T): Promise<T> {
  if (faults.slow.has(op)) {
    await new Promise((resolve) => setTimeout(resolve, SLOW_MS));
  }
  if (faults.fail.has(op)) {
    throw faults.failMessage;
  }
  return run();
}

// --- commands --------------------------------------------------------------

export function hasVault(): Promise<boolean> {
  if (!backend) return fallback("has", () => !faults.newVault);
  return invoke("has_vault");
}

export function unlockVault(passphrase: string): Promise<void> {
  if (!backend) return fallback("unlock", () => undefined);
  return invoke("unlock", { passphrase });
}

export function lockVault(): Promise<void> {
  if (!backend) return Promise.resolve();
  return invoke("lock");
}

export function listEntries(): Promise<EntryHeader[]> {
  if (!backend) return fallback("list", () => [...fallbackEntries.values()].map(toHeader));
  return invoke("list_entries");
}

export function getEntry(id: string): Promise<JournalEntry | null> {
  if (!backend) return fallback("get", () => fallbackEntries.get(id) ?? null);
  return invoke("get_entry", { id });
}

export function saveEntry(entry: JournalEntry): Promise<void> {
  if (!backend) {
    return fallback("save", () => {
      fallbackEntries.set(entry.id, entry);
    });
  }
  return invoke("save_entry", { entry });
}

export function deleteEntry(id: string): Promise<void> {
  if (!backend) {
    return fallback("delete", () => {
      fallbackEntries.delete(id);
    });
  }
  return invoke("delete_entry", { id });
}
