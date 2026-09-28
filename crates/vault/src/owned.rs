//! Blobs that belong to one entry and are named `<entry-id><infix><tail>`:
//! snapshots (`snapshot.rs`) and media (`media.rs`).
//!
//! The naming, its strict inverse and the per-entry enumeration exist once
//! here. Each kind supplies only its infix and how its key is written as, and
//! read back from, the tail.

use journal_entry::Id;
use journal_store::{BlobName, BlobStore, StoreError};

pub(crate) trait OwnedBlob {
    const INFIX: &'static str;
    /// What tells one of an entry's blobs of this kind from another.
    type Key;

    /// Everything after the infix.
    fn tail(key: &Self::Key) -> String;
    /// The inverse of [`tail`](Self::tail). May be lenient: [`parse`] checks
    /// the round trip.
    fn parse_tail(tail: &str) -> Option<Self::Key>;
}

pub(crate) fn name<K: OwnedBlob>(id: &Id, key: &K::Key) -> BlobName {
    BlobName::try_from(format!("{id}{}{}", K::INFIX, K::tail(key)))
        .expect("an id, an infix and a key tail make a valid blob name")
}

/// The inverse of [`name`], and strict about it: only a name `name` would
/// itself produce comes back `Some` (no upper-case uuids, no odd padding).
pub(crate) fn parse<K: OwnedBlob>(blob: &BlobName) -> Option<(Id, K::Key)> {
    let (id_part, tail) = blob.split_once(K::INFIX)?;
    let id: Id = id_part.parse().ok()?;
    let key = K::parse_tail(tail)?;
    (name::<K>(&id, &key) == *blob).then_some((id, key))
}

/// Every blob of kind `K` that `id` owns, with its key. The one enumeration
/// of "this entry's snapshots" / "this entry's media".
pub(crate) fn owned_by<K: OwnedBlob>(
    store: &dyn BlobStore,
    id: &Id,
) -> Result<Vec<(K::Key, BlobName)>, StoreError> {
    Ok(store
        .list()?
        .into_iter()
        .filter_map(|blob| {
            let (owner, key) = parse::<K>(&blob)?;
            (owner == *id).then_some((key, blob))
        })
        .collect())
}
