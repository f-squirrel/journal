import { invoke } from "@tauri-apps/api/core";
import type { EntryHeader, JournalEntry, SearchMatch } from "../types";
import { backend, fallback } from "./backend";

// Every call goes over `invoke` inside Tauri and to the in-memory fallback
// outside it; `./backend` is where that is decided (see there), so callers
// never branch on it and any rejection they see is a real backend failure.

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

/// Matches the query against every entry's title *and* body, which is why it
/// is a command and not a filter over the headers the app already holds: those
/// carry no markdown. The backend decrypts the journal to answer, so this is
/// the slowest call here — callers debounce it.
export async function searchEntries(query: string): Promise<SearchMatch[]> {
  if (backend) return invoke("search_entries", { query });
  return (await fallback()).searchEntries(query);
}
