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
}

fn canary_path(dir: &Path) -> PathBuf {
    dir.join(CANARY_FILE)
}

/// Returns whether a vault has already been created in `dir`.
pub fn has_vault(dir: &Path) -> bool {
    canary_path(dir).is_file()
}

/// Creates a new vault in `dir`, sealing a known canary value with `passphrase`.
pub fn create_vault(dir: &Path, passphrase: &SecretString) -> Result<(), VaultError> {
    fs::create_dir_all(dir)?;
    let ciphertext = encrypt(passphrase, CANARY_MAGIC)?;
    fs::write(canary_path(dir), ciphertext)?;
    Ok(())
}

/// Verifies `passphrase` against the vault's canary file in `dir`.
pub fn unlock_vault(dir: &Path, passphrase: &SecretString) -> Result<(), VaultError> {
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
}
