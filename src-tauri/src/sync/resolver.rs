//! What a conflict means, decided where the key is (D1).
//!
//! The sync engine cannot tell two versions of an entry apart beyond
//! "different", so when two devices edited the same entry it parks the other
//! side's bytes and asks. The answer needs plaintext, so it comes from here: a
//! resolver over the unlocked vault.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use journal_entry::{Entry, Header, Id};
use journal_store::BlobName;
use journal_sync::{ConflictResolver, Resolution};
use journal_vault::{entry_id, Vault, VaultError};

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
        let Some(vault) = (self.vault)() else {
            return Resolution::Deferred;
        };
        match merge(&vault, name, ours, theirs) {
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

    let copy = Entry {
        header: Header {
            id: Id::new(),
            title: format!("{}{COPY_SUFFIX}", loser.header.title),
            // The copy sorts where the version it holds was written.
            updated_at: loser.header.updated_at,
            // The same day the losing version was about: a conflicted copy is
            // another take on one entry, not a new one written today.
            entry_date: loser.header.entry_date,
        },
        markdown: loser.markdown,
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
}
