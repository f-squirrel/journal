//! The key journal entries are encrypted to.
//!
//! A [`Keyring`] is an X25519 keypair and the sealed blob it lives in, and
//! nothing else — it owns no directory and knows nothing about what an entry
//! looks like (ADR 0009). The vault those entries make up is
//! [`journal-vault`](../journal_vault/index.html), one crate up. The passphrase
//! protects only the sealed key blob — one scrypt operation, at
//! [`Keyring::unlock`] — so entry encryption is sub-millisecond and carries no
//! per-file work factor for another device to disagree about (ADR 0007).

#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

mod cipher;
mod error;
mod identity;
mod keyring;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod testing;

pub use cipher::Cipher;
pub use error::KeyError;
pub use keyring::Keyring;
