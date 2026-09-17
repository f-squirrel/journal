import { invoke, isTauri } from "@tauri-apps/api/core";
import type { EntryHeader, JournalEntry } from "../types";

// This module is the only place that knows whether a Rust backend exists.
//
// Inside Tauri every call goes over `invoke` and rejections propagate
// untouched. Outside it (`npm run dev` in a browser, and the Playwright
// suite) there is no vault to talk to, so a development build serves the
// in-memory fallback in `./fallback` instead. Callers therefore never
// branch on `isTauri()`: any rejection they see is a real backend failure.
//
// A *production* build has no fallback to serve. Fabricating entries there
// would show a user two entries they never wrote and report every save as
// having succeeded while it went to a `Map` that dies with the page, so
// this fails loudly instead.

const backend = isTauri();

function noBackend(): never {
  throw new Error("This build must run inside the Journal app.");
}

/// The in-memory fallback, loaded on demand.
///
/// The `import()` must stay textually inside the `import.meta.env.DEV`
/// branch: the constant is replaced with `false` in a production build, so
/// the branch is dead code and the whole chunk — seed entries, fault hook
/// and all — is dropped. `make check-no-fallback` greps the bundle for
/// exactly that. A form the bundler cannot prove dead (a helper called
/// only from dead branches, a `new Map(...)` at this module's scope) ships
/// the fallback, which is how this was broken before.
function fallback(): Promise<typeof import("./fallback")> {
  if (import.meta.env.DEV) return import("./fallback");
  return noBackend();
}

export async function hasVault(): Promise<boolean> {
  if (backend) return invoke("has_vault");
  return (await fallback()).hasVault();
}

export async function unlockVault(passphrase: string): Promise<void> {
  if (backend) return invoke("unlock", { passphrase });
  return (await fallback()).unlockVault(passphrase);
}

export async function lockVault(): Promise<void> {
  if (backend) return invoke("lock");
  return (await fallback()).lockVault();
}

export async function listEntries(): Promise<EntryHeader[]> {
  if (backend) return invoke("list_entries");
  return (await fallback()).listEntries();
}

export async function getEntry(id: string): Promise<JournalEntry | null> {
  if (backend) return invoke("get_entry", { id });
  return (await fallback()).getEntry(id);
}

export async function saveEntry(entry: JournalEntry): Promise<void> {
  if (backend) return invoke("save_entry", { entry });
  return (await fallback()).saveEntry(entry);
}

export async function deleteEntry(id: string): Promise<void> {
  if (backend) return invoke("delete_entry", { id });
  return (await fallback()).deleteEntry(id);
}
