//! Encryption at rest for journal entries.
//!
//! A [`Vault`] is an X25519 keypair that entry blobs are encrypted to, and
//! nothing else — it owns no directory and knows nothing about what an entry
//! looks like (ADR 0009). The passphrase protects only the sealed key blob the
//! keypair is stored in — one scrypt operation, at [`Vault::unlock`] — so entry
//! encryption is sub-millisecond and carries no per-file work factor for
//! another device to disagree about (ADR 0007).

mod cipher;
mod error;
mod identity;
mod vault;

#[cfg(test)]
mod testing;

pub use cipher::Cipher;
pub use error::VaultError;
pub use vault::Vault;
