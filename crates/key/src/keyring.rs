use journal_store::{BlobStore, StoreError};
use secrecy::SecretString;

use crate::identity::{key_blob, Identity};
use crate::{Cipher, KeyError};

/// The X25519 keypair every entry in a store is encrypted to, and the sealed
/// blob it lives in.
///
/// Deliberately *only* the key: it holds no entries, owns no directory, and
/// cannot tell you whether a store has a journal in it — that is
/// [`Vault`](../journal_vault/struct.Vault.html)'s job, one crate up. This type
/// was once called `Vault` itself, which put the user-facing word on the layer
/// that knows the least about what a user has (ADR 0009 moved the directory to
/// the store and the entry naming to the layer above; the name followed later).
///
/// The passphrase does not appear as a field. It protects the sealed key blob
/// and nothing else, so it is needed exactly once — at [`Self::unlock`] or
/// [`Self::create`] — and entry encryption never touches it again.
#[derive(Clone)]
pub struct Keyring {
    key: Identity,
}

/// Redacted by hand rather than derived: the whole point of the type is that it
/// holds the key to every entry, and `age`'s `Identity` has no `Debug` of its
/// own precisely so one cannot be printed by accident.
impl std::fmt::Debug for Keyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keyring")
            .field("key", &self.key.public())
            .finish_non_exhaustive()
    }
}

impl Keyring {
    /// Whether `store` already holds a sealed key.
    ///
    /// Associated rather than a method: it answers a question about a store that
    /// may hold no key at all, which is exactly when there is no `Keyring` to
    /// ask.
    pub fn exists(store: &dyn BlobStore) -> Result<bool, KeyError> {
        Ok(store.contains(&key_blob())?)
    }

    /// Creates a new key in `store`: generates the keypair and seals it under
    /// `passphrase`.
    ///
    /// **Call this only through `Vault::open`.** Writing a fresh key into a
    /// store whose entries were sealed under a different one orphans every one
    /// of them, and the check that prevents it needs to know what an entry blob
    /// is named — which this crate deliberately does not. `Vault::open` owns
    /// that decision; this constructor is public only because it lives one
    /// crate down from the code allowed to make it (ADR 0009).
    pub fn create(store: &dyn BlobStore, passphrase: &SecretString) -> Result<Self, KeyError> {
        let key = Identity::generate();
        // Sealing is ~1s of scrypt, and sync writes this store from its own
        // thread, so "there was no key a second ago" is not "there is no key".
        // `put_new` is what makes the check and the write one step.
        let sealed = key.seal(passphrase)?;
        store.put_new(&key_blob(), &sealed).map_err(|e| match e {
            StoreError::AlreadyExists(_) => KeyError::KeyArrivedWhileCreating,
            other => other.into(),
        })?;
        Ok(Self { key })
    }

    /// Opens the existing key in `store` by unsealing its blob.
    ///
    /// See [`Self::create`] on why this is not the entry point to use.
    pub fn unlock(store: &dyn BlobStore, passphrase: &SecretString) -> Result<Self, KeyError> {
        let sealed = store.get(&key_blob())?.ok_or(KeyError::NoVault)?;
        Ok(Self {
            key: Identity::open(&sealed, passphrase)?,
        })
    }
}

impl Cipher for Keyring {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, KeyError> {
        self.key.encrypt(plaintext)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, KeyError> {
        self.key.decrypt(ciphertext)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::testing::{passphrase, temp_store};
    use journal_fs_store::FsStore;
    use journal_store::BlobName;
    use std::fs;

    /// A keyring with a fresh key and no store behind it, for the tests that
    /// only exercise encryption.
    fn detached() -> Keyring {
        Keyring {
            key: Identity::generate(),
        }
    }

    fn discard(store: FsStore) {
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn create_then_unlock_with_the_correct_passphrase_succeeds() {
        let store = temp_store();
        Keyring::create(&store, &passphrase("correct horse battery staple")).unwrap();
        assert!(Keyring::exists(&store).unwrap());
        Keyring::unlock(&store, &passphrase("correct horse battery staple")).unwrap();
        discard(store);
    }

    #[test]
    fn the_same_key_comes_back_on_every_unlock() {
        let store = temp_store();
        let created = Keyring::create(&store, &passphrase("hunter2")).unwrap();
        let unlocked = Keyring::unlock(&store, &passphrase("hunter2")).unwrap();
        assert_eq!(
            created.key.public(),
            unlocked.key.public(),
            "unlock produced a different key than creation, so entries written \
             in one session would not open in the next"
        );
        discard(store);
    }

    #[test]
    fn the_key_survives_a_round_trip_through_the_store() {
        // The second launch: the key is read back out of the store rather than
        // reused from memory, which is the only way this proves anything about
        // what was written.
        let store = temp_store();
        let ciphertext = Keyring::create(&store, &passphrase("hunter2"))
            .unwrap()
            .encrypt(b"dear diary")
            .unwrap();

        let reopened = Keyring::unlock(&store, &passphrase("hunter2")).unwrap();
        assert_eq!(reopened.decrypt(&ciphertext).unwrap(), b"dear diary");
        discard(store);
    }

    #[test]
    fn create_puts_the_sealed_key_in_the_store() {
        // Not on a path beside it: a store replicated to another device has to
        // carry the key, or the copy over there is unopenable (ADR 0009).
        let store = temp_store();
        Keyring::create(&store, &passphrase("hunter2")).unwrap();
        assert_eq!(store.list().unwrap(), [key_blob()]);
        discard(store);
    }

    /// A store that lets a key blob arrive — as a sync pass would — in the gap
    /// between `create` deciding to write and its write, by putting one in
    /// place on the first `put_new`.
    struct KeyArrives<S>(S);

    impl<S: BlobStore> BlobStore for KeyArrives<S> {
        fn get(&self, name: &BlobName) -> Result<Option<Vec<u8>>, StoreError> {
            self.0.get(name)
        }
        fn put(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
            self.0.put(name, bytes)
        }
        fn put_new(&self, name: &BlobName, bytes: &[u8]) -> Result<(), StoreError> {
            self.0.put(&key_blob(), b"the journal's real key")?;
            self.0.put_new(name, bytes)
        }
        fn delete(&self, name: &BlobName) -> Result<(), StoreError> {
            self.0.delete(name)
        }
        fn list(&self) -> Result<Vec<BlobName>, StoreError> {
            self.0.list()
        }
        fn contains(&self, name: &BlobName) -> Result<bool, StoreError> {
            self.0.contains(name)
        }
    }

    #[test]
    fn a_key_that_arrives_while_creating_is_not_overwritten() {
        let store = KeyArrives(temp_store());

        let err = Keyring::create(&store, &passphrase("hunter2")).unwrap_err();

        assert!(
            matches!(err, KeyError::KeyArrivedWhileCreating),
            "got {err:?}"
        );
        assert_eq!(
            store.get(&key_blob()).unwrap().unwrap(),
            b"the journal's real key",
            "the key that arrived was replaced by the one being created"
        );
        discard(store.0);
    }

    #[test]
    fn unlock_with_the_wrong_passphrase_fails_distinctly() {
        let store = temp_store();
        Keyring::create(&store, &passphrase("correct horse battery staple")).unwrap();
        let err = Keyring::unlock(&store, &passphrase("wrong passphrase")).unwrap_err();
        assert!(matches!(err, KeyError::WrongPassphrase), "got {err:?}");
        discard(store);
    }

    #[test]
    fn a_damaged_key_blob_is_not_reported_as_a_wrong_passphrase() {
        // The whole path, as the user meets it: the blob in the store is
        // unreadable, and `unlock` must say so rather than blame a passphrase
        // that is in fact correct.
        let store = temp_store();
        Keyring::create(&store, &passphrase("hunter2")).unwrap();
        store.put(&key_blob(), b"").unwrap();

        let err = Keyring::unlock(&store, &passphrase("hunter2")).unwrap_err();
        assert!(matches!(err, KeyError::CorruptKeyFile), "got {err:?}");
        discard(store);
    }

    #[test]
    fn no_vault_exists_in_an_untouched_store() {
        assert!(!Keyring::exists(&temp_store()).unwrap());
    }

    #[test]
    fn unlocking_a_store_with_no_key_is_not_a_wrong_passphrase() {
        // Distinct from `WrongPassphrase` on purpose: telling the user their
        // passphrase is wrong when there is nothing to open sends them round a
        // loop no passphrase can exit.
        let err = Keyring::unlock(&temp_store(), &passphrase("hunter2")).unwrap_err();
        assert!(matches!(err, KeyError::NoVault), "got {err:?}");
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let keyring = detached();
        let ciphertext = keyring.encrypt(b"hello vault").unwrap();
        assert_eq!(keyring.decrypt(&ciphertext).unwrap(), b"hello vault");
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let keyring = detached();
        let mut ciphertext = keyring.encrypt(b"hello vault").unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;
        assert!(keyring.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn another_vaults_key_cannot_read_these_entries() {
        let ciphertext = detached().encrypt(b"hello vault").unwrap();
        assert!(detached().decrypt(&ciphertext).is_err());
    }

    #[test]
    fn decrypting_non_age_data_returns_clean_error_not_panic() {
        let err = detached()
            .decrypt(b"not age ciphertext at all")
            .unwrap_err();
        assert!(matches!(err, KeyError::Decrypt(_)), "got {err:?}");
    }

    #[test]
    fn an_entry_written_here_opens_on_a_lower_powered_device() {
        // age benchmarks the writing machine to pick each file's scrypt work
        // factor, and refuses on read anything above the reading machine's own
        // benchmark + 4. A slower device is therefore simulatable exactly: cap
        // what it will accept and watch the desktop-written file bounce.
        let ciphertext = detached().encrypt(b"dear diary").unwrap();

        let mut phone = age::scrypt::Identity::new(passphrase("hunter2"));
        phone.set_max_work_factor(10);

        // Not `is_ok`: entries carry no scrypt stanza at all, so a *passphrase*
        // identity rightly fails to open one. What must never happen is being
        // turned away for the work factor, which the app reports as "wrong
        // passphrase" on a device where the passphrase is correct.
        assert!(
            !matches!(
                age::decrypt(&phone, &ciphertext),
                Err(age::DecryptError::ExcessiveWork { .. })
            ),
            "entry rejected for its work factor on a lower-powered device"
        );
    }

    #[test]
    fn entry_ciphertext_carries_no_scrypt_stanza() {
        // age headers are ASCII, so this reads the property straight off the
        // bytes: no scrypt stanza means no per-file work factor to disagree
        // about, and no per-file key derivation to pay for.
        let ciphertext = detached().encrypt(b"dear diary").unwrap();
        let header = String::from_utf8_lossy(&ciphertext);
        assert!(
            !header.contains("-> scrypt"),
            "entry ciphertext carries an scrypt stanza: {}",
            &header[..header.len().min(120)]
        );
    }

    #[test]
    fn debug_shows_the_public_key_and_never_the_secret() {
        let keyring = detached();
        let shown = format!("{keyring:?}");
        assert!(shown.contains(&keyring.key.public().to_string()));
        assert!(!shown.contains("AGE-SECRET-KEY"));
    }
}
