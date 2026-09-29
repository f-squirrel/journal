//! What the app tells the frontend about sync: one payload, sent over both the
//! `sync_status` command and the `sync://status` event, and the list of
//! providers the connect dialog offers.
//!
//! Only the shape lives here; deriving the state from the sync engine's own
//! `SyncStatus` is the backend's job (`src-tauri/src/sync/status.rs`).

use chrono::{DateTime, Utc};
use serde::Serialize;

use journal_entry::Id;

/// The one word the rail's sync button is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// Not connected to a cloud provider.
    Off,
    /// Everything local has reached the provider, as of `last_synced_at`.
    Synced,
    /// Local changes are waiting to be uploaded.
    Pending,
    /// A pass is running right now.
    Syncing,
    /// The last pass could not reach the provider, or was told to slow down.
    Offline,
    /// Two devices edited the same entry, and the resolver has not settled it
    /// yet (the vault is locked).
    Conflicts,
    /// The last pass failed for a reason retrying will not obviously fix.
    Error,
    /// The provider no longer accepts our credentials: connect again.
    Reauth,
    /// This device and the cloud hold different journal keys, so sync is
    /// stopped until the user chooses which journal to keep.
    KeyMismatch,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
pub struct SyncStatusPayload {
    pub state: SyncState,
    /// The connected provider's name as the user knows it ("Dropbox"), for
    /// words like "Synced to Dropbox". `None` when nothing is connected.
    pub provider: Option<String>,
    /// The connected provider's id (`ProviderInfo::id`), so a list of
    /// providers can tell which one is connected. `None` when nothing is.
    pub provider_id: Option<String>,
    /// Entries with local changes not yet uploaded. Includes an entry deleted
    /// here whose deletion has not been sent, so the frontend ignores ids with
    /// no row. Sent in every state, so an `offline` status still marks what
    /// did not upload.
    pub pending_ids: Vec<Id>,
    /// Entries with a parked conflict.
    pub conflicted_ids: Vec<Id>,
    /// Pending blobs that are not entries: the sealed key.
    pub pending_other: u32,
    /// When the last pass finished without error.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Why the last pass failed, in words for the user.
    pub message: Option<String>,
}

/// A cloud provider the build can connect to, for the connect dialog.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../src/bindings/")
)]
pub struct ProviderInfo {
    /// What `connect_remote` takes.
    pub id: String,
    /// What the user calls it.
    pub name: String,
    /// `false` = listed as coming later; it cannot be connected.
    pub available: bool,
    /// `true` = connecting can start now (Dropbox: an app key resolves).
    pub configured: bool,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    /// The frontend reads these keys and strings by name, and the wire format
    /// is `snake_case` on both sides (see the crate's other types).
    #[test]
    fn the_payload_serializes_snake_case() {
        let payload = SyncStatusPayload {
            state: SyncState::KeyMismatch,
            provider: Some("Dropbox".to_owned()),
            provider_id: Some("dropbox".to_owned()),
            pending_ids: vec!["5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01".parse().unwrap()],
            conflicted_ids: vec![],
            pending_other: 1,
            last_synced_at: Some("2026-09-07T20:17:50Z".parse().unwrap()),
            message: None,
        };

        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            serde_json::json!({
                "state": "key_mismatch",
                "provider": "Dropbox",
                "provider_id": "dropbox",
                "pending_ids": ["5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01"],
                "conflicted_ids": [],
                "pending_other": 1,
                "last_synced_at": "2026-09-07T20:17:50Z",
                "message": null,
            })
        );
    }
}
