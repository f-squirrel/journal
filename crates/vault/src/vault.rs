use journal_store::BlobStore;
use secrecy::SecretString;

use crate::identity::{key_blob, Identity};
use crate::{Cipher, VaultError};

/// An unlocked vault: the X25519 keypair every entry in a store is encrypted to.
///
/// Deliberately *only* the key. A vault used to carry the directory its entries
/// lived in, which meant two layers held the same path and two crates knew what
/// an entry file was named; the store below it now owns the first and
/// `journal-storage` above it owns the second (ADR 0009). What is left here is
/// key material and the operations that need it.
///
/// The passphrase does not appear as a field. It protects the sealed key blob
/// and nothing else, so it is needed exactly once — at [`Self::unlock`] or
/// [`Self::create`] — and entry encryption never touches it again.
#[derive(Clone)]
pub struct Vault {
    key: Identity,
}

/// Redacted by hand rather than derived: the whole point of the type is that it
/// holds the key to every entry, and `age`'s `Identity` has no `Debug` of its
/// own precisely so one cannot be printed by accident.
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("key", &self.key.public())
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Whether `store` already holds a vault's key.
    ///
    /// Associated rather than a method: it answers a question about a store that
    /// may hold no vault at all, which is exactly when there is no `Vault` to
    /// ask.
    pub fn exists(store: &dyn BlobStore) -> Result<bool, VaultError> {
        Ok(store.contains(&key_blob())?)
    }

    /// Creates a new vault in `store`: generates the keypair and seals it under
    /// `passphrase`.
    ///
    /// **Call this only through `Storage::open`.** Creating a vault in a store
    /// whose entries were sealed under a different key orphans every one of
    /// them, and the check that prevents it needs to know what an entry blob is
    /// named — which this crate deliberately does not. `Storage::open` owns that
    /// decision; this constructor is public only because it lives one crate
    /// down from the code allowed to make it (ADR 0009).
    pub fn create(store: &dyn BlobStore, passphrase: &SecretString) -> Result<Self, VaultError> {
        let key = Identity::generate();
        store.put(&key_blob(), &key.seal(passphrase)?)?;
        Ok(Self { key })
    }

    /// Opens the existing vault in `store` by unsealing its key blob.
    ///
    /// See [`Self::create`] on why this is not the entry point to use.
    pub fn unlock(store: &dyn BlobStore, passphrase: &SecretString) -> Result<Self, VaultError> {
        let sealed = store.get(&key_blob())?.ok_or(VaultError::NoVault)?;
        Ok(Self {
            key: Identity::open(&sealed, passphrase)?,
        })
    }
}

impl Cipher for Vault {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        self.key.encrypt(plaintext)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
        self.key.decrypt(ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{passphrase, temp_store};
    use journal_fs_store::FsStore;
    use std::fs;

    /// A vault with a fresh key and no store behind it, for the tests that only
    /// exercise encryption.
    fn detached() -> Vault {
        Vault {
            key: Identity::generate(),
        }
    }

    fn discard(store: FsStore) {
        fs::remove_dir_all(store.dir()).ok();
    }

    #[test]
    fn create_then_unlock_with_the_correct_passphrase_succeeds() {
        let store = temp_store();
        Vault::create(&store, &passphrase("correct horse battery staple")).unwrap();
        assert!(Vault::exists(&store).unwrap());
        Vault::unlock(&store, &passphrase("correct horse battery staple")).unwrap();
        discard(store);
    }

    #[test]
    fn the_same_key_comes_back_on_every_unlock() {
        let store = temp_store();
        let created = Vault::create(&store, &passphrase("hunter2")).unwrap();
        let unlocked = Vault::unlock(&store, &passphrase("hunter2")).unwrap();
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
        let ciphertext = Vault::create(&store, &passphrase("hunter2"))
            .unwrap()
            .encrypt(b"dear diary")
            .unwrap();

        let reopened = Vault::unlock(&store, &passphrase("hunter2")).unwrap();
        assert_eq!(reopened.decrypt(&ciphertext).unwrap(), b"dear diary");
        discard(store);
    }

    #[test]
    fn create_puts_the_sealed_key_in_the_store() {
        // Not on a path beside it: a store replicated to another device has to
        // carry the key, or the copy over there is unopenable (ADR 0009).
        let store = temp_store();
        Vault::create(&store, &passphrase("hunter2")).unwrap();
        assert_eq!(store.list().unwrap(), [key_blob()]);
        discard(store);
    }

    #[test]
    fn unlock_with_the_wrong_passphrase_fails_distinctly() {
        let store = temp_store();
        Vault::create(&store, &passphrase("correct horse battery staple")).unwrap();
        let err = Vault::unlock(&store, &passphrase("wrong passphrase")).unwrap_err();
        assert!(matches!(err, VaultError::WrongPassphrase), "got {err:?}");
        discard(store);
    }

    #[test]
    fn no_vault_exists_in_an_untouched_store() {
        assert!(!Vault::exists(&temp_store()).unwrap());
    }

    #[test]
    fn unlocking_a_store_with_no_key_is_not_a_wrong_passphrase() {
        // Distinct from `WrongPassphrase` on purpose: telling the user their
        // passphrase is wrong when there is nothing to open sends them round a
        // loop no passphrase can exit.
        let err = Vault::unlock(&temp_store(), &passphrase("hunter2")).unwrap_err();
        assert!(matches!(err, VaultError::NoVault), "got {err:?}");
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let vault = detached();
        let ciphertext = vault.encrypt(b"hello vault").unwrap();
        assert_eq!(vault.decrypt(&ciphertext).unwrap(), b"hello vault");
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let vault = detached();
        let mut ciphertext = vault.encrypt(b"hello vault").unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;
        assert!(vault.decrypt(&ciphertext).is_err());
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
        assert!(matches!(err, VaultError::Decrypt(_)), "got {err:?}");
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
}
