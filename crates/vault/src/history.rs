//! Naming and retention policy for a second kind of blob: an immutable
//! snapshot of an entry as it looked right before some write overwrote it.
//!
//! Kept in its own module because the naming has an invariant worth proving
//! in isolation (round-tripping through [`snapshot_id_and_time`]) separately
//! from `Vault`'s policy of *when* to take one.

use chrono::{DateTime, Duration, Utc};
use journal_entry::Id;
use journal_store::BlobName;

use crate::ENTRY_EXT;

/// How long an outgoing version must have sat unchanged — or how much
/// continuous editing must have passed since this vault's last snapshot of an
/// entry — before a save takes one. See `Vault::snapshot_outgoing`.
pub(crate) const SNAPSHOT_MIN_AGE: Duration = Duration::minutes(30);

/// How many past versions of one entry are kept; the oldest beyond this are
/// pruned after every snapshot.
pub(crate) const MAX_SNAPSHOTS_PER_ENTRY: usize = 20;

const SNAPSHOT_INFIX: &str = ".snap-";

/// `<id>.snap-<updated_at as epoch millis, zero-padded to 16 digits>.json`.
///
/// Zero-padding makes the lexicographic order of `BlobName`s (`Ord` is
/// derived from the wrapped `String`) agree with chronological order, the
/// same property `blob_name`'s `<uuid>.json` naming gets for free from a store
/// that only ever holds one name per id.
pub(crate) fn snapshot_blob_name(id: &Id, at: DateTime<Utc>) -> BlobName {
    BlobName::try_from(format!(
        "{id}{SNAPSHOT_INFIX}{:016}{ENTRY_EXT}",
        at.timestamp_millis()
    ))
    .expect("an id, the snapshot infix and a zero-padded millisecond count is a valid blob name")
}

/// The inverse of [`snapshot_blob_name`], strict the same way `entry_id` is
/// strict about `blob_name`: only a name this function would itself produce
/// comes back `Some`.
pub(crate) fn snapshot_id_and_time(name: &BlobName) -> Option<(Id, DateTime<Utc>)> {
    let rest = name.strip_suffix(ENTRY_EXT)?;
    let (id_part, millis_part) = rest.split_once(SNAPSHOT_INFIX)?;
    let id: Id = id_part.parse().ok()?;
    if millis_part.len() != 16 {
        return None;
    }
    let millis: i64 = millis_part.parse().ok()?;
    let at = DateTime::from_timestamp_millis(millis)?;
    (snapshot_blob_name(&id, at) == *name).then_some((id, at))
}

/// Whether `name` is one of an entry's snapshot blobs.
///
/// Public because the sync resolver in `src-tauri` must recognise snapshot
/// blobs, to resolve a same-name conflict between two of them without a key
/// (two devices' independent encryptions of the same snapshot are always
/// resolved by keeping either one).
pub fn is_snapshot(name: &BlobName) -> bool {
    snapshot_id_and_time(name).is_some()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn snapshot_id_and_time_inverts_the_naming_and_refuses_other_names() {
        let id = Id::new();
        let at = Utc::now();
        // Round-trips, but only to millisecond precision (`timestamp_millis`
        // truncates whatever sub-millisecond part `Utc::now()` produced).
        let at = DateTime::from_timestamp_millis(at.timestamp_millis()).unwrap();
        let name = snapshot_blob_name(&id, at);
        assert_eq!(snapshot_id_and_time(&name), Some((id, at)));
        assert!(is_snapshot(&name));

        let other = |s: String| BlobName::try_from(s).unwrap();
        // An entry blob, the key blob, and names from somewhere else.
        assert_eq!(
            snapshot_id_and_time(&other(format!("{id}{ENTRY_EXT}"))),
            None
        );
        assert_eq!(snapshot_id_and_time(&other(".vault-key".to_owned())), None);
        assert_eq!(snapshot_id_and_time(&other("notes.json".to_owned())), None);
        // The right shape, but garbage where the id or the millis should be.
        assert_eq!(
            snapshot_id_and_time(&other(format!(
                "not-a-uuid{SNAPSHOT_INFIX}0000000000000000{ENTRY_EXT}"
            ))),
            None
        );
        assert_eq!(
            snapshot_id_and_time(&other(format!("{id}{SNAPSHOT_INFIX}notmillis{ENTRY_EXT}"))),
            None
        );
        // Right length, but not zero-padded the way this module writes it —
        // still 16 digits and still parses, so it must still round-trip.
        let short_millis = format!("{id}{SNAPSHOT_INFIX}0000000000000001{ENTRY_EXT}");
        assert_eq!(
            snapshot_id_and_time(&other(short_millis)),
            Some((id, DateTime::from_timestamp_millis(1).unwrap()))
        );
    }

    #[test]
    fn snapshot_names_sort_lexicographically_in_chronological_order() {
        let id = Id::new();
        let earlier = snapshot_blob_name(&id, DateTime::from_timestamp_millis(1_000).unwrap());
        let later = snapshot_blob_name(&id, DateTime::from_timestamp_millis(2_000).unwrap());
        assert!(earlier < later);
    }
}
