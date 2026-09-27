import { invoke } from "@tauri-apps/api/core";
import type { Settings, SettingsPatch } from "../types";
import { backend, fallback } from "./backend";

// Wrapped like `./entries`: `invoke` inside Tauri, the in-memory fallback
// outside it. See `./backend` for why callers never branch on `backend`
// themselves.

export async function getSettings(): Promise<Settings> {
  if (backend) return invoke("get_settings");
  return (await fallback()).getSettings();
}

/// `patch` carries only the fields the user changed. The backend refuses an
/// unknown key or an invalid value with its own message, and a journal key
/// while the vault is locked with `"vault is locked"`.
export async function updateSettings(patch: SettingsPatch): Promise<Settings> {
  if (backend) return invoke("update_settings", { patch });
  return (await fallback()).updateSettings(patch);
}
