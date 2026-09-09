use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretString};

use crate::VaultError;

const KEY_FILE: &str = ".vault-key";

/// The scrypt work factor the key is sealed with, `N = 2^18`.
///
/// Pinned rather than left to `age`, which benchmarks the *writing* machine to
/// land near a second and stamps the result into the file. That number is the
/// one thing a vault must never disagree with itself about across devices: this
/// file holds the only copy of the key, and a reader that refuses the factor
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

/// Where a vault's key is stored inside its directory.
pub(crate) fn key_path(dir: &Path) -> PathBuf {
    dir.join(KEY_FILE)
}

/// The vault's key: an X25519 keypair, and everything about how it is stored.
///
/// Sealing and unsealing are the key's own business rather than a file type's,
/// because the two halves are one format: the passphrase-derived scrypt layer
/// and the `AGE-SECRET-KEY-1…` encoding inside it only ever appear together.
/// Nothing outside this module sees either.
#[derive(Clone)]
pub(crate) struct Identity {
    identity: age::x25519::Identity,
    recipient: age::x25519::Recipient,
}

impl Identity {
    pub(crate) fn generate() -> Self {
        Self::from_age(age::x25519::Identity::generate())
    }

    /// Unseals the key stored at `path`.
    ///
    /// The one scrypt operation in the whole design, and the only place the
    /// passphrase is used at all.
    pub(crate) fn open(path: &Path, passphrase: &SecretString) -> Result<Self, VaultError> {
        let sealed = fs::read(path)?;
        let mut serialized =
            unseal(passphrase, &sealed).map_err(|_| VaultError::WrongPassphrase)?;
        let identity = std::str::from_utf8(&serialized)
            .ok()
            .and_then(|s| age::x25519::Identity::from_str(s).ok());
        serialized.zeroize();
        // A key that unseals but does not parse is damaged, not a typo'd
        // passphrase: the passphrase demonstrably worked to get this far.
        Ok(Self::from_age(identity.ok_or(VaultError::CorruptKeyFile)?))
    }

    /// Seals this key to `path`, creating its directory if it is not there yet.
    pub(crate) fn seal_to(&self, path: &Path, passphrase: &SecretString) -> Result<(), VaultError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let serialized = self.identity.to_string();
        let sealed = seal(passphrase, serialized.expose_secret().as_bytes())?;
        fs::write(path, sealed)?;
        Ok(())
    }

    /// Encrypts `plaintext` to this key. Sub-millisecond: no key derivation,
    /// and nothing in the output for a reader to disagree about.
    pub(crate) fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        Ok(age::encrypt(&self.recipient, plaintext)?)
    }

    pub(crate) fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
        Ok(age::decrypt(&self.identity, ciphertext)?)
    }

    /// The public half, in its `age1…` form. The only part of a key that is
    /// safe to print, which is what [`Vault`](crate::Vault)'s `Debug` shows.
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
fn seal(passphrase: &SecretString, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let mut recipient = age::scrypt::Recipient::new(passphrase.clone());
    recipient.set_work_factor(WORK_FACTOR);
    Ok(age::encrypt(&recipient, plaintext)?)
}

/// Unseals bytes with `passphrase`, refusing to do more work than
/// [`MAX_WORK_FACTOR`].
fn unseal(passphrase: &SecretString, ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let mut identity = age::scrypt::Identity::new(passphrase.clone());
    identity.set_max_work_factor(MAX_WORK_FACTOR);
    Ok(age::decrypt(&identity, ciphertext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{passphrase, temp_dir};

    #[test]
    fn the_key_file_is_portable_across_devices() {
        // The key file is the one file still sealed with scrypt, and under this
        // design it holds the only copy of the vault key — so a work factor a
        // slower device refuses loses the journal outright rather than merely
        // slowing it down. Both sides of that number are therefore constants,
        // not benchmarks of whatever machine is running.
        let dir = temp_dir();
        let path = key_path(&dir);
        Identity::generate()
            .seal_to(&path, &passphrase("hunter2"))
            .unwrap();
        let sealed = fs::read(&path).unwrap();

        let header = String::from_utf8_lossy(&sealed);
        let stanza = header
            .lines()
            .find(|line| line.starts_with("-> scrypt "))
            .expect("key file is scrypt-sealed");
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
            "key file carries a benchmarked work factor, not the pinned one"
        );

        let mut slowest_supported_device = age::scrypt::Identity::new(passphrase("hunter2"));
        slowest_supported_device.set_max_work_factor(MAX_WORK_FACTOR);
        assert!(
            !matches!(
                age::decrypt(&slowest_supported_device, &sealed),
                Err(age::DecryptError::ExcessiveWork { .. })
            ),
            "key file rejected for its work factor on a lower-powered device"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sealing_then_opening_returns_the_same_key() {
        let dir = temp_dir();
        let path = key_path(&dir);
        let key = Identity::generate();
        key.seal_to(&path, &passphrase("hunter2")).unwrap();

        let reopened = Identity::open(&path, &passphrase("hunter2")).unwrap();
        assert_eq!(key.public(), reopened.public());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_key_file_that_opens_but_does_not_parse_is_not_a_wrong_passphrase() {
        // The passphrase provably worked — the file unsealed — so blaming it
        // would send the user round a loop no correct passphrase can exit.
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = key_path(&dir);
        fs::write(
            &path,
            seal(&passphrase("hunter2"), b"not an age secret key").unwrap(),
        )
        .unwrap();

        let Err(err) = Identity::open(&path, &passphrase("hunter2")) else {
            panic!("a sealed value that is not a key must not open a vault");
        };
        assert!(matches!(err, VaultError::CorruptKeyFile), "got {err:?}");

        fs::remove_dir_all(&dir).ok();
    }
}
