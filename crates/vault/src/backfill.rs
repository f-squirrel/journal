//! A one-shot repair for entries written before `Header::entry_date` existed.
//!
//! **This is the one exception to the no-migration rule** in `CLAUDE.md`, and
//! it is deliberately shaped so that it stays one: nothing in the app calls
//! it, no command exposes it, and the code that reads entries has no idea it
//! exists. It is run by hand, once, from `examples/backfill.rs`, and this
//! whole module can be deleted afterwards without touching anything else.
//!
//! The rule it bends is about *format detection in the read path* — a second
//! shape alive at once, and every reader having to branch on which one it
//! got. That is still forbidden. A separate tool that rewrites blobs into the
//! one current shape and then goes away adds no branch to any reader.
//!
//! ## Why it cannot work on `Entry`
//!
//! `entry_date` has a `#[serde(default)]` of "now", so by the time a blob has
//! been deserialized into an [`Entry`] the field is populated either way and
//! the two cases are indistinguishable. Whether the field was *on disk* is
//! therefore a question about the raw JSON, and has to be asked before serde
//! answers it. That is the only reason this module reaches for
//! [`serde_json::Value`]; the rewrite itself goes back through
//! [`Vault::save_entry`], so there is exactly one encrypt-and-store path.

use chrono::{DateTime, Utc};
use journal_entry::Entry;
use serde_json::Value;

use crate::{is_entry, Vault, VaultError};

/// Whether a run writes anything.
///
/// The tool defaults to [`BackfillMode::DryRun`] because the blobs it rewrites
/// are the user's only copy of their journal, and a dry run answers the only
/// question worth asking first: how many entries would this touch?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillMode {
    DryRun,
    Apply,
}

/// What one run found. Counts only — a title or an id printed next to "this
/// entry was repaired" is journal content, and this tool has no business
/// putting any of it on a terminal or in a log.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BackfillReport {
    /// Entry blobs looked at.
    pub scanned: usize,
    /// Entries that had no `entry_date` and were given their `updated_at`.
    /// In [`BackfillMode::DryRun`] this is what *would* have been written.
    pub backfilled: usize,
    /// Entries that already had an `entry_date`, left untouched.
    pub already_dated: usize,
    /// Entries that could not be decrypted or parsed. Skipped, never
    /// rewritten — the same thing `list_entries` does with them.
    pub unreadable: usize,
}

impl Vault {
    /// Gives every entry that predates `entry_date` its `updated_at` as one.
    ///
    /// `updated_at` itself is never touched: the whole point is that these two
    /// timestamps are different facts, and an entry's last-written time is not
    /// this tool's to invent. Entries that already carry an `entry_date` are
    /// left alone, which makes the run idempotent — a second one repairs
    /// nothing.
    ///
    /// Rewriting a blob re-encrypts it, and `age` output is not
    /// deterministic, so every repaired entry looks changed to the sync
    /// engine and will upload. That is correct — it *did* change — but it is
    /// why the tool that calls this says to sync afterwards.
    pub fn backfill_entry_dates(&self, mode: BackfillMode) -> Result<BackfillReport, VaultError> {
        let mut report = BackfillReport::default();

        for name in self.store.list()?.into_iter().filter(is_entry) {
            report.scanned += 1;

            let Some(ciphertext) = self.store.get(&name)? else {
                // Listed a moment ago and gone now: a delete racing this run.
                // Not an error, and not something to repair.
                report.scanned -= 1;
                continue;
            };
            let Ok(plaintext) = self.cipher.decrypt(&ciphertext) else {
                // Rendered nowhere: unlike `list_entries` this runs in a
                // terminal the user is watching, and `age`'s error text says
                // nothing actionable about a blob we are not going to touch.
                report.unreadable += 1;
                continue;
            };

            match entry_date_missing_from(&plaintext) {
                Ok(false) => {
                    report.already_dated += 1;
                    continue;
                }
                Ok(true) => {}
                Err(()) => {
                    report.unreadable += 1;
                    continue;
                }
            }

            // Parsed from the same bytes, so `entry_date` takes its serde
            // default here — which is exactly the value being replaced.
            let Ok(mut entry) = serde_json::from_slice::<Entry>(&plaintext) else {
                report.unreadable += 1;
                continue;
            };
            entry.header.entry_date = entry.header.updated_at;

            if mode == BackfillMode::Apply {
                self.save_entry(&entry)?;
            }
            report.backfilled += 1;
        }

        Ok(report)
    }
}

/// Whether this blob's JSON has no `entry_date` of its own.
///
/// `Err(())` for bytes that are not an entry object at all — not something to
/// repair, and not something to guess at.
fn entry_date_missing_from(plaintext: &[u8]) -> Result<bool, ()> {
    let value: Value = serde_json::from_slice(plaintext).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    // An `updated_at` to copy is what makes an entry repairable, so a blob
    // without a usable one is unreadable for this purpose even if it parses.
    object
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .ok_or(())?;
    // `null` counts as missing: it is what a hand-edited blob would hold, and
    // serde would reject it rather than take the default.
    Ok(match object.get("entry_date") {
        None | Some(Value::Null) => true,
        Some(_) => false,
    })
}

/// The id a repaired blob belongs to, for tests that need to read one back.
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn parse_id(plaintext: &[u8]) -> Option<journal_entry::Id> {
    let value: Value = serde_json::from_slice(plaintext).ok()?;
    value.get("id")?.as_str()?.parse().ok()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::tests::{discard, fake_vault, temp_store, FakeCipher};
    use journal_entry::Id;
    use journal_key::Cipher;
    use journal_store::{BlobName, BlobStore};

    /// An entry blob as it looked before `entry_date` existed: no such key.
    fn legacy_blob(title: &str, updated_at: &str) -> (Id, Vec<u8>) {
        let id = Id::new();
        let json = format!(
            r##"{{"id":"{id}","title":"{title}","updated_at":"{updated_at}","markdown":"# {title}"}}"##
        );
        (id, FakeCipher.encrypt(json.as_bytes()).unwrap())
    }

    fn put(store: &journal_fs_store::FsStore, id: &Id, bytes: &[u8]) {
        let name = BlobName::try_from(format!("{id}.json")).unwrap();
        store.put(&name, bytes).unwrap();
    }

    #[test]
    fn an_entry_without_a_date_gets_its_updated_at() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (id, blob) = legacy_blob("older", "2026-06-12T07:30:00Z");
        put(&store, &id, &blob);

        let report = vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        assert_eq!(report.backfilled, 1);
        assert_eq!(report.already_dated, 0);
        assert_eq!(report.unreadable, 0);
        let repaired = vault.get_entry(&id).unwrap().unwrap();
        let expected: DateTime<Utc> = "2026-06-12T07:30:00Z".parse().unwrap();
        assert_eq!(repaired.header.entry_date, expected);
        // The fact the tool must not invent.
        assert_eq!(repaired.header.updated_at, expected);
        discard(&store);
    }

    #[test]
    fn a_dry_run_writes_nothing() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (id, blob) = legacy_blob("untouched", "2026-06-12T07:30:00Z");
        put(&store, &id, &blob);

        let report = vault.backfill_entry_dates(BackfillMode::DryRun).unwrap();

        assert_eq!(report.backfilled, 1);
        // Still the bytes we wrote, with no `entry_date` key in them.
        let name = BlobName::try_from(format!("{id}.json")).unwrap();
        let on_disk = store.get(&name).unwrap().unwrap();
        assert_eq!(on_disk, blob);
        discard(&store);
    }

    #[test]
    fn an_entry_that_already_has_a_date_is_left_alone() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let mut entry = crate::tests::sample(id);
        let entry_date: DateTime<Utc> = "2026-06-12T07:30:00Z".parse().unwrap();
        entry.header.entry_date = entry_date;
        vault.save_entry(&entry).unwrap();

        let report = vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        assert_eq!(report.already_dated, 1);
        assert_eq!(report.backfilled, 0);
        assert_eq!(
            vault.get_entry(&id).unwrap().unwrap().header.entry_date,
            entry_date
        );
        discard(&store);
    }

    #[test]
    fn running_it_twice_repairs_nothing_the_second_time() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (id, blob) = legacy_blob("once", "2026-06-12T07:30:00Z");
        put(&store, &id, &blob);

        assert_eq!(
            vault
                .backfill_entry_dates(BackfillMode::Apply)
                .unwrap()
                .backfilled,
            1
        );
        let second = vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        assert_eq!(second.backfilled, 0);
        assert_eq!(second.already_dated, 1);
        discard(&store);
    }

    #[test]
    fn a_blob_that_will_not_decrypt_is_skipped_not_fatal() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (good, blob) = legacy_blob("good", "2026-06-12T07:30:00Z");
        put(&store, &good, &blob);
        put(&store, &Id::new(), b"not ciphertext at all");

        let report = vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        assert_eq!(report.backfilled, 1);
        assert_eq!(report.unreadable, 1);
        assert!(vault.get_entry(&good).unwrap().is_some());
        discard(&store);
    }

    #[test]
    fn a_blob_with_no_usable_updated_at_is_not_guessed_at() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        let json = format!(r##"{{"id":"{id}","title":"t","markdown":"# t"}}"##);
        put(&store, &id, &FakeCipher.encrypt(json.as_bytes()).unwrap());

        let report = vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        assert_eq!(report.backfilled, 0);
        assert_eq!(report.unreadable, 1);
        discard(&store);
    }

    #[test]
    fn the_key_blob_is_not_an_entry_and_is_not_scanned() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (id, blob) = legacy_blob("only entry", "2026-06-12T07:30:00Z");
        put(&store, &id, &blob);
        store
            .put(
                &BlobName::try_from(".vault-key".to_owned()).unwrap(),
                b"key",
            )
            .unwrap();

        let report = vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        assert_eq!(report.scanned, 1);
        assert_eq!(report.unreadable, 0);
        discard(&store);
    }

    #[test]
    fn a_repaired_blob_really_has_the_key_on_disk() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let (id, blob) = legacy_blob("check", "2026-06-12T07:30:00Z");
        put(&store, &id, &blob);

        vault.backfill_entry_dates(BackfillMode::Apply).unwrap();

        let name = BlobName::try_from(format!("{id}.json")).unwrap();
        let plaintext = FakeCipher
            .decrypt(&store.get(&name).unwrap().unwrap())
            .unwrap();
        assert!(!entry_date_missing_from(&plaintext).unwrap());
        assert_eq!(parse_id(&plaintext), Some(id));
        discard(&store);
    }
}
