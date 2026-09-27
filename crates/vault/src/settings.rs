//! The journal half of settings: one encrypted blob in the vault's store,
//! stored as [`StampedFields`] and merged per field on a sync conflict.
//!
//! Kept out of `lib.rs` for the reason `search.rs` is: none of this needs the
//! entry naming or listing above it, only the store and the cipher `Vault`
//! already holds. Being a submodule of the crate root, it can still reach
//! `Vault`'s private fields and its private [`Vault::read`] — the same way
//! every method in `lib.rs` can.

use chrono::Utc;
use journal_settings::{JournalSettings, SettingsError, StampedFields};
use journal_store::BlobName;

use crate::{Vault, VaultError};

/// The blob the journal half of settings lives in.
pub const SETTINGS_BLOB: &str = ".journal-settings";

fn settings_blob_name() -> BlobName {
    BlobName::literal(SETTINGS_BLOB)
}

/// Whether `name` is the settings blob — the other case, besides an entry,
/// that [`super::holds_content`] must not treat as "nothing here yet".
pub(crate) fn is_settings_blob(name: &BlobName) -> bool {
    *name == settings_blob_name()
}

impl Vault {
    /// The journal's settings: defaults if the blob is missing, or if it
    /// fails to decrypt or parse (logged, not an error — the same treatment a
    /// corrupt entry gets from [`Vault::list_entries`](crate::Vault::list_entries)).
    pub fn journal_settings(&self) -> Result<JournalSettings, VaultError> {
        Ok(self.read_stamped_fields()?.view())
    }

    /// Validates `settings`, stamps whatever changed with `Utc::now()`, and
    /// writes the blob only if something did.
    ///
    /// A thin wrapper over [`Vault::update_journal_settings`], kept because
    /// it is in the plan's contract. **A caller that needs to change one
    /// field while leaving the rest exactly as they are must use
    /// `update_journal_settings` instead**: building the "rest as they are"
    /// half from a read taken before this call, rather than from the read
    /// this call takes under `settings_lock`, reopens the race
    /// `update_journal_settings` closes (see its doc comment, and the race
    /// test below).
    pub fn save_journal_settings(&self, settings: &JournalSettings) -> Result<(), VaultError> {
        let next = settings.clone();
        self.update_journal_settings(move |_current| Ok(Some(next.clone())))?;
        Ok(())
    }

    /// Reads the journal's settings, and — if `f` returns `Some` — validates,
    /// stamps and writes the result, all under `settings_lock`, then returns
    /// the resulting settings (unchanged, if `f` returned `None` or wrote
    /// nothing new).
    ///
    /// **This is the read-modify-write callers must use** to change one field
    /// while leaving the others as they are. `f` receives the freshly read
    /// current settings *from inside the lock*, so a concurrent
    /// [`Vault::merge_journal_settings`] landing between an external read and
    /// this call cannot make `f`'s starting point stale: there is no external
    /// read for it to be stale relative to. Contrast
    /// [`Vault::save_journal_settings`], whose caller must already hold a
    /// `next` for every field, which — if built from a read taken outside
    /// this lock — can be exactly that stale starting point.
    pub fn update_journal_settings(
        &self,
        f: impl FnOnce(&JournalSettings) -> Result<Option<JournalSettings>, SettingsError>,
    ) -> Result<JournalSettings, VaultError> {
        let _guard = self.settings_lock.lock().unwrap_or_else(|e| e.into_inner());

        let mut fields = self.read_stamped_fields()?;
        if let Some(next) = f(&fields.view())? {
            next.validate()?;
            if fields.record(&next, Utc::now()) {
                self.write_stamped_fields(&fields)?;
            }
        }
        Ok(fields.view())
    }

    /// Resolves a sync conflict on [`SETTINGS_BLOB`]: `ours` and `theirs` are
    /// the two sides' raw (encrypted) blob bytes.
    ///
    /// Takes `settings_lock`, then re-reads the blob actually stored now —
    /// `ours` is only what the sync engine read at the start of its pass, and
    /// can already be stale by the time this runs (a concurrent
    /// [`Vault::save_journal_settings`], say) — and writes
    /// `stored.merge(&ours).merge(&theirs)` unconditionally. `merge` is
    /// commutative and associative, so the order the three are combined in
    /// does not matter.
    ///
    /// A side that fails to decode is logged and dropped; the other alone
    /// stands in for it (merging a value with itself is a no-op, since `merge`
    /// picks the greatest `(changed_at, text)` per key). Both sides failing
    /// returns one side's decode error, so the sync engine leaves the
    /// conflict parked rather than overwriting the stored blob with nothing.
    pub fn merge_journal_settings(&self, ours: &[u8], theirs: &[u8]) -> Result<(), VaultError> {
        let _guard = self.settings_lock.lock().unwrap_or_else(|e| e.into_inner());

        let ours = self.decode_stamped_fields(ours);
        let theirs = self.decode_stamped_fields(theirs);
        for (side, result) in [("ours", &ours), ("theirs", &theirs)] {
            if let Err(e) = result {
                let error = e.to_string();
                log::warn!("{side} side of a settings merge failed to decode, merging without it: error={error}");
            }
        }

        let stored = self.read_stamped_fields()?;
        let merged = match (ours, theirs) {
            (Err(e), Err(_)) => return Err(e),
            (ours, theirs) => [ours, theirs]
                .into_iter()
                .flatten()
                .fold(stored, |acc, side| acc.merge(&side)),
        };
        self.write_stamped_fields(&merged)
    }

    /// The stored settings blob, decoded — or defaults, for a missing,
    /// corrupt or undecryptable one. Only a decrypt or parse failure is
    /// treated that way (logged, not an error); a store I/O error propagates,
    /// since silently treating a transient read failure as "nothing here"
    /// would let a save write a blob holding only the field it changed, and
    /// a merge lose whatever the re-read was supposed to combine in.
    fn read_stamped_fields(&self) -> Result<StampedFields, VaultError> {
        match self.read::<StampedFields>(&settings_blob_name()) {
            Ok(Some(fields)) => Ok(fields),
            Ok(None) => Ok(StampedFields::default()),
            Err(e @ (VaultError::Key(_) | VaultError::Serde(_))) => {
                let error = e.to_string();
                log::warn!(
                    "settings blob failed to decrypt or parse, reading as defaults: error={error}"
                );
                Ok(StampedFields::default())
            }
            Err(e) => Err(e),
        }
    }

    /// Decodes raw blob bytes as [`StampedFields`], as one side of a
    /// [`Vault::merge_journal_settings`] conflict — bytes that are not
    /// necessarily what is in this vault's own store.
    fn decode_stamped_fields(&self, ciphertext: &[u8]) -> Result<StampedFields, VaultError> {
        let plaintext = self.cipher.decrypt(ciphertext)?;
        Ok(serde_json::from_slice(&plaintext)?)
    }

    fn write_stamped_fields(&self, fields: &StampedFields) -> Result<(), VaultError> {
        let plaintext = serde_json::to_vec(fields)?;
        let ciphertext = self.cipher.encrypt(&plaintext)?;
        self.store.put(&settings_blob_name(), &ciphertext)?;
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::tests::{discard, fake_vault, passphrase, sample, temp_store};
    use journal_key::Cipher;
    use journal_settings::{TimeFormat, WeekStart};
    use journal_store::{BlobStore, StoreError};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn read_on_an_empty_store_returns_defaults() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        assert_eq!(
            vault.journal_settings().unwrap(),
            JournalSettings::default()
        );
        discard(&store);
    }

    #[test]
    fn save_then_read_round_trips() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let settings = JournalSettings {
            time_format: TimeFormat::H24,
            ..Default::default()
        };
        vault.save_journal_settings(&settings).unwrap();

        assert_eq!(vault.journal_settings().unwrap(), settings);
        discard(&store);
    }

    #[test]
    fn a_save_that_changes_nothing_does_not_write() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let settings = JournalSettings {
            time_format: TimeFormat::H24,
            ..Default::default()
        };
        vault.save_journal_settings(&settings).unwrap();
        let bytes_before = store.get(&settings_blob_name()).unwrap().unwrap();

        vault.save_journal_settings(&settings).unwrap();
        let bytes_after = store.get(&settings_blob_name()).unwrap().unwrap();

        assert_eq!(bytes_before, bytes_after);
        discard(&store);
    }

    #[test]
    fn an_invalid_value_is_refused_and_nothing_is_written() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let invalid = JournalSettings {
            entry_template: "a".repeat(10_001),
            ..Default::default()
        };
        assert!(vault.save_journal_settings(&invalid).is_err());
        assert!(store.get(&settings_blob_name()).unwrap().is_none());
        discard(&store);
    }

    #[test]
    fn a_corrupt_blob_reads_as_defaults_and_the_next_save_replaces_it() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        store
            .put(&settings_blob_name(), b"not valid ciphertext")
            .unwrap();
        assert_eq!(
            vault.journal_settings().unwrap(),
            JournalSettings::default()
        );

        let settings = JournalSettings {
            time_format: TimeFormat::H12,
            ..Default::default()
        };
        vault.save_journal_settings(&settings).unwrap();
        assert_eq!(vault.journal_settings().unwrap(), settings);
        discard(&store);
    }

    /// A store whose `get` always fails, the way a transient I/O error would:
    /// distinct from a corrupt blob, which `get` still returns the bytes of.
    struct FailingGet;

    impl BlobStore for FailingGet {
        fn get(&self, _: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
            Err(std::io::Error::other("disk read failed").into())
        }
        fn put(&self, _: &BlobName, _: &[u8]) -> Result<(), StoreError> {
            Ok(())
        }
        fn delete(&self, _: &BlobName) -> Result<(), StoreError> {
            Ok(())
        }
        fn list(&self) -> Result<Vec<BlobName>, StoreError> {
            Ok(Vec::new())
        }
        fn contains(&self, _: &BlobName) -> Result<bool, StoreError> {
            Ok(false)
        }
    }

    /// A store I/O error must not be swallowed as "nothing here yet": that
    /// would let a concurrent save or merge write a blob with only whatever
    /// it itself changed, dropping everything a proper read would have
    /// combined in.
    #[test]
    fn a_store_error_on_read_propagates_rather_than_reading_as_defaults() {
        let vault = Vault::with_cipher(Arc::new(FailingGet), Box::new(crate::tests::FakeCipher));

        let Err(err) = vault.journal_settings() else {
            panic!("a failing store read was silently treated as defaults");
        };
        assert!(matches!(err, VaultError::Store(_)), "got {err:?}");
    }

    /// The blob written for `settings` on `vault`, ready to hand to
    /// `merge_journal_settings` as one side of a conflict.
    fn raw_blob(
        vault: &Vault,
        store: &journal_fs_store::FsStore,
        settings: &JournalSettings,
    ) -> Vec<u8> {
        vault.save_journal_settings(settings).unwrap();
        store.get(&settings_blob_name()).unwrap().unwrap()
    }

    /// A stale-read stand-in for the sync engine's "ours": an empty
    /// `StampedFields`, encoded directly (not via a save, which would find
    /// nothing changed against defaults and write nothing).
    fn empty_blob() -> Vec<u8> {
        crate::tests::FakeCipher
            .encrypt(&serde_json::to_vec(&StampedFields::default()).unwrap())
            .unwrap()
    }

    #[test]
    fn merge_of_two_concurrent_edits_to_different_fields_keeps_both() {
        let store_a = temp_store();
        let vault_a = fake_vault(store_a.clone());
        let ours = raw_blob(
            &vault_a,
            &store_a,
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
        );

        let store_b = temp_store();
        let vault_b = fake_vault(store_b.clone());
        let theirs = raw_blob(
            &vault_b,
            &store_b,
            &JournalSettings {
                week_starts_on: WeekStart::Monday,
                ..Default::default()
            },
        );

        // The device resolving the conflict starts from a fresh, empty store.
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.merge_journal_settings(&ours, &theirs).unwrap();

        let merged = vault.journal_settings().unwrap();
        assert_eq!(merged.time_format, TimeFormat::H24);
        assert_eq!(merged.week_starts_on, WeekStart::Monday);

        discard(&store_a);
        discard(&store_b);
        discard(&store);
    }

    #[test]
    fn merge_of_the_same_field_keeps_the_later_edit() {
        let store_a = temp_store();
        let vault_a = fake_vault(store_a.clone());
        let ours = raw_blob(
            &vault_a,
            &store_a,
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
        );
        // A save on the same vault, later, wins the race for `changed_at`.
        let theirs = raw_blob(
            &vault_a,
            &store_a,
            &JournalSettings {
                time_format: TimeFormat::H12,
                ..Default::default()
            },
        );

        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.merge_journal_settings(&ours, &theirs).unwrap();

        assert_eq!(
            vault.journal_settings().unwrap().time_format,
            TimeFormat::H12
        );

        discard(&store_a);
        discard(&store);
    }

    #[test]
    fn first_sync_two_stores_with_no_settings_blob_saving_different_fields_merge_to_both() {
        // Neither store had a settings blob before either save: this is the
        // case `record` diffing against the view exists for.
        let store_a = temp_store();
        let vault_a = fake_vault(store_a.clone());
        let ours = raw_blob(
            &vault_a,
            &store_a,
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
        );

        let store_b = temp_store();
        let vault_b = fake_vault(store_b.clone());
        let theirs = raw_blob(
            &vault_b,
            &store_b,
            &JournalSettings {
                week_starts_on: WeekStart::Monday,
                ..Default::default()
            },
        );

        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.merge_journal_settings(&ours, &theirs).unwrap();

        let merged = vault.journal_settings().unwrap();
        assert_eq!(merged.time_format, TimeFormat::H24);
        assert_eq!(merged.week_starts_on, WeekStart::Monday);

        discard(&store_a);
        discard(&store_b);
        discard(&store);
    }

    #[test]
    fn entry_id_of_settings_blob_is_none_and_list_entries_ignores_it() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault
            .save_journal_settings(&JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            })
            .unwrap();
        vault.save_entry(&sample(crate::Id::new())).unwrap();

        assert_eq!(crate::entry_id(&settings_blob_name()), None);
        assert_eq!(vault.list_entries().unwrap().len(), 1);
        discard(&store);
    }

    #[test]
    fn open_over_a_store_holding_only_the_settings_blob_fails_with_missing_key_file() {
        let store = temp_store();
        // Written with a fake cipher, since only the blob's presence matters
        // here, not its content. A non-default value, so `record` actually
        // writes the blob rather than finding nothing changed.
        fake_vault(store.clone())
            .save_journal_settings(&JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            })
            .unwrap();

        let Err(err) = crate::Vault::open(store.clone(), &passphrase("whatever")) else {
            panic!("opened a keyless store holding only the settings blob");
        };
        assert!(
            matches!(err, VaultError::Key(journal_key::KeyError::MissingKeyFile)),
            "got {err:?}"
        );
        discard(&store);
    }

    #[test]
    fn a_save_and_a_merge_racing_from_two_threads_touching_different_fields_both_survive() {
        let store = temp_store();
        let vault = Arc::new(fake_vault(store.clone()));

        let ours_stub = empty_blob();

        // The other side of the conflict: a different device changing a
        // different field.
        let theirs_store = temp_store();
        let theirs_vault = fake_vault(theirs_store.clone());
        let theirs = raw_blob(
            &theirs_vault,
            &theirs_store,
            &JournalSettings {
                week_starts_on: WeekStart::Monday,
                ..Default::default()
            },
        );

        // Goes through `update_journal_settings`, whose closure reads the
        // current settings from *inside* the lock, so it never has a stale
        // opinion about `week_starts_on` to (re)write: whichever thread's
        // critical section runs first, the other sees its result and leaves
        // it alone.
        let save_vault = vault.clone();
        let save_thread = thread::spawn(move || {
            save_vault
                .update_journal_settings(|current| {
                    let mut next = current.clone();
                    next.time_format = TimeFormat::H24;
                    Ok(Some(next))
                })
                .unwrap();
        });

        let merge_vault = vault.clone();
        let merge_thread = thread::spawn(move || {
            merge_vault
                .merge_journal_settings(&ours_stub, &theirs)
                .unwrap();
        });

        save_thread.join().unwrap();
        merge_thread.join().unwrap();

        let result = vault.journal_settings().unwrap();
        assert_eq!(result.time_format, TimeFormat::H24);
        assert_eq!(result.week_starts_on, WeekStart::Monday);

        discard(&store);
        discard(&theirs_store);
    }

    /// A stale `ours`: the merge's `ours` argument was read before a
    /// concurrent save landed. Design says merging the freshly re-read
    /// stored copy in as a third side must keep the save's change alongside
    /// the merge's own.
    #[test]
    fn a_stale_ours_does_not_erase_a_save_that_landed_after_it_was_read() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        // `ours` is the engine's read of the empty blob, before field A is
        // saved below.
        let ours = empty_blob();

        // Field A (time_format) is saved after `ours` was captured.
        vault
            .save_journal_settings(&JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            })
            .unwrap();

        // `theirs` changes field B (week_starts_on).
        let theirs_store = temp_store();
        let theirs_vault = fake_vault(theirs_store.clone());
        let theirs = raw_blob(
            &theirs_vault,
            &theirs_store,
            &JournalSettings {
                week_starts_on: WeekStart::Monday,
                ..Default::default()
            },
        );

        vault.merge_journal_settings(&ours, &theirs).unwrap();

        let result = vault.journal_settings().unwrap();
        assert_eq!(result.time_format, TimeFormat::H24, "field A must survive");
        assert_eq!(
            result.week_starts_on,
            WeekStart::Monday,
            "field B must survive"
        );
        discard(&store);
        discard(&theirs_store);
    }

    /// A merge lands (field B), and only then does a caller change field A
    /// through `update_journal_settings`. The closure's `current` argument
    /// must already reflect the merge, so its `next` (built by cloning
    /// `current` and touching only field A) does not carry a stale value for
    /// field B back over the merge's result.
    #[test]
    fn update_after_a_merge_keeps_the_merges_field() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let ours = empty_blob();
        let theirs_store = temp_store();
        let theirs_vault = fake_vault(theirs_store.clone());
        let theirs = raw_blob(
            &theirs_vault,
            &theirs_store,
            &JournalSettings {
                week_starts_on: WeekStart::Monday,
                ..Default::default()
            },
        );
        vault.merge_journal_settings(&ours, &theirs).unwrap();

        let result = vault
            .update_journal_settings(|current| {
                let mut next = current.clone();
                next.time_format = TimeFormat::H24;
                Ok(Some(next))
            })
            .unwrap();

        assert_eq!(result.time_format, TimeFormat::H24);
        assert_eq!(
            result.week_starts_on,
            WeekStart::Monday,
            "the merge's field must survive"
        );
        discard(&store);
        discard(&theirs_store);
    }
}
