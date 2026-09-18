use std::collections::BTreeSet;
use std::time::SystemTime;

use journal_store::BlobName;

use crate::SyncErrorKind;

/// What the app can honestly say about how far the journal has got.
///
/// Tracks the **local → remote** direction only. A remote change that has not
/// been pulled cannot be known without asking the provider, so the strongest
/// claim available is "synced as of `last_success`".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncStatus {
    /// Local changes not yet on the remote: a blob that is new, edited since
    /// the last sync, or deleted here but not yet deleted there.
    pub pending: BTreeSet<BlobName>,
    /// Parked conflicts waiting for the resolver (e.g. the vault is locked).
    /// Each is also in `pending` until the resolver's result has been pushed.
    pub conflicted: BTreeSet<BlobName>,
    /// A pass is running right now.
    pub running: bool,
    /// When the last pass finished without error.
    pub last_success: Option<SystemTime>,
    /// Why the last pass failed, if it did. Cleared by the next success.
    pub last_error: Option<SyncErrorKind>,
}
