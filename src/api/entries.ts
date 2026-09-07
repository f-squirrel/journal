import { invoke } from "@tauri-apps/api/core";
import type { EntryHeader, JournalEntry } from "../types";

export function hasVault(): Promise<boolean> {
  return invoke("has_vault");
}

export function unlockVault(passphrase: string): Promise<void> {
  return invoke("unlock", { passphrase });
}

export function lockVault(): Promise<void> {
  return invoke("lock");
}

export function listEntries(): Promise<EntryHeader[]> {
  return invoke("list_entries");
}

export function getEntry(id: string): Promise<JournalEntry | null> {
  return invoke("get_entry", { id });
}

export function saveEntry(entry: JournalEntry): Promise<void> {
  return invoke("save_entry", { entry });
}

export function deleteEntry(id: string): Promise<void> {
  return invoke("delete_entry", { id });
}
