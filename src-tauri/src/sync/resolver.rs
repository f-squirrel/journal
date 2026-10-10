//! What a conflict means, decided where the key is (D1).
//!
//! The sync engine cannot tell two versions of an entry apart beyond
//! "different", so when two devices edited the same entry it parks the other
//! side's bytes and asks. The answer needs plaintext, so it comes from here: a
//! resolver over the unlocked vault.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use journal_api::{Entry, Header, Id};
use journal_store::BlobName;
use journal_sync_engine::{ConflictResolver, Resolution};
use journal_vault::{entry_id, is_media, is_snapshot, Vault, VaultError, SETTINGS_BLOB};

/// The unlocked vault, if there is one, as a handle the caller owns.
///
/// A closure rather than the state itself so this module needs no idea where
/// the vault lives. Whatever implements it must not hand out anything that
/// keeps the lock state's mutex held: resolving does crypto and disk writes,
/// and a `lock` (logout) must not wait for them.
pub type VaultHandle = Arc<dyn Fn() -> Option<Arc<Vault>> + Send + Sync>;

const COPY_SUFFIX: &str = " (conflicted copy)";

/// Keeps the newer version of a conflicted entry under its own id, and the
/// older as a new entry beside it. Never last-writer-wins: no text is dropped.
pub struct VaultResolver {
    vault: VaultHandle,
    /// Conflicts settled since the worker last asked, to tell it that entries
    /// on screen may have changed.
    resolved: AtomicUsize,
}

impl VaultResolver {
    pub fn new(vault: VaultHandle) -> Self {
        Self {
            vault,
            resolved: AtomicUsize::new(0),
        }
    }

    /// How many conflicts were settled since the last call.
    pub fn take_resolved(&self) -> usize {
        self.resolved.swap(0, Ordering::SeqCst)
    }
}

impl ConflictResolver for VaultResolver {
    /// `Deferred` while the vault is locked, and for anything that cannot be
    /// read as an entry: the conflict stays parked and nothing is lost.
    ///
    /// The copy is saved before the original is replaced, so a crash between
    /// the two leaves both versions somewhere rather than one. The engine may
    /// offer the same conflict again after such a crash, which makes a second
    /// copy: at-least-once, and a duplicate is the better failure.
    fn resolve(&self, name: &BlobName, ours: &[u8], theirs: &[u8]) -> Resolution {
        // Same name means the same version of the same entry: either
        // device's ciphertext of it is correct, so keep the one already in
        // the store. Media is immutable, so same name means the same bytes.
        // Checked before the vault, on purpose: keeping ours needs no key,
        // so there's no reason to leave the conflict parked while the vault
        // is locked.
        if is_snapshot(name) || is_media(name) {
            self.resolved.fetch_add(1, Ordering::SeqCst);
            return Resolution::Resolved;
        }
        let Some(vault) = (self.vault)() else {
            return Resolution::Deferred;
        };
        let result = if *name == BlobName::literal(SETTINGS_BLOB) {
            vault
                .merge_journal_settings(ours, theirs)
                .map_err(|e| e.to_string())
        } else {
            merge(&vault, name, ours, theirs)
        };
        match result {
            Ok(()) => {
                self.resolved.fetch_add(1, Ordering::SeqCst);
                Resolution::Resolved
            }
            Err(error) => {
                log::warn!("could not resolve a sync conflict, leaving it parked: blob={name}, error={error}");
                Resolution::Deferred
            }
        }
    }
}

/// The error is text because it is only logged: `age`'s `Display` is rendered
/// here, before the `warn!` (see `Vault::list_entries` for why).
fn merge(vault: &Vault, name: &BlobName, ours: &[u8], theirs: &[u8]) -> Result<(), String> {
    let text = |e: VaultError| e.to_string();
    let ours = vault.decode_entry(ours).map_err(text)?;
    let theirs = vault.decode_entry(theirs).map_err(text)?;
    // The result is written under `name`, so a blob holding some other id is
    // not something to merge.
    if entry_id(name) != Some(ours.header.id) || ours.header.id != theirs.header.id {
        return Err("the entry's id does not match the blob it is in".to_owned());
    }

    if same_text(&ours, &theirs) {
        // Both devices got to the same place; only the ciphertext differs.
        return Ok(());
    }
    // A tie keeps ours, which is already in the store.
    let theirs_wins = theirs.header.updated_at > ours.header.updated_at;
    let (winner, loser) = if theirs_wins {
        (theirs, ours)
    } else {
        (ours, theirs)
    };

    let copy_id = Id::new();
    // The copy owns the media its text shows, so deleting the original later
    // cannot take the copy's images with it. Remaining gap: the loser's media
    // may not have been pulled yet, since entry names sort before media
    // names; refs to media that is not here yet, or to a non-web-safe
    // original whose thumbnail has not arrived, keep pointing at the
    // original.
    let markdown = vault.adopt_media(&copy_id, &loser.markdown).map_err(text)?;
    let copy = Entry {
        header: Header {
            id: copy_id,
            title: format!("{}{COPY_SUFFIX}", loser.header.title),
            // The copy sorts where the version it holds was written.
            updated_at: loser.header.updated_at,
            // The same day the losing version was about: a conflicted copy is
            // another take on one entry, not a new one written today.
            entry_date: loser.header.entry_date,
        },
        markdown,
    };
    vault.save_entry(&copy).map_err(text)?;
    if theirs_wins {
        vault.save_entry(&winner).map_err(text)?;
    }
    Ok(())
}

fn same_text(a: &Entry, b: &Entry) -> bool {
    a.header.title == b.header.title
        && a.header.updated_at == b.header.updated_at
        && a.markdown == b.markdown
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::AtomicUsize;

    use chrono::{Duration, Utc};
    use journal_fs_store::FsStore;
    use journal_store::BlobStore;
    use secrecy::SecretString;

    use super::*;

    /// A vault over a store nothing else uses, and the store itself.
    fn vault() -> (Arc<Vault>, Arc<FsStore>, PathBuf) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("journal-resolver-test-{}-{n}", std::process::id()));
        let store = Arc::new(FsStore::new(dir.clone()));
        let vault = Vault::open(store.clone(), &SecretString::from("pass".to_owned())).unwrap();
        (Arc::new(vault), store, dir)
    }

    fn entry(id: Id, title: &str, markdown: &str, age_minutes: i64) -> Entry {
        Entry {
            header: Header {
                id,
                title: title.to_owned(),
                updated_at: Utc::now() - Duration::minutes(age_minutes),
                entry_date: Utc::now() - Duration::minutes(age_minutes),
            },
            markdown: markdown.to_owned(),
        }
    }

    /// The bytes an entry has on disk, sealed under the vault's key.
    fn sealed(vault: &Vault, store: &FsStore, entry: &Entry) -> Vec<u8> {
        vault.save_entry(entry).unwrap();
        let name = BlobName::try_from(format!("{}.json", entry.header.id)).unwrap();
        store.get(&name).unwrap().unwrap()
    }

    fn resolver(vault: &Arc<Vault>) -> VaultResolver {
        let vault = vault.clone();
        VaultResolver::new(Arc::new(move || Some(vault.clone())))
    }

    fn name_of(id: Id) -> BlobName {
        BlobName::try_from(format!("{id}.json")).unwrap()
    }

    /// The vault with the two versions' entry and whatever the resolver made.
    fn titles(vault: &Vault) -> Vec<String> {
        let mut titles: Vec<_> = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|h| h.title)
            .collect();
        titles.sort();
        titles
    }

    #[test]
    fn the_newer_version_keeps_the_id_and_the_older_becomes_a_copy() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        // Theirs is newer. Ours is what the local store holds.
        let theirs = sealed(&vault, &store, &entry(id, "phone", "written later", 1));
        let ours = sealed(&vault, &store, &entry(id, "laptop", "written first", 30));
        let resolver = resolver(&vault);

        assert_eq!(
            resolver.resolve(&name_of(id), &ours, &theirs),
            Resolution::Resolved
        );

        let kept = vault.get_entry(&id).unwrap().unwrap();
        assert_eq!(
            (kept.header.title.as_str(), kept.markdown.as_str()),
            ("phone", "written later")
        );
        assert_eq!(
            titles(&vault),
            ["laptop (conflicted copy)", "phone"],
            "the older version survives as a copy"
        );
        let copy = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .find(|h| h.title.ends_with(COPY_SUFFIX))
            .unwrap();
        assert_ne!(copy.id, id);
        assert_eq!(
            vault.get_entry(&copy.id).unwrap().unwrap().markdown,
            "written first"
        );
        assert_eq!(resolver.take_resolved(), 1);
        assert_eq!(resolver.take_resolved(), 0);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn when_ours_is_newer_it_stays_and_theirs_becomes_the_copy() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let theirs = sealed(&vault, &store, &entry(id, "phone", "old text", 30));
        let ours = sealed(&vault, &store, &entry(id, "laptop", "new text", 1));

        assert_eq!(
            resolver(&vault).resolve(&name_of(id), &ours, &theirs),
            Resolution::Resolved
        );

        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "new text");
        assert_eq!(titles(&vault), ["laptop", "phone (conflicted copy)"]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_tie_keeps_ours() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let mut a = entry(id, "laptop", "ours", 5);
        let mut b = entry(id, "phone", "theirs", 5);
        b.header.updated_at = a.header.updated_at;
        a.header.title = "laptop".into();
        let theirs = sealed(&vault, &store, &b);
        let ours = sealed(&vault, &store, &a);

        resolver(&vault).resolve(&name_of(id), &ours, &theirs);

        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "ours");
        assert_eq!(titles(&vault), ["laptop", "phone (conflicted copy)"]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn two_versions_with_the_same_text_make_no_copy() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let e = entry(id, "same", "same text", 5);
        // Sealed twice: the ciphertexts differ, the entries do not.
        let theirs = sealed(&vault, &store, &e);
        let ours = sealed(&vault, &store, &e);
        assert_ne!(ours, theirs);

        assert_eq!(
            resolver(&vault).resolve(&name_of(id), &ours, &theirs),
            Resolution::Resolved
        );

        assert_eq!(titles(&vault), ["same"]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_locked_vault_defers_and_touches_nothing() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let theirs = sealed(&vault, &store, &entry(id, "phone", "t", 1));
        let ours = sealed(&vault, &store, &entry(id, "laptop", "o", 30));
        let locked = VaultResolver::new(Arc::new(|| None));

        assert_eq!(
            locked.resolve(&name_of(id), &ours, &theirs),
            Resolution::Deferred
        );

        assert_eq!(titles(&vault), ["laptop"]);
        assert_eq!(locked.take_resolved(), 0);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn bytes_that_are_not_an_entry_are_left_parked() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let ours = sealed(&vault, &store, &entry(id, "laptop", "o", 30));

        assert_eq!(
            resolver(&vault).resolve(&name_of(id), &ours, b"not ciphertext"),
            Resolution::Deferred
        );
        assert_eq!(titles(&vault), ["laptop"]);

        std::fs::remove_dir_all(dir).ok();
    }

    /// `save_entry` also snapshots on a `theirs_wins`
    /// overwrite, since `merge` writes the winner through it. With a gap of
    /// `SNAPSHOT_MIN_AGE` or more, the loser is expected to exist twice — as
    /// the conflicted copy `merge` always writes, and as a snapshot of the
    /// version the winner overwrote — and that's not a duplicate to
    /// eliminate.
    #[test]
    fn a_conflict_older_than_the_snapshot_throttle_leaves_the_loser_recoverable_as_a_snapshot() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        // Ours is old enough that overwriting it crosses the snapshot
        // throttle; theirs is newer and wins.
        let ours = sealed(&vault, &store, &entry(id, "laptop", "written first", 45));
        let theirs = sealed(&vault, &store, &entry(id, "phone", "written later", 1));
        // Put the store back the way it would be before the conflict is
        // resolved: holding "ours", the local version — `sealed` wrote
        // "theirs" last, as a side effect of producing its ciphertext.
        store.put(&name_of(id), &ours).unwrap();

        assert_eq!(
            resolver(&vault).resolve(&name_of(id), &ours, &theirs),
            Resolution::Resolved
        );

        // The winner keeps the id.
        let kept = vault.get_entry(&id).unwrap().unwrap();
        assert_eq!(kept.markdown, "written later");
        // The loser survives as the conflicted copy `merge` always writes...
        assert_eq!(
            titles(&vault),
            ["laptop (conflicted copy)", "phone"],
            "the resolver's own copy-on-conflict behaviour is unchanged"
        );
        // ...and, separately, as a snapshot `save_entry` took of the version
        // it overwrote.
        let history = vault.list_history(&id).unwrap();
        assert_eq!(
            history.len(),
            1,
            "the overwritten version should be recoverable"
        );
        assert_eq!(
            vault
                .get_history_entry(&id, history[0].updated_at)
                .unwrap()
                .unwrap()
                .markdown,
            "written first"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_entry_in_a_blob_named_for_another_id_is_left_parked() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let theirs = sealed(&vault, &store, &entry(id, "phone", "t", 1));
        let ours = sealed(&vault, &store, &entry(id, "laptop", "o", 30));

        assert_eq!(
            resolver(&vault).resolve(&name_of(Id::new()), &ours, &theirs),
            Resolution::Deferred
        );

        std::fs::remove_dir_all(dir).ok();
    }

    /// Two devices' independent encryptions of the same
    /// snapshot collide on name; the resolver keeps ours without needing a
    /// key, rather than leaving the rail stuck on "Conflicts" forever.
    #[test]
    fn a_snapshot_name_conflict_keeps_ours_and_needs_no_key() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let name = BlobName::try_from(format!(
            "{id}.snap-{:016}.json",
            Utc::now().timestamp_millis()
        ))
        .unwrap();
        let ours_bytes = b"ours-ciphertext".to_vec();
        let theirs_bytes = b"theirs-ciphertext".to_vec();
        store.put(&name, &ours_bytes).unwrap();

        assert_eq!(
            resolver(&vault).resolve(&name, &ours_bytes, &theirs_bytes),
            Resolution::Resolved
        );
        assert_eq!(store.get(&name).unwrap().unwrap(), ours_bytes);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_snapshot_name_conflict_resolves_even_while_the_vault_is_locked() {
        let locked = VaultResolver::new(Arc::new(|| None));
        let name = BlobName::try_from(format!(
            "{}.snap-{:016}.json",
            Id::new(),
            Utc::now().timestamp_millis()
        ))
        .unwrap();

        assert_eq!(
            locked.resolve(&name, b"ours", b"theirs"),
            Resolution::Resolved
        );
        assert_eq!(locked.take_resolved(), 1);
    }

    fn webp(body: &[u8]) -> Vec<u8> {
        [b"RIFF\0\0\0\0WEBP".as_slice(), body].concat()
    }

    fn media_meta() -> journal_api::MediaMeta {
        journal_api::MediaMeta::Image {
            format: journal_api::ImageFormat::Webp,
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn a_media_conflict_resolves_keeping_ours_while_locked() {
        let locked = VaultResolver::new(Arc::new(|| None));
        let (id, m) = (Id::new(), journal_api::MediaId::new());
        for suffix in ["", ".display", ".thumb"] {
            let name = BlobName::try_from(format!("{id}.media-{m}{suffix}")).unwrap();
            assert_eq!(
                locked.resolve(&name, b"ours", b"theirs"),
                Resolution::Resolved,
                "{suffix}"
            );
        }
        assert_eq!(locked.take_resolved(), 3);
    }

    #[test]
    fn a_media_conflict_does_not_create_a_conflicted_copy() {
        let (vault, store, dir) = vault();
        let (id, m) = (Id::new(), journal_api::MediaId::new());
        vault
            .save_media(
                &id,
                &m,
                journal_api::MediaVariant::Original,
                &media_meta(),
                &webp(b"px"),
            )
            .unwrap();
        let name = BlobName::try_from(format!("{id}.media-{m}")).unwrap();
        let ours = store.get(&name).unwrap().unwrap();

        assert_eq!(
            resolver(&vault).resolve(&name, &ours, b"theirs"),
            Resolution::Resolved
        );
        assert_eq!(store.get(&name).unwrap().unwrap(), ours);
        assert!(titles(&vault).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_conflicted_copy_adopts_the_losers_media() {
        let (vault, store, dir) = vault();
        let (id, m) = (Id::new(), journal_api::MediaId::new());
        vault
            .save_media(
                &id,
                &m,
                journal_api::MediaVariant::Original,
                &media_meta(),
                &webp(b"px"),
            )
            .unwrap();
        let link = format!("![](media:{id}/{m})");
        let theirs = sealed(&vault, &store, &entry(id, "phone", "later", 1));
        let ours = sealed(&vault, &store, &entry(id, "laptop", &link, 30));

        resolver(&vault).resolve(&name_of(id), &ours, &theirs);

        let copy = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .find(|h| h.id != id)
            .unwrap();
        let copy_entry = vault.get_entry(&copy.id).unwrap().unwrap();
        assert_eq!(copy_entry.markdown, format!("![](media:{}/{m})", copy.id));
        assert_eq!(
            vault
                .get_media(&copy.id, &m, journal_api::MediaVariant::Original)
                .unwrap()
                .unwrap()
                .bytes,
            webp(b"px")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_conflicted_copy_keeps_refs_to_media_not_yet_pulled() {
        let (vault, store, dir) = vault();
        let id = Id::new();
        let link = format!("![](media:{id}/{})", journal_api::MediaId::new());
        let theirs = sealed(&vault, &store, &entry(id, "phone", "later", 1));
        let ours = sealed(&vault, &store, &entry(id, "laptop", &link, 30));

        resolver(&vault).resolve(&name_of(id), &ours, &theirs);

        let copy = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .find(|h| h.id != id)
            .unwrap();
        assert_eq!(vault.get_entry(&copy.id).unwrap().unwrap().markdown, link);
        std::fs::remove_dir_all(dir).ok();
    }

    fn settings_blob_name() -> BlobName {
        BlobName::literal(SETTINGS_BLOB)
    }

    /// The bytes the settings blob has on disk after saving `settings` from
    /// an empty store — one independent "side" of a conflict, sealed under
    /// the vault's own key so the same vault can decrypt it back.
    fn raw_settings(
        vault: &Vault,
        store: &FsStore,
        settings: &journal_settings::JournalSettings,
    ) -> Vec<u8> {
        vault.save_journal_settings(settings).unwrap();
        store.get(&settings_blob_name()).unwrap().unwrap()
    }

    #[test]
    fn a_settings_conflict_merges_both_fields_and_counts_as_resolved() {
        use journal_settings::{JournalSettings, TimeFormat, WeekStart};

        let (vault, store, dir) = vault();
        let ours = raw_settings(
            &vault,
            &store,
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
        );
        // Reset to the same empty starting point "ours" was saved from,
        // so "theirs" is an independent change rather than one stacked on
        // top of "ours".
        store.delete(&settings_blob_name()).unwrap();
        let theirs = raw_settings(
            &vault,
            &store,
            &JournalSettings {
                week_starts_on: WeekStart::Monday,
                ..Default::default()
            },
        );
        let resolver = resolver(&vault);

        assert_eq!(
            resolver.resolve(&settings_blob_name(), &ours, &theirs),
            Resolution::Resolved
        );

        let merged = vault.journal_settings().unwrap();
        assert_eq!(merged.time_format, TimeFormat::H24);
        assert_eq!(merged.week_starts_on, WeekStart::Monday);
        assert_eq!(resolver.take_resolved(), 1);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_settings_conflict_still_defers_while_locked() {
        use journal_settings::{JournalSettings, TimeFormat};

        let (vault, store, dir) = vault();
        let ours = raw_settings(
            &vault,
            &store,
            &JournalSettings {
                time_format: TimeFormat::H24,
                ..Default::default()
            },
        );
        let theirs = ours.clone();
        let locked = VaultResolver::new(Arc::new(|| None));

        assert_eq!(
            locked.resolve(&settings_blob_name(), &ours, &theirs),
            Resolution::Deferred
        );
        assert_eq!(locked.take_resolved(), 0);

        std::fs::remove_dir_all(dir).ok();
    }
}
