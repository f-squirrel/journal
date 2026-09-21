use thiserror::Error;

use journal_store::StoreError;

/// Every way opening or using the vault's key can fail.
///
/// The `#[error]` strings are user-facing: `src-tauri` renders commands'
/// errors with `to_string()` and the frontend shows them verbatim, so a
/// variant added here is copy, not just a discriminant.
#[derive(Debug, Error)]
pub enum KeyError {
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("failed to encrypt vault data: {0}")]
    Encrypt(#[from] age::EncryptError),
    #[error("failed to decrypt vault data: {0}")]
    Decrypt(#[from] age::DecryptError),
    #[error("Incorrect passphrase.")]
    WrongPassphrase,
    #[error(
        "This vault's key file is damaged. Restore it from a backup — the \
         entries here cannot be read without it."
    )]
    CorruptKeyFile,
    #[error(
        "This folder holds journal entries but its key file is missing. \
         Restore the vault's key file from a backup — creating a new vault \
         here would leave the existing entries unreadable."
    )]
    MissingKeyFile,
    /// Asked to unlock a vault that was never created. Reachable only by
    /// calling [`Keyring::unlock`](crate::Keyring::unlock) without checking
    /// [`Keyring::exists`](crate::Keyring::exists) first, which `Vault::open`
    /// does — so this is a wiring bug surfacing as an error rather than as a
    /// fresh key written over nothing.
    #[error("There is no vault here yet.")]
    NoVault,
    /// A key blob appeared in the store while [`Keyring::create`](crate::Keyring::create)
    /// was sealing its own — sync pulled the journal's real key in from
    /// another device. Nothing is missing and nothing is damaged, so neither
    /// of the copy above fits. The key that arrived is sealed under the
    /// passphrase from the device that made it, not the one just chosen, so
    /// the copy says so: "try again" would end in "Incorrect passphrase."
    #[error(
        "This journal already exists — it arrived from the cloud while yours \
         was being created. Sign in with the passphrase you chose when you \
         first created it, not the new one."
    )]
    KeyArrivedWhileCreating,
}

impl KeyError {
    /// The error a [`Cipher`](crate::Cipher) reports for bytes it did not
    /// produce.
    ///
    /// Exists so an implementation can honor the trait's "must fail for
    /// foreign bytes" contract without taking a dependency on `age` to name
    /// the variant — the point of the trait being that the layer above does
    /// not know which cipher it has.
    pub fn not_ciphertext() -> Self {
        Self::Decrypt(age::DecryptError::InvalidHeader)
    }
}
