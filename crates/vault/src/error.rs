use thiserror::Error;

use journal_store::StoreError;

/// Every way opening or using a vault can fail.
///
/// The `#[error]` strings are user-facing: `src-tauri` renders commands'
/// errors with `to_string()` and the frontend shows them verbatim, so a
/// variant added here is copy, not just a discriminant.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
    #[error("failed to encrypt vault data: {0}")]
    Encrypt(#[from] age::EncryptError),
    #[error("failed to decrypt vault data: {0}")]
    Decrypt(#[from] age::DecryptError),
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error(
        "this vault's key file is damaged. Restore it from a backup — the \
         entries here cannot be read without it."
    )]
    CorruptKeyFile,
    #[error(
        "this folder holds journal entries but its key file is missing. \
         Restore the vault's key file from a backup — creating a new vault \
         here would leave the existing entries unreadable."
    )]
    MissingKeyFile,
    /// Asked to unlock a vault that was never created. Reachable only by
    /// calling [`Vault::unlock`](crate::Vault::unlock) without checking
    /// [`Vault::exists`](crate::Vault::exists) first, which
    /// `Storage::open` does — so this is a wiring bug surfacing as an error
    /// rather than as a fresh vault written over nothing.
    #[error("there is no vault here yet")]
    NoVault,
}

impl VaultError {
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
