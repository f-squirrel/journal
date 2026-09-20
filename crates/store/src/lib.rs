//! Where the bytes live — the abstraction only.
//!
//! A [`BlobStore`] is a flat namespace of opaque byte blobs addressed by name.
//! It is the bottom layer of the persistence stack (see ADR 0009): it knows
//! nothing about journal entries, keys, JSON, or encryption, and everything
//! about which directory — or, later, which cloud folder — the bytes are in.
//!
//! This crate holds no implementation on purpose. `journal-fs-store` is the
//! local one; a Dropbox or Drive backend would be another crate beside it, and
//! every layer above depends on this trait rather than on any of them.
//!
//! Because a store only ever handles ciphertext, a consumer that merely moves
//! blobs around needs no key at all. That is what a future sync engine is meant
//! to be.

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

mod error;
mod name;

pub use error::StoreError;
pub use name::BlobName;

/// A flat, named collection of byte blobs.
///
/// `Send + Sync` because the app holds one for the whole session behind Tauri's
/// managed state, shared across command invocations on different threads.
///
/// Names arrive as [`BlobName`], so an implementation does not have to check
/// them: a name that would address something outside the store cannot be
/// constructed in the first place.
pub trait BlobStore: Send + Sync {
    /// The blob's bytes, or `None` if there is no such blob.
    fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError>;

    /// Creates or replaces the named blob.
    fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError>;

    /// Removes the named blob. Succeeds if it was not there.
    fn delete(&self, name: &BlobName) -> Result<(), StoreError>;

    /// The names of every blob in the store, in no particular order.
    ///
    /// Empty rather than an error when the store has never been written to:
    /// "nothing here yet" is a normal first run, not a failure.
    fn list(&self) -> Result<Vec<BlobName>, StoreError>;

    /// Whether the named blob exists, without reading it.
    fn contains(&self, name: &BlobName) -> Result<bool, StoreError>;
}
