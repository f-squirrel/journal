use journal_store::BlobName;
use serde::{Deserialize, Serialize};

/// An opaque per-blob version: Dropbox `rev`, Drive `version` /
/// `headRevisionId`, CloudKit `recordChangeTag`.
///
/// Compared for equality only. Providers disagree on what a revision looks
/// like and none promises they order, so nothing above an adapter may parse
/// one or ask which is newer. Serializes as the bare string, because the sync
/// engine persists it as part of its per-device base state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(String);

impl Revision {
    pub fn new(rev: impl Into<String>) -> Self {
        Self(rev.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An opaque position in a remote's change feed.
///
/// Hand it back to [`RemoteStore::changes`](crate::RemoteStore::changes) to
/// continue from where a previous call stopped. Serializes as the bare string,
/// because the engine persists it between runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    pub fn new(cursor: impl Into<String>) -> Self {
        Self(cursor.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a write requires of the remote's current state.
///
/// There is deliberately no "unconditional" option: a write that cannot lose a
/// race is a write that can silently overwrite another device's edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Precondition {
    /// The blob must not exist. For `put` this is create-only. For `delete` it
    /// means the delete only succeeds if there is nothing to delete, so it is
    /// rarely what a caller wants.
    Absent,
    /// The blob must still be at this revision.
    Matches(Revision),
}

/// One entry in the change feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The blob was created or replaced.
    Upserted {
        name: BlobName,
        rev: Revision,
        /// The backend's content hash of these bytes, comparable with
        /// [`RemoteStore::content_hash`](crate::RemoteStore::content_hash), so
        /// the engine can tell whether a blob it has never synced already
        /// matches the local copy without downloading it. `None` if the
        /// backend exposes none.
        content_hash: Option<String>,
    },
    /// The blob was deleted. There is no tombstone blob (D3); the feed itself
    /// carries the deletion.
    Deleted { name: BlobName },
}

impl Change {
    pub fn name(&self) -> &BlobName {
        match self {
            Change::Upserted { name, .. } | Change::Deleted { name } => name,
        }
    }
}

/// One page of the change feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePage {
    pub changes: Vec<Change>,
    /// Where to continue from. When `has_more` is false this is the position to
    /// persist and pass as `since` on the next pass.
    pub cursor: Cursor,
    /// More changes are waiting: call `changes(Some(&cursor))` again.
    pub has_more: bool,
}
