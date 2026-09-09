use thiserror::Error;

/// Every way opening or using a vault can fail.
///
/// The `#[error]` strings are user-facing: `src-tauri` renders commands'
/// errors with `to_string()` and the frontend shows them verbatim, so a
/// variant added here is copy, not just a discriminant.
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
}
