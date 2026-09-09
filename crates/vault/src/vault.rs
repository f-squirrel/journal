use std::fs;
use std::path::{Path, PathBuf};

use secrecy::SecretString;

use crate::identity::{key_path, Identity};
use crate::VaultError;

/// An unlocked vault: a directory, and the X25519 keypair every entry in it is
/// encrypted to.
///
/// The directory is part of the vault rather than a parameter its callers carry
/// alongside it, so there is one answer to "which directory did this key come
/// from?". [`Storage`](../journal_storage/struct.Storage.html) takes a vault
/// and asks it, instead of being handed a path that could name somewhere else.
///
/// The passphrase does not appear here. It protects the key file and nothing
/// else, so it is needed exactly once, at [`Vault::open_or_create`], and entry
/// encryption never touches it again.
#[derive(Clone)]
pub struct Vault {
    dir: PathBuf,
    key: Identity,
}

/// Redacted by hand rather than derived: the whole point of the type is that it
/// holds the key to every entry, and `age`'s `Identity` has no `Debug` of its
/// own precisely so one cannot be printed by accident.
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("dir", &self.dir)
            .field("key", &self.key.public())
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Whether a vault has already been created in `dir`.
    ///
    /// Associated rather than a method: it answers a question about a directory
    /// that may hold no vault at all, which is exactly when there is no `Vault`
    /// to ask.
    pub fn exists(dir: &Path) -> bool {
        key_path(dir).is_file()
    }

    /// The directory this vault was opened in, and where its entries live.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Opens the vault in `dir` with `passphrase`, creating it if `dir` holds
    /// no vault at all.
    ///
    /// The only way in, deliberately: [`Self::create`] and [`Self::unlock`]
    /// stay private so the create-or-open decision cannot be made anywhere but
    /// here. Getting it wrong means writing a fresh key file over a directory
    /// whose entries were sealed under a different key, orphaning every one of
    /// them. A missing key file next to surviving entry files is that case, and
    /// is refused with [`VaultError::MissingKeyFile`] rather than treated as a
    /// first run.
    pub fn open_or_create(dir: &Path, passphrase: &SecretString) -> Result<Self, VaultError> {
        if Self::exists(dir) {
            return Self::unlock(dir, passphrase);
        }
        if holds_entry_files(dir)? {
            return Err(VaultError::MissingKeyFile);
        }
        Self::create(dir, passphrase)
    }

    /// Creates a new vault in `dir`: generates the keypair and seals it under
    /// `passphrase`.
    fn create(dir: &Path, passphrase: &SecretString) -> Result<Self, VaultError> {
        let key = Identity::generate();
        key.seal_to(&key_path(dir), passphrase)?;
        Ok(Self::new(dir, key))
    }

    /// Opens the existing vault in `dir` by unsealing its key file.
    fn unlock(dir: &Path, passphrase: &SecretString) -> Result<Self, VaultError> {
        let key = Identity::open(&key_path(dir), passphrase)?;
        Ok(Self::new(dir, key))
    }

    fn new(dir: &Path, key: Identity) -> Self {
        Self {
            dir: dir.to_path_buf(),
            key,
        }
    }

    /// Encrypts `plaintext` to the vault's key.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        self.key.encrypt(plaintext)
    }

    /// Decrypts `ciphertext` with the vault's key.
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
        self.key.decrypt(ciphertext)
    }
}

/// Whether `dir` holds at least one entry file.
///
/// The `is_file` test is load-bearing: a *directory* named `<uuid>.json` is
/// reachable, and counting one would permanently refuse to create a vault,
/// pointing the user at a backup that does not exist.
fn holds_entry_files(dir: &Path) -> Result<bool, VaultError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    for file in entries {
        let path = file?.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{passphrase, temp_dir};

    #[test]
    fn create_then_unlock_with_correct_passphrase_succeeds() {
        let dir = temp_dir();
        Vault::create(&dir, &passphrase("correct horse battery staple")).unwrap();
        assert!(Vault::exists(&dir));
        Vault::unlock(&dir, &passphrase("correct horse battery staple")).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_same_key_comes_back_on_every_unlock() {
        let dir = temp_dir();
        let created = Vault::create(&dir, &passphrase("hunter2")).unwrap();
        let unlocked = Vault::unlock(&dir, &passphrase("hunter2")).unwrap();
        assert_eq!(
            created.key.public(),
            unlocked.key.public(),
            "unlock produced a different key than creation, so entries written \
             in one session would not open in the next"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unlock_with_wrong_passphrase_fails_distinctly() {
        let dir = temp_dir();
        Vault::create(&dir, &passphrase("correct horse battery staple")).unwrap();
        let err = Vault::unlock(&dir, &passphrase("wrong passphrase")).unwrap_err();
        assert!(matches!(err, VaultError::WrongPassphrase));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_vault_exists_before_creation() {
        let dir = temp_dir();
        assert!(!Vault::exists(&dir));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let vault = Vault::new(&temp_dir(), Identity::generate());
        let ciphertext = vault.encrypt(b"hello vault").unwrap();
        assert_eq!(vault.decrypt(&ciphertext).unwrap(), b"hello vault");
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let vault = Vault::new(&temp_dir(), Identity::generate());
        let mut ciphertext = vault.encrypt(b"hello vault").unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;
        assert!(vault.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn another_vaults_key_cannot_read_these_entries() {
        let ciphertext = Vault::new(&temp_dir(), Identity::generate())
            .encrypt(b"hello vault")
            .unwrap();
        let stranger = Vault::new(&temp_dir(), Identity::generate());
        assert!(stranger.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn decrypting_non_age_data_returns_clean_error_not_panic() {
        let vault = Vault::new(&temp_dir(), Identity::generate());
        let err = vault.decrypt(b"not age ciphertext at all").unwrap_err();
        assert!(matches!(err, VaultError::Decrypt(_)));
    }

    #[test]
    fn open_or_create_refuses_when_the_key_file_is_missing_but_entries_remain() {
        let dir = temp_dir();
        let vault = Vault::create(&dir, &passphrase("right")).unwrap();
        let entry = vault.encrypt(b"{}").unwrap();
        fs::write(
            dir.join("5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json"),
            &entry,
        )
        .unwrap();
        fs::remove_file(key_path(&dir)).unwrap();

        let err = Vault::open_or_create(&dir, &passphrase("typo")).unwrap_err();
        assert!(matches!(err, VaultError::MissingKeyFile));
        // The assertion that matters: an error that still wrote a key file
        // would have orphaned the entry anyway.
        assert!(!Vault::exists(&dir));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_named_like_an_entry_does_not_block_creation() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json")).unwrap();

        Vault::open_or_create(&dir, &passphrase("fresh start")).unwrap();
        assert!(Vault::exists(&dir));

        fs::remove_dir_all(&dir).ok();
    }

    /// The ciphertext the vault's entry path actually writes. When the entry
    /// format changes this helper changes with it; the assertions built on it
    /// below do not.
    fn entry_ciphertext(pass: &SecretString, plaintext: &[u8]) -> Vec<u8> {
        let dir = temp_dir();
        let ciphertext = Vault::open_or_create(&dir, pass)
            .unwrap()
            .encrypt(plaintext)
            .unwrap();
        fs::remove_dir_all(&dir).ok();
        ciphertext
    }

    #[test]
    fn an_entry_written_here_opens_on_a_lower_powered_device() {
        // age benchmarks the writing machine to pick each file's scrypt work
        // factor, and refuses on read anything above the reading machine's own
        // benchmark + 4. A slower device is therefore simulatable exactly: cap
        // what it will accept and watch the desktop-written file bounce.
        let ciphertext = entry_ciphertext(&passphrase("hunter2"), b"dear diary");

        let mut phone = age::scrypt::Identity::new(passphrase("hunter2"));
        phone.set_max_work_factor(10);

        // Not `is_ok`: once entries carry no scrypt stanza at all, a *passphrase*
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
    fn entry_files_carry_no_scrypt_stanza() {
        // age headers are ASCII, so this reads the property straight off the
        // bytes: no scrypt stanza means no per-file work factor to disagree
        // about, and no per-file key derivation to pay for.
        let ciphertext = entry_ciphertext(&passphrase("hunter2"), b"dear diary");
        let header = String::from_utf8_lossy(&ciphertext);
        assert!(
            !header.contains("-> scrypt"),
            "entry ciphertext carries an scrypt stanza: {}",
            &header[..header.len().min(120)]
        );
    }

    #[test]
    fn key_file_round_trips_and_rejects_the_wrong_passphrase() {
        let dir = temp_dir();
        let vault = Vault::open_or_create(&dir, &passphrase("hunter2")).unwrap();
        let ciphertext = vault.encrypt(b"dear diary").unwrap();

        // Re-opened rather than reused: this is the second launch, reading the
        // key back off disk. An in-memory vault would prove nothing about the
        // file.
        let reopened = Vault::open_or_create(&dir, &passphrase("hunter2")).unwrap();
        assert_eq!(reopened.decrypt(&ciphertext).unwrap(), b"dear diary");

        let err = Vault::open_or_create(&dir, &passphrase("typo")).unwrap_err();
        assert!(matches!(err, VaultError::WrongPassphrase));

        fs::remove_dir_all(&dir).ok();
    }
}
