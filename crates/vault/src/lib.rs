use std::fs;
use std::path::{Path, PathBuf};

use secrecy::SecretString;
use thiserror::Error;

const CANARY_FILE: &str = ".vault-canary";
const CANARY_MAGIC: &[u8] = b"journal-vault-canary-v1";

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to encrypt vault data: {0}")]
    Encrypt(#[from] age::EncryptError),
    #[error("failed to decrypt vault data: {0}")]
    Decrypt(#[from] age::DecryptError),
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error(
        "this folder holds journal entries but its key file is missing. \
         Restore the vault's key file from a backup — creating a new vault \
         here would leave the existing entries unreadable."
    )]
    MissingKeyFile,
}

fn canary_path(dir: &Path) -> PathBuf {
    dir.join(CANARY_FILE)
}

/// Returns whether a vault has already been created in `dir`.
pub fn has_vault(dir: &Path) -> bool {
    canary_path(dir).is_file()
}

/// Opens the vault in `dir` with `passphrase`, creating it if `dir` holds no
/// vault at all.
///
/// "Create or open?" deliberately lives here rather than in the command layer:
/// getting it wrong means writing a fresh key file over a directory whose
/// entries were sealed under a different passphrase, orphaning every one of
/// them. A missing key file next to surviving entry files is that case, and is
/// refused with [`VaultError::MissingKeyFile`] rather than treated as a first
/// run.
pub fn open_or_create(dir: &Path, passphrase: &SecretString) -> Result<(), VaultError> {
    if has_vault(dir) {
        return unlock_vault(dir, passphrase);
    }
    if holds_entry_files(dir)? {
        return Err(VaultError::MissingKeyFile);
    }
    create_vault(dir, passphrase)
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

/// Creates a new vault in `dir`, sealing a known canary value with `passphrase`.
fn create_vault(dir: &Path, passphrase: &SecretString) -> Result<(), VaultError> {
    fs::create_dir_all(dir)?;
    let ciphertext = encrypt(passphrase, CANARY_MAGIC)?;
    fs::write(canary_path(dir), ciphertext)?;
    Ok(())
}

/// Verifies `passphrase` against the vault's canary file in `dir`.
fn unlock_vault(dir: &Path, passphrase: &SecretString) -> Result<(), VaultError> {
    let ciphertext = fs::read(canary_path(dir))?;
    let plaintext = decrypt(passphrase, &ciphertext).map_err(|_| VaultError::WrongPassphrase)?;
    if plaintext != CANARY_MAGIC {
        return Err(VaultError::WrongPassphrase);
    }
    Ok(())
}

/// Encrypts `plaintext` with `passphrase` via `age`'s scrypt recipient.
///
/// This applies age's scrypt work factor (~1s by design) on every call — not
/// something to call in a tight loop without expecting it. See the
/// `age-vault-encryption` plan for why this per-call cost is a deliberately
/// accepted tradeoff for now.
pub fn encrypt(passphrase: &SecretString, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let recipient = age::scrypt::Recipient::new(passphrase.clone());
    Ok(age::encrypt(&recipient, plaintext)?)
}

/// Decrypts `ciphertext` with `passphrase` via `age`'s scrypt identity.
///
/// Same ~1s-by-design scrypt cost per call as [`encrypt`].
pub fn decrypt(passphrase: &SecretString, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let identity = age::scrypt::Identity::new(passphrase.clone());
    Ok(age::decrypt(&identity, ciphertext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("journal-vault-test-{}-{n}", std::process::id()))
    }

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn create_then_unlock_with_correct_passphrase_succeeds() {
        let dir = temp_dir();
        create_vault(&dir, &passphrase("correct horse battery staple")).unwrap();
        assert!(has_vault(&dir));
        unlock_vault(&dir, &passphrase("correct horse battery staple")).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unlock_with_wrong_passphrase_fails_distinctly() {
        let dir = temp_dir();
        create_vault(&dir, &passphrase("correct horse battery staple")).unwrap();
        let err = unlock_vault(&dir, &passphrase("wrong passphrase")).unwrap_err();
        assert!(matches!(err, VaultError::WrongPassphrase));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn has_vault_is_false_before_creation() {
        let dir = temp_dir();
        assert!(!has_vault(&dir));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let ciphertext = encrypt(&passphrase("hunter2"), b"hello vault").unwrap();
        let plaintext = decrypt(&passphrase("hunter2"), &ciphertext).unwrap();
        assert_eq!(plaintext, b"hello vault");
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let mut ciphertext = encrypt(&passphrase("hunter2"), b"hello vault").unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;
        assert!(decrypt(&passphrase("hunter2"), &ciphertext).is_err());
    }

    #[test]
    fn decrypting_non_age_data_returns_clean_error_not_panic() {
        let err = decrypt(&passphrase("hunter2"), b"not age ciphertext at all").unwrap_err();
        assert!(matches!(err, VaultError::Decrypt(_)));
    }

    #[test]
    fn open_or_create_refuses_when_the_key_file_is_missing_but_entries_remain() {
        let dir = temp_dir();
        create_vault(&dir, &passphrase("right")).unwrap();
        let entry = encrypt(&passphrase("right"), b"{}").unwrap();
        fs::write(
            dir.join("5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json"),
            &entry,
        )
        .unwrap();
        fs::remove_file(canary_path(&dir)).unwrap();

        let err = open_or_create(&dir, &passphrase("typo")).unwrap_err();
        assert!(matches!(err, VaultError::MissingKeyFile));
        // The assertion that matters: an error that still wrote a key file
        // would have orphaned the entry anyway.
        assert!(!has_vault(&dir));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_named_like_an_entry_does_not_block_creation() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json")).unwrap();

        open_or_create(&dir, &passphrase("fresh start")).unwrap();
        assert!(has_vault(&dir));

        fs::remove_dir_all(&dir).ok();
    }
}
