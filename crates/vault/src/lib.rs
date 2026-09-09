//! Encryption at rest for journal entries.
//!
//! One [`Vault`] owns a directory: an X25519 keypair that every entry file in
//! it is encrypted to. The passphrase protects only the key file that keypair
//! is stored in — one scrypt operation, at [`Vault::open_or_create`] — so entry
//! encryption is sub-millisecond and carries no per-file work factor for
//! another device to disagree about. See ADR 0007.

mod error;
mod identity;
mod vault;

#[cfg(test)]
mod testing;

pub use error::VaultError;
pub use vault::Vault;
