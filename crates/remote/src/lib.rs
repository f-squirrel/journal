//! A cloud provider's copy of the journal — the abstraction only.
//!
//! [`RemoteStore`] is what the sync engine (`journal-sync`) needs from a
//! provider, and it is *not* a [`BlobStore`](journal_store::BlobStore). A local
//! store answers "what is here?" and overwrites on request. A remote is shared
//! with other devices, so it also has to answer "what changed since I last
//! looked?" (including deletions) and refuse a write that would clobber someone
//! else's. Those two contracts are the point of this crate; see ADR 0009 for
//! where it sits in the stack.
//!
//! Like `journal-store`, it holds no real implementation: `journal-dropbox` is
//! the first, and Drive and CloudKit are meant to fit the same shape without
//! changing it. Only bytes ever cross this trait, and they are ciphertext, so
//! an implementation needs no key. The trait is synchronous on purpose (D8): the
//! engine runs on its own thread, and each adapter blocks inside its own client.
//!
//! Behind the `testing` feature, [`MemoryRemote`] is an in-memory implementation
//! that enforces the contracts and can inject failures, for the engine's tests.

mod error;
#[cfg(any(test, feature = "testing"))]
mod memory;
mod types;

pub use error::RemoteError;
#[cfg(any(test, feature = "testing"))]
pub use memory::{ConflictMode, MemoryRemote};
pub use types::{Change, ChangePage, Cursor, Precondition, Revision};

use journal_store::BlobName;

/// A remote, shared, flat collection of encrypted blobs.
///
/// `Send + Sync`: the engine holds one for the whole session, and status calls
/// arrive from other threads while a pass is running.
///
/// Every method blocks until the provider has answered. None retries: a
/// failure comes back as a typed [`RemoteError`] and the caller decides what to
/// do about it.
pub trait RemoteStore: Send + Sync {
    /// What changed on the remote.
    ///
    /// - `None`: the initial listing. Every blob that exists, each as
    ///   [`Change::Upserted`].
    /// - `Some(cursor)`: everything since that cursor, deletions included.
    ///
    /// Page until `has_more` is false by passing each page's `cursor` back in,
    /// then persist the last cursor. This works the same way for the initial
    /// listing.
    ///
    /// Changes come in feed order, and one name may appear more than once (an
    /// edit followed by a delete): a later change supersedes an earlier one.
    /// A change may also describe a revision that a later change, or a later
    /// `get`, has already replaced, so treat the feed as "these names moved" and
    /// let `get` say what they moved to.
    fn changes(&self, since: Option<&Cursor>) -> Result<ChangePage, RemoteError>;

    /// The blob's bytes and current revision, or `None` if there is no such
    /// blob.
    fn get(&self, name: &BlobName) -> Result<Option<(Vec<u8>, Revision)>, RemoteError>;

    /// Creates or replaces the named blob if `pre` holds, and returns the
    /// revision it now has.
    ///
    /// **Write-race contract (D2).** If `pre` does not hold — someone else got
    /// there first — this returns [`RemoteError::Conflict`]. It never
    /// overwrites silently:
    ///
    /// - A backend with an atomic conditional write (Dropbox, CloudKit) refuses
    ///   the write and returns `Conflict { theirs: None }`; the other side is
    ///   still there to `get`.
    /// - A backend without one (Drive) may have to write first and check after.
    ///   It then returns `Conflict { theirs: Some(..) }` carrying the version
    ///   that was overwritten, because a `get` would only return our own bytes.
    ///
    /// `Matches` on a blob that no longer exists is also a conflict: the state
    /// the caller based its write on is gone.
    fn put(
        &self,
        name: &BlobName,
        bytes: &[u8],
        pre: Precondition,
    ) -> Result<Revision, RemoteError>;

    /// Deletes the named blob if `pre` holds. Same race contract as
    /// [`put`](RemoteStore::put), except that deleting a blob that is already
    /// gone succeeds: the caller wanted it gone.
    fn delete(&self, name: &BlobName, pre: Precondition) -> Result<(), RemoteError>;

    /// This backend's content hash of `bytes`, computed locally and comparable
    /// with [`Change::Upserted::content_hash`]. `None` if the backend exposes
    /// no hash, in which case the caller has to download to compare.
    ///
    /// Pure and offline, so it cannot fail.
    fn content_hash(&self, bytes: &[u8]) -> Option<String>;
}
