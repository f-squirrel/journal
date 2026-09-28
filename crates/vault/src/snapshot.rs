//! Naming and retention policy for a second kind of blob: an immutable
//! snapshot of an entry as it looked right before some write overwrote it.
//!
//! Kept in its own module because the naming has an invariant worth proving
//! in isolation (round-tripping through [`owned::parse`]) separately
//! from `Vault`'s policy of *when* to take one.

use chrono::{DateTime, Duration, Utc};
use journal_store::BlobName;

use crate::owned::{self, OwnedBlob};
use crate::ENTRY_EXT;

/// How long an outgoing version must have sat unchanged — or how much
/// continuous editing must have passed since this vault's last snapshot of an
/// entry — before a save takes one. See `Vault::snapshot_outgoing`.
pub(crate) const SNAPSHOT_MIN_AGE: Duration = Duration::minutes(30);

/// How many past versions of one entry are kept; the oldest beyond this are
/// pruned after every snapshot.
pub(crate) const MAX_SNAPSHOTS_PER_ENTRY: usize = 20;

/// `<id>.snap-<updated_at as epoch millis, zero-padded to 16 digits>.json`.
///
/// Zero-padding makes the lexicographic order of `BlobName`s (`Ord` is
/// derived from the wrapped `String`) agree with chronological order, the
/// same property `blob_name`'s `<uuid>.json` naming gets for free from a store
/// that only ever holds one name per id.
pub(crate) struct Snapshot;

impl OwnedBlob for Snapshot {
    const INFIX: &'static str = ".snap-";
    type Key = DateTime<Utc>;

    fn tail(at: &DateTime<Utc>) -> String {
        format!("{:016}{ENTRY_EXT}", at.timestamp_millis())
    }

    fn parse_tail(tail: &str) -> Option<DateTime<Utc>> {
        let millis = tail.strip_suffix(ENTRY_EXT)?;
        if millis.len() != 16 {
            return None;
        }
        DateTime::from_timestamp_millis(millis.parse().ok()?)
    }
}

/// Whether `name` is one of an entry's snapshot blobs.
///
/// Public because the sync resolver in `src-tauri` must recognise snapshot
/// blobs, to resolve a same-name conflict between two of them without a key
/// (two devices' independent encryptions of the same snapshot are always
/// resolved by keeping either one).
pub fn is_snapshot(name: &BlobName) -> bool {
    owned::parse::<Snapshot>(name).is_some()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use journal_entry::Id;

    const INFIX: &str = Snapshot::INFIX;

    #[test]
    fn parse_inverts_the_naming_and_refuses_other_names() {
        let id = Id::new();
        let at = Utc::now();
        // Round-trips, but only to millisecond precision (`timestamp_millis`
        // truncates whatever sub-millisecond part `Utc::now()` produced).
        let at = DateTime::from_timestamp_millis(at.timestamp_millis()).unwrap();
        let name = owned::name::<Snapshot>(&id, &at);
        assert_eq!(owned::parse::<Snapshot>(&name), Some((id, at)));
        assert!(is_snapshot(&name));

        let other = |s: String| BlobName::try_from(s).unwrap();
        // An entry blob, the key blob, and names from somewhere else.
        assert_eq!(
            owned::parse::<Snapshot>(&other(format!("{id}{ENTRY_EXT}"))),
            None
        );
        assert_eq!(
            owned::parse::<Snapshot>(&other(".vault-key".to_owned())),
            None
        );
        assert_eq!(
            owned::parse::<Snapshot>(&other("notes.json".to_owned())),
            None
        );
        // The right shape, but garbage where the id or the millis should be.
        assert_eq!(
            owned::parse::<Snapshot>(&other(format!(
                "not-a-uuid{INFIX}0000000000000000{ENTRY_EXT}"
            ))),
            None
        );
        assert_eq!(
            owned::parse::<Snapshot>(&other(format!("{id}{INFIX}notmillis{ENTRY_EXT}"))),
            None
        );
        // Right length, but not zero-padded the way this module writes it —
        // still 16 digits and still parses, so it must still round-trip.
        let short_millis = format!("{id}{INFIX}0000000000000001{ENTRY_EXT}");
        assert_eq!(
            owned::parse::<Snapshot>(&other(short_millis)),
            Some((id, DateTime::from_timestamp_millis(1).unwrap()))
        );
    }

    #[test]
    fn snapshot_names_sort_lexicographically_in_chronological_order() {
        let id = Id::new();
        let earlier =
            owned::name::<Snapshot>(&id, &DateTime::from_timestamp_millis(1_000).unwrap());
        let later = owned::name::<Snapshot>(&id, &DateTime::from_timestamp_millis(2_000).unwrap());
        assert!(earlier < later);
    }
}
