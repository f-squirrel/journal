import type { SyncStatusPayload } from "../types";
import { formatAgo } from "./date";

/// Changes that have not reached the cloud: entries, plus the blobs that are
/// not entries (the sealed key), which the user has no row for.
export function pendingCount(sync: SyncStatusPayload): number {
  return sync.pending_ids.length + sync.pending_other;
}

function changes(count: number): string {
  return count === 1 ? "1 change" : `${count} changes`;
}

/// One line saying what sync is doing, for the rail's tooltip and its accessible
/// name. It states only what is true: "Synced" means everything reached the
/// provider, so it is never said while something is waiting.
export function syncSummary(sync: SyncStatusPayload, now: number): string {
  const count = pendingCount(sync);
  const provider = sync.provider ?? "the cloud";
  switch (sync.state) {
    case "off":
      return "Sync is off";
    case "synced":
      return sync.last_synced_at
        ? `Synced · ${formatAgo(sync.last_synced_at, now)}`
        : "Synced";
    case "pending":
      return `${changes(count)} not uploaded yet`;
    case "syncing":
      return "Syncing…";
    case "offline":
      return count > 0 ? `Offline · ${changes(count)} not uploaded` : "Offline";
    case "conflicts":
      return "Conflicting versions to resolve";
    case "error":
      return "Sync failed";
    case "reauth":
      return `Sign in to ${provider} again`;
    case "key_mismatch":
      return "This device and the cloud have different journal keys";
  }
}
