import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
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

/// The folder the user picks for an export, or `null` if they cancelled. The
/// native picker is a Tauri plugin (`dialog:allow-open` is the only permission
/// it is granted); outside Tauri the fallback answers with a made-up path,
/// since a browser has no folder to hand back.
export async function chooseExportDirectory(): Promise<string | null> {
  if (backend) {
    return open({
      directory: true,
      multiple: false,
      title: "Choose an empty folder for the export",
    });
  }
  return (await fallback()).chooseExportDirectory();
}

/// Writes every entry to `directory` as plaintext markdown and resolves to how
/// many were written. **Unencrypted** — the one call here that puts the journal
/// on disk in the clear — so callers say so before making it. Rejects with why
/// the folder was refused (not empty, inside the app's data) or the write
/// failed.
export async function exportEntries(directory: string): Promise<number> {
  if (backend) return invoke("export_entries", { directory });
  return (await fallback()).exportEntries(directory);
}
