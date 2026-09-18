import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ProviderInfo, SyncStatusPayload } from "../types";
import { backend, fallback } from "./backend";

// Sync's half of the API, wrapped like `./entries`: over `invoke` and Tauri
// events in the app, over the fallback's simulation outside it. Sync does not
// need the vault unlocked, so none of these are refused while it is locked.

export async function syncStatus(): Promise<SyncStatusPayload> {
  if (backend) return invoke("sync_status");
  return (await fallback()).syncStatus();
}

export async function listProviders(): Promise<ProviderInfo[]> {
  if (backend) return invoke("list_providers");
  return (await fallback()).listProviders();
}

/// Sends the user to the provider in their browser and waits for them to come
/// back, which can take minutes. Resolves once connected and the first pass has
/// run; rejects with the reason it could not.
export async function connectRemote(provider: string): Promise<void> {
  if (backend) return invoke("connect_remote", { provider });
  return (await fallback()).connectRemote(provider);
}

/// Forgets the account. The journal on this device is not touched.
export async function disconnectRemote(): Promise<void> {
  if (backend) return invoke("disconnect_remote");
  return (await fallback()).disconnectRemote();
}

/// Runs a pass now. Rejects with why it failed; the status says the same.
export async function syncNow(): Promise<void> {
  if (backend) return invoke("sync_now");
  return (await fallback()).syncNow();
}

/// `sync://status`: the latest status, at most a few times a second. Resolves
/// to the function that stops listening.
export async function onSyncStatus(
  handler: (status: SyncStatusPayload) => void,
): Promise<() => void> {
  if (backend) {
    return listen<SyncStatusPayload>("sync://status", (event) =>
      handler(event.payload),
    );
  }
  return (await fallback()).onSyncStatus(handler);
}

/// `sync://changed`: a pass changed entries on disk, so the list is stale.
export async function onSyncChanged(handler: () => void): Promise<() => void> {
  if (backend) return listen("sync://changed", () => handler());
  return (await fallback()).onSyncChanged(handler);
}
