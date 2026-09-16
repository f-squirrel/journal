//! A journal: entries kept encrypted in a blob store.
//!
//! This is the layer that knows what an entry *is*: which blob holds it, how it
//! is serialized, which blobs in a store are entries at all, and — because that
//! last question is what makes the answer safe — whether a store is a journal
//! to open or an empty one to create. The layers below it know less on purpose:
//! a [`BlobStore`] knows only where bytes live, and a
//! [`Keyring`](journal_key::Keyring) only how to encrypt them (ADR 0009).

use std::sync::Arc;

use journal_entry::{Entry, Header, Id};
use journal_key::{Cipher, KeyError, Keyring};
use journal_store::{BlobName, BlobStore, StoreError};
use secrecy::SecretString;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("failed to (de)serialize entry: {0}")]
    Serde(#[from] serde_json::Error),
    /// Transparent on purpose: `journal-key`'s `#[error]` strings are already
    /// written for the user (P1-2), and a "vault error: " label in front of
    /// one would read as a developer's note wrapping a sentence.
    #[error(transparent)]
    Key(#[from] KeyError),
}

/// The extension every entry blob's name ends in.
///
/// It describes the plaintext inside, not the blob: what is stored is `age`
/// ciphertext wrapping this JSON. Kept as-is from ADR 0003 because renaming it
/// would make existing entries unreachable for no benefit.
const ENTRY_EXT: &str = ".json";

/// The blob an entry with this id lives in.
fn blob_name(id: &Id) -> BlobName {
    BlobName::try_from(format!("{id}{ENTRY_EXT}"))
        .expect("a uuid followed by an extension is a valid blob name")
}

/// Whether `name` is one of our entry blobs.
///
/// The single definition of that question. It used to have a second copy in
/// the crate below, which is how a rename here could silently disable the
/// orphan check in [`Vault::open`] — the two halves of that check now live
/// side by side.
fn is_entry(name: &BlobName) -> bool {
    name.ends_with(ENTRY_EXT)
}

/// A journal: the entries in one blob store, each encrypted at rest under the
/// store's own key.
///
/// The type the rest of the app means by "vault" — what `unlock` produces and
/// `lock` drops. The key itself is a [`Keyring`](journal_key::Keyring) one
/// crate down, which this type holds only as an opaque
/// [`Cipher`](journal_key::Cipher).
pub struct Vault {
    store: Arc<dyn BlobStore>,
    cipher: Box<dyn Cipher>,
}

impl Vault {
    /// Opens the journal in `store`, unlocking its key with `passphrase` and
    /// creating a vault if the store is empty.
    ///
    /// The only place the create-or-open decision is made. Getting it wrong
    /// means writing a fresh key over a store whose entries were sealed under a
    /// different one, orphaning every entry in it. A store holding entries but
    /// no key is exactly that case, and is refused with
    /// [`KeyError::MissingKeyFile`] rather than treated as a first run.
    ///
    /// This decision lives here, rather than in `journal-key` where it began,
    /// because it needs both halves of the question: whether there is a key —
    /// that crate's business — and whether there are entries, which only this
    /// layer can answer.
    pub fn open(store: Arc<dyn BlobStore>, passphrase: &SecretString) -> Result<Self, VaultError> {
        let key = if Keyring::exists(&*store)? {
            Keyring::unlock(&*store, passphrase)?
        } else if holds_entries(&*store)? {
            return Err(KeyError::MissingKeyFile.into());
        } else {
            Keyring::create(&*store, passphrase)?
        };
        Ok(Self::with_cipher(store, Box::new(key)))
    }

    /// Private so that the only cipher reachable from outside this crate is a
    /// real [`Keyring`]: the tests below stand a fake one up through here, and
    /// nothing else can.
    fn with_cipher(store: Arc<dyn BlobStore>, cipher: Box<dyn Cipher>) -> Self {
        Self { store, cipher }
    }

    /// Lists all entry headers, most recently updated first.
    ///
    /// Entries that fail to decrypt or parse are skipped and logged rather
    /// than failing the whole call.
    pub fn list_entries(&self) -> Result<Vec<Header>, VaultError> {
        let mut headers = Vec::new();
        for name in self.store.list()?.into_iter().filter(is_entry) {
            match self.read::<Header>(&name) {
                Ok(Some(header)) => headers.push(header),
                // Listed a moment ago and gone now. Not an error — a delete
                // racing a list is legitimate — but worth a line, because the
                // alternative reading is a store losing blobs.
                Ok(None) => log::warn!("entry blob disappeared while listing: blob={name}"),
                Err(e) => {
                    // Rendered before the `warn!` rather than inside it: `age`'s
                    // error `Display` lazily loads its i18n bundle and logs while
                    // doing so, and a sink that holds a lock across formatting its
                    // own record (fern's file target, say) would deadlock on the
                    // reentrant call. Formatting here keeps those records outside.
                    let error = e.to_string();
                    log::warn!(
                        "skipping entry that failed to decrypt or parse: blob={name}, error={error}"
                    );
                }
            }
        }
        headers.sort_by_key(|header| std::cmp::Reverse(header.updated_at));
        Ok(headers)
    }

    pub fn get_entry(&self, id: &Id) -> Result<Option<Entry>, VaultError> {
        self.read(&blob_name(id))
    }

    /// Creates or overwrites the entry with the given id.
    pub fn save_entry(&self, entry: &Entry) -> Result<(), VaultError> {
        let plaintext = serde_json::to_vec(entry)?;
        let ciphertext = self.cipher.encrypt(&plaintext)?;
        self.store.put(&blob_name(&entry.header.id), &ciphertext)?;
        Ok(())
    }

    pub fn delete_entry(&self, id: &Id) -> Result<(), VaultError> {
        Ok(self.store.delete(&blob_name(id))?)
    }

    /// Reads one blob and decodes it, or `None` if there is no such blob.
    ///
    /// Generic over the shape decoded because a full [`Entry`] and a bare
    /// [`Header`] come out of the same bytes — the header is flattened into the
    /// entry, so the smaller type parses the same JSON and skips the markdown.
    fn read<T: serde::de::DeserializeOwned>(
        &self,
        name: &BlobName,
    ) -> Result<Option<T>, VaultError> {
        let Some(ciphertext) = self.store.get(name)? else {
            return Ok(None);
        };
        let plaintext = self.cipher.decrypt(&ciphertext)?;
        Ok(Some(serde_json::from_slice(&plaintext)?))
    }
}

/// Whether `store` holds at least one entry blob.
fn holds_entries(store: &dyn BlobStore) -> Result<bool, VaultError> {
    Ok(store.list()?.iter().any(is_entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use journal_fs_store::FsStore;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A store in a directory no other test in this process will pick.
    fn temp_store() -> Arc<FsStore> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        Arc::new(FsStore::new(
            std::env::temp_dir().join(format!("journal-vault-test-{}-{n}", std::process::id())),
        ))
    }

    fn discard(store: &FsStore) {
        fs::remove_dir_all(store.dir()).ok();
    }

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    /// A stand-in for [`Keyring`] for the tests that are not about encryption.
    ///
    /// Reversible without a key, so these tests cost no scrypt: a `Vault` over
    /// a real keyring pays ~0.4s to open, for coverage of entry naming,
    /// listing and serde that has nothing to do with cryptography.
    struct FakeCipher;

    /// What makes `decrypt` fail for bytes this cipher did not write. Without a
    /// marker, XOR would happily "decrypt" a corrupt blob into garbage and the
    /// skip-corrupt tests below would prove nothing.
    const FAKE_MARKER: &[u8] = b"fake:";

    impl Cipher for FakeCipher {
        fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, KeyError> {
            let mut out = FAKE_MARKER.to_vec();
            out.extend(plaintext.iter().map(|b| b ^ 0x5A));
            Ok(out)
        }

        fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, KeyError> {
            let body = ciphertext
                .strip_prefix(FAKE_MARKER)
                .ok_or_else(KeyError::not_ciphertext)?;
            Ok(body.iter().map(|b| b ^ 0x5A).collect())
        }
    }

    /// A `Vault` whose entries are cheaply reversible rather than encrypted.
    fn fake_vault(store: Arc<FsStore>) -> Vault {
        Vault::with_cipher(store, Box::new(FakeCipher))
    }

    fn sample(id: Id) -> Entry {
        Entry {
            header: Header {
                id,
                title: "Test entry".to_string(),
                updated_at: Utc::now(),
            },
            markdown: "# hello".to_string(),
        }
    }

    #[test]
    fn save_get_list_delete_roundtrip() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        assert!(vault.list_entries().unwrap().is_empty());

        let a = Id::new();
        let b = Id::new();
        vault.save_entry(&sample(a)).unwrap();
        vault.save_entry(&sample(b)).unwrap();

        let fetched = vault.get_entry(&a).unwrap().unwrap();
        assert_eq!(fetched.markdown, "# hello");

        let headers = vault.list_entries().unwrap();
        assert_eq!(headers.len(), 2);
        assert!(headers.iter().all(|header| header.title == "Test entry"));

        vault.delete_entry(&a).unwrap();
        assert!(vault.get_entry(&a).unwrap().is_none());
        assert_eq!(vault.list_entries().unwrap().len(), 1);

        discard(&store);
    }

    #[test]
    fn get_missing_entry_returns_none() {
        let store = temp_store();
        assert!(fake_vault(store.clone())
            .get_entry(&Id::new())
            .unwrap()
            .is_none());
    }

    #[test]
    fn deleting_an_entry_twice_is_not_an_error() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();
        vault.save_entry(&sample(id)).unwrap();
        vault.delete_entry(&id).unwrap();
        vault.delete_entry(&id).unwrap();
        discard(&store);
    }

    #[test]
    fn saving_the_same_id_twice_overwrites_rather_than_duplicating() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        vault.save_entry(&sample(id)).unwrap();
        let mut edited = sample(id);
        edited.markdown = "# edited".to_string();
        vault.save_entry(&edited).unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);
        assert_eq!(vault.get_entry(&id).unwrap().unwrap().markdown, "# edited");
        discard(&store);
    }

    #[test]
    fn list_entries_is_most_recently_updated_first() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        let old = Id::new();
        let new = Id::new();
        let mut older = sample(old);
        older.header.updated_at = Utc::now() - chrono::Duration::hours(1);
        older.header.title = "older".to_string();
        let mut newer = sample(new);
        newer.header.title = "newer".to_string();

        vault.save_entry(&older).unwrap();
        vault.save_entry(&newer).unwrap();

        let titles: Vec<_> = vault
            .list_entries()
            .unwrap()
            .into_iter()
            .map(|h| h.title)
            .collect();
        assert_eq!(titles, ["newer", "older"]);
        discard(&store);
    }

    #[test]
    fn list_entries_skips_corrupt_blobs_without_failing() {
        let store = temp_store();
        let vault = fake_vault(store.clone());

        vault.save_entry(&sample(Id::new())).unwrap();
        store
            .put(&blob_name(&Id::new()), b"not valid ciphertext")
            .unwrap();

        let headers = vault.list_entries().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].title, "Test entry");

        discard(&store);
    }

    #[test]
    fn list_entries_ignores_blobs_that_are_not_entries() {
        // The vault's own key blob shares the store with the entries, so a
        // wrong answer here would try to decrypt the key as an entry and warn
        // about it on every launch.
        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&sample(Id::new())).unwrap();
        store
            .put(
                &BlobName::literal(".vault-key"),
                b"sealed key, not an entry",
            )
            .unwrap();
        store
            .put(&BlobName::literal("notes.txt"), b"something else entirely")
            .unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);
        discard(&store);
    }

    #[test]
    fn a_store_that_was_never_written_lists_no_entries() {
        // No directory yet, and reading must not create one: `has_vault` asks
        // this question before the user has typed a passphrase.
        let store = temp_store();
        assert!(fake_vault(store.clone()).list_entries().unwrap().is_empty());
        assert!(!store.dir().exists());
    }

    #[test]
    fn open_creates_a_vault_then_reopens_it() {
        // The real thing, scrypt and all: two unlocks, proving an entry written
        // in one session is readable in the next.
        let store = temp_store();
        let id = Id::new();

        let first = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        first.save_entry(&sample(id)).unwrap();

        let second = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        assert_eq!(second.get_entry(&id).unwrap().unwrap().markdown, "# hello");

        discard(&store);
    }

    #[test]
    fn open_with_the_wrong_passphrase_does_not_open_the_journal() {
        let store = temp_store();
        Vault::open(store.clone(), &passphrase("hunter2")).unwrap();

        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("the wrong passphrase opened the journal");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::WrongPassphrase)),
            "got {err:?}"
        );

        discard(&store);
    }

    #[test]
    fn open_refuses_when_the_key_is_missing_but_entries_remain() {
        // The case that must never be read as a first run: creating a fresh
        // vault here would seal a new key over entries encrypted to the old
        // one, and every entry would be unreadable forever.
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        vault.save_entry(&sample(Id::new())).unwrap();
        store.delete(&BlobName::literal(".vault-key")).unwrap();

        let Err(err) = Vault::open(store.clone(), &passphrase("typo")) else {
            panic!("opened a journal whose key is gone");
        };
        assert!(
            matches!(err, VaultError::Key(KeyError::MissingKeyFile)),
            "got {err:?}"
        );
        // The assertion that matters: an error that still wrote a key would
        // have orphaned the entry anyway.
        assert!(!Keyring::exists(&*store).unwrap());

        discard(&store);
    }

    #[test]
    fn a_directory_named_like_an_entry_does_not_block_creating_a_vault() {
        let store = temp_store();
        fs::create_dir_all(store.dir().join(blob_name(&Id::new()).as_str())).unwrap();

        Vault::open(store.clone(), &passphrase("fresh start")).unwrap();
        assert!(Keyring::exists(&*store).unwrap());

        discard(&store);
    }

    /// P1-2: `src-tauri` renders a command's error with `to_string()` and the
    /// frontend shows that string verbatim, so these are user-facing copy and
    /// not developer notes. Two properties: the passphrase failure reads like
    /// copy, and `journal-key`'s own wording reaches the user unprefixed —
    /// wrapping it in "vault error: …" would put a developer's label in front
    /// of a sentence written for the user.
    #[test]
    fn error_messages_are_user_facing() {
        assert_eq!(
            VaultError::from(KeyError::WrongPassphrase).to_string(),
            "Incorrect passphrase."
        );

        for err in [KeyError::MissingKeyFile, KeyError::CorruptKeyFile] {
            let copy = err.to_string();
            assert!(
                copy.starts_with(|c: char| c.is_uppercase()),
                "{copy:?} is shown to the user as written"
            );
            assert_eq!(VaultError::from(err).to_string(), copy);
        }
    }

    /// P1-4 guard (green before and after — atomicity is `rename`'s guarantee,
    /// and racing threads can only ever fail to disprove it). Since P1-1 two
    /// saves for the same id can interleave, so the store writes to scratch and
    /// renames; this asserts the invariant that leaves behind, one layer up
    /// from where the rename happens.
    #[test]
    fn save_entry_leaves_no_temp_file() {
        let store = temp_store();
        let vault = fake_vault(store.clone());
        let id = Id::new();

        vault.save_entry(&sample(id)).unwrap();
        let mut second = sample(id);
        second.header.title = "Second".into();
        vault.save_entry(&second).unwrap();

        assert_eq!(
            vault.get_entry(&id).unwrap().unwrap().header.title,
            "Second"
        );
        assert_eq!(vault.list_entries().unwrap().len(), 1);
        assert!(
            !fs::read_dir(store.dir()).unwrap().any(|f| f
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")),
            "a scratch file was left in the vault directory"
        );

        discard(&store);
    }

    #[test]
    fn entries_are_encrypted_at_rest() {
        // The one test here that uses a real vault for the entries themselves:
        // everything above stands a fake cipher up in its place, so this is
        // what holds the property that the bytes on disk are unreadable.
        let store = temp_store();
        let vault = Vault::open(store.clone(), &passphrase("hunter2")).unwrap();
        let id = Id::new();
        vault.save_entry(&sample(id)).unwrap();

        let on_disk = fs::read(store.dir().join(blob_name(&id).as_str())).unwrap();
        let as_text = String::from_utf8_lossy(&on_disk);
        assert!(!as_text.contains("Test entry"));
        assert!(!as_text.contains("hello"));

        discard(&store);
    }

    static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// `log`'s logger is global and install-once, so
    /// `list_entries_logs_the_blob_it_skipped` is the only test in this crate
    /// allowed to install one.
    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            // Formatted before the lock is taken: formatting a record can emit
            // records of its own, and `CAPTURED` is not reentrant.
            let message = record.args().to_string();
            CAPTURED.lock().unwrap().push(message);
        }

        fn flush(&self) {}
    }

    #[test]
    fn list_entries_logs_the_blob_it_skipped() {
        log::set_logger(&CapturingLogger).expect("only test installing a logger");
        log::set_max_level(log::LevelFilter::Trace);

        let store = temp_store();
        let vault = fake_vault(store.clone());
        vault.save_entry(&sample(Id::new())).unwrap();

        let corrupt = blob_name(&Id::new());
        store.put(&corrupt, b"not valid ciphertext").unwrap();

        assert_eq!(vault.list_entries().unwrap().len(), 1);

        let captured = CAPTURED.lock().unwrap().join("\n");
        assert!(
            captured.contains(corrupt.as_str()),
            "no log record named the skipped blob; captured: {captured:?}"
        );

        discard(&store);
    }
}
