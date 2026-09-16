use std::str::FromStr;

use journal_store::BlobName;
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretString};

use crate::KeyError;

/// The name of the blob a vault's sealed key lives in.
///
/// A blob in the entry store rather than a path on this machine, so that a
/// store replicated to another device carries the key with it — see ADR 0009.
/// Without it the other device holds ciphertext that no passphrase can open.
pub(crate) fn key_blob() -> BlobName {
    BlobName::literal(".vault-key")
}

/// The scrypt work factor the key is sealed with, `N = 2^18`.
///
/// Pinned rather than left to `age`, which benchmarks the *writing* machine to
/// land near a second and stamps the result into the file. That number is the
/// one thing a vault must never disagree with itself about across devices: this
/// blob holds the only copy of the key, and a reader that refuses the factor
/// cannot fall back on the passphrase. 18 is what the Go implementation of age
/// writes unconditionally, and measures ~0.4s on a desktop and a couple of
/// seconds on a phone — paid once per unlock.
const WORK_FACTOR: u8 = 18;

/// The largest work factor we will pay to open a sealed key.
///
/// Set explicitly instead of inheriting `age`'s "whatever this machine
/// benchmarks, plus 4", which on a fast desktop reaches 22 — where `age`'s own
/// documentation warns a malicious file can cost hours and tens of GiB. Room
/// for two future bumps of [`WORK_FACTOR`], and nothing more.
const MAX_WORK_FACTOR: u8 = 20;

/// Raising the write side past the read side would seal a key this very build
/// then refuses to open. A compile error rather than a test, because by the
/// time a test could observe it the vault is already unopenable.
const _: () = assert!(WORK_FACTOR <= MAX_WORK_FACTOR);

/// The vault's key: an X25519 keypair, and everything about how it is sealed.
///
/// Sealing and unsealing are the key's own business rather than a file type's,
/// because the two halves are one format: the passphrase-derived scrypt layer
/// and the `AGE-SECRET-KEY-1…` encoding inside it only ever appear together.
/// Nothing outside this module sees either.
///
/// Bytes in, bytes out: where the sealed key is *kept* is a
/// [`BlobStore`](journal_store::BlobStore)'s business, not this type's.
#[derive(Clone)]
pub(crate) struct Identity {
    identity: age::x25519::Identity,
    recipient: age::x25519::Recipient,
}

impl Identity {
    pub(crate) fn generate() -> Self {
        Self::from_age(age::x25519::Identity::generate())
    }

    /// Unseals a key from the bytes [`Self::seal`] produced.
    ///
    /// The one scrypt operation in the whole design, and the only place the
    /// passphrase is used at all.
    pub(crate) fn open(sealed: &[u8], passphrase: &SecretString) -> Result<Self, KeyError> {
        let mut serialized =
            decrypt_with_passphrase(passphrase, sealed).map_err(|_| KeyError::WrongPassphrase)?;
        let identity = std::str::from_utf8(&serialized)
            .ok()
            .and_then(|s| age::x25519::Identity::from_str(s).ok());
        serialized.zeroize();
        // A key that unseals but does not parse is damaged, not a typo'd
        // passphrase: the passphrase demonstrably worked to get this far.
        Ok(Self::from_age(identity.ok_or(KeyError::CorruptKeyFile)?))
    }

    /// Seals this key under `passphrase`, ready to be stored.
    pub(crate) fn seal(&self, passphrase: &SecretString) -> Result<Vec<u8>, KeyError> {
        let serialized = self.identity.to_string();
        encrypt_with_passphrase(passphrase, serialized.expose_secret().as_bytes())
    }

    /// Encrypts `plaintext` to this key. Sub-millisecond: no key derivation,
    /// and nothing in the output for a reader to disagree about.
    pub(crate) fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, KeyError> {
        Ok(age::encrypt(&self.recipient, plaintext)?)
    }

    pub(crate) fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, KeyError> {
        Ok(age::decrypt(&self.identity, ciphertext)?)
    }

    /// The public half, in its `age1…` form. The only part of a key that is
    /// safe to print, which is what [`Keyring`](crate::Keyring)'s `Debug` shows.
    pub(crate) fn public(&self) -> String {
        self.recipient.to_string()
    }

    fn from_age(identity: age::x25519::Identity) -> Self {
        let recipient = identity.to_public();
        Self {
            identity,
            recipient,
        }
    }
}

/// Seals bytes under `passphrase` with `age`'s scrypt recipient, at the pinned
/// [`WORK_FACTOR`].
fn encrypt_with_passphrase(
    passphrase: &SecretString,
    plaintext: &[u8],
) -> Result<Vec<u8>, KeyError> {
    let mut recipient = age::scrypt::Recipient::new(passphrase.clone());
    recipient.set_work_factor(WORK_FACTOR);
    Ok(age::encrypt(&recipient, plaintext)?)
}

/// Unseals bytes with `passphrase`, refusing to do more work than
/// [`MAX_WORK_FACTOR`].
fn decrypt_with_passphrase(
    passphrase: &SecretString,
    ciphertext: &[u8],
) -> Result<Vec<u8>, KeyError> {
    let mut identity = age::scrypt::Identity::new(passphrase.clone());
    identity.set_max_work_factor(MAX_WORK_FACTOR);
    Ok(age::decrypt(&identity, ciphertext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::passphrase;

    #[test]
    fn the_sealed_key_is_portable_across_devices() {
        // The key is the one thing still sealed with scrypt, and under this
        // design it is the only copy — so a work factor a slower device refuses
        // loses the journal outright rather than merely slowing it down. Both
        // sides of that number are therefore constants, not benchmarks of
        // whatever machine is running.
        let sealed = Identity::generate().seal(&passphrase("hunter2")).unwrap();

        let header = String::from_utf8_lossy(&sealed);
        let stanza = header
            .lines()
            .find(|line| line.starts_with("-> scrypt "))
            .expect("the key is scrypt-sealed");
        let written: u8 = stanza
            .split_whitespace()
            .next_back()
            .and_then(|arg| arg.parse().ok())
            .expect("scrypt stanza ends in a work factor");
        // A machine whose own benchmark happens to land on the pinned value
        // cannot tell these apart; the constant relationship asserted at compile
        // time above, and the explicit `set_work_factor` call, are what carry
        // the property.
        assert_eq!(
            written, WORK_FACTOR,
            "sealed key carries a benchmarked work factor, not the pinned one"
        );

        let mut slowest_supported_device = age::scrypt::Identity::new(passphrase("hunter2"));
        slowest_supported_device.set_max_work_factor(MAX_WORK_FACTOR);
        assert!(
            !matches!(
                age::decrypt(&slowest_supported_device, &sealed),
                Err(age::DecryptError::ExcessiveWork { .. })
            ),
            "sealed key rejected for its work factor on a lower-powered device"
        );
    }

    #[test]
    fn sealing_then_opening_returns_the_same_key() {
        let key = Identity::generate();
        let sealed = key.seal(&passphrase("hunter2")).unwrap();

        let reopened = Identity::open(&sealed, &passphrase("hunter2")).unwrap();
        assert_eq!(key.public(), reopened.public());
    }

    #[test]
    fn the_wrong_passphrase_does_not_open_a_sealed_key() {
        let sealed = Identity::generate().seal(&passphrase("hunter2")).unwrap();
        let Err(err) = Identity::open(&sealed, &passphrase("typo")) else {
            panic!("the wrong passphrase unsealed a key");
        };
        assert!(matches!(err, KeyError::WrongPassphrase), "got {err:?}");
    }

    #[test]
    fn a_key_that_opens_but_does_not_parse_is_not_a_wrong_passphrase() {
        // The passphrase provably worked — the blob unsealed — so blaming it
        // would send the user round a loop no correct passphrase can exit.
        let sealed =
            encrypt_with_passphrase(&passphrase("hunter2"), b"not an age secret key").unwrap();

        let Err(err) = Identity::open(&sealed, &passphrase("hunter2")) else {
            panic!("a sealed value that is not a key must not open a vault");
        };
        assert!(matches!(err, KeyError::CorruptKeyFile), "got {err:?}");
    }
}
