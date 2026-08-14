//! The seam between the vault's backend-independent core and its pluggable
//! sealing backend.
//!
//! These traits are normative — see §3 of the crypto spec
//! (`docs/superpowers/specs/2026-08-14-crypto-design.md`). A backend that
//! cannot implement them as written is not a valid backend for this vault.
//!
//! M1 ships the `age` backend (spec §5); the native Argon2id + `XChaCha20`
//! backend (spec §6) is the M2 target. Everything above this seam — the
//! directory layout, the header, object naming, the manifest, and the
//! recovery code — is identical under both, which is what keeps the M2
//! migration a slot migration rather than a rewrite.

use std::path::Path;

use secrecy::SecretString;
use zeroize::Zeroizing;

/// Which kind of object a [`Binding`] refers to (spec §4.3).
///
/// The discriminants are on-disk values and are bound into every sealed
/// object; they are part of the format and MUST NOT be renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectKind {
    /// The vault manifest. Object ID is 20 zero bytes.
    Manifest = 1,
    /// A journal entry.
    Entry = 2,
    /// An attachment blob.
    Attachment = 3,
}

/// The length in bytes of an object ID — a truncated object-name tag
/// (spec §4.4).
pub const OBJECT_ID_LEN: usize = 20;

/// Everything that identifies one object.
///
/// Every backend MUST bind every sealed object to this value, by whatever
/// mechanism it has: the native backend uses AEAD associated data, the `age`
/// backend a preamble inside the plaintext (spec §5.4). The mechanism differs;
/// the guarantee does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub format_major: u16,
    pub format_minor: u16,
    pub vault_id: [u8; 16],
    pub kind: ObjectKind,
    pub object_id: [u8; OBJECT_ID_LEN],
}

/// The HMAC key used to derive object names (spec §4.4).
///
/// Deliberately opaque: it is key material, so it implements neither
/// `Debug` nor `Serialize`, and it is zeroized on drop.
pub struct NameKey(Zeroizing<[u8; 32]>);

impl NameKey {
    #[must_use]
    pub fn new(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }

    /// Exposes the raw key for use as an HMAC key. Callers MUST NOT copy it
    /// into a buffer that is not itself zeroized.
    #[must_use]
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A parsed recovery code: the decoded 20 bytes, never the display string
/// (spec §4.5).
pub struct RecoveryCode(Zeroizing<[u8; 20]>);

impl RecoveryCode {
    #[must_use]
    pub fn new(bytes: Zeroizing<[u8; 20]>) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn expose(&self) -> &[u8; 20] {
        &self.0
    }
}

/// Which secret an unlock attempt is offering.
///
/// The backend MUST try only the slot matching the variant: a passphrase is
/// never run against the recovery slot (spec §5.2).
pub enum UnlockSecret {
    Passphrase(SecretString),
    Recovery(RecoveryCode),
}

/// Errors that cross the seam.
///
/// Unlock failures collapse into a single [`CryptoError::Unlock`] variant on
/// purpose: the error MUST NOT reveal which slot failed, how many slots
/// exist, or how far the attempt got (spec §6.2).
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// The vault did not unlock. Deliberately carries no detail.
    #[error("the vault did not unlock")]
    Unlock,

    /// A sealed object did not match the [`Binding`] it was opened against —
    /// a substituted, rolled-back, or transplanted object.
    #[error("sealed object does not match the requested binding")]
    BindingMismatch,

    /// The vault header or a sealed object is structurally invalid.
    #[error("malformed vault data: {0}")]
    Malformed(&'static str),

    /// The vault was written by a backend or format version this client does
    /// not implement. It MUST NOT be opened or written to.
    #[error("unsupported vault format or backend")]
    Unsupported,

    #[error("vault I/O failed")]
    Io(#[from] std::io::Error),
}

/// Seals and opens individual vault objects.
pub trait ObjectSealer {
    /// Seals `plaintext` as the object described by `binding`.
    ///
    /// # Errors
    /// Returns [`CryptoError`] if the backend fails to seal the object.
    fn seal(&self, binding: &Binding, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>;

    /// Opens `sealed`, which MUST have been sealed under exactly `binding`.
    ///
    /// Implementations MUST verify the binding and MUST return
    /// [`CryptoError::BindingMismatch`] — never plaintext — when it does not
    /// match.
    ///
    /// # Errors
    /// Returns [`CryptoError::BindingMismatch`] if the sealed object belongs
    /// to a different object, vault, or format version, and other
    /// [`CryptoError`] variants if it is malformed or cannot be decrypted.
    fn open(&self, binding: &Binding, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError>;
}

/// Holds the unlocked vault secret and manages the on-disk key slots.
pub trait KeyStore: Sized {
    type Sealer: ObjectSealer;

    /// Creates a new vault's key material under `root`.
    ///
    /// Returns the recovery code, which the caller MUST show exactly once and
    /// MUST NOT persist.
    ///
    /// # Errors
    /// Returns [`CryptoError`] if the key material cannot be generated or the
    /// slots cannot be written.
    fn create(root: &Path, passphrase: &SecretString) -> Result<(Self, RecoveryCode), CryptoError>;

    /// Unlocks an existing vault with a passphrase or a recovery code.
    ///
    /// # Errors
    /// Returns [`CryptoError::Unlock`] if no matching slot opens — with no
    /// detail about which slot was tried or why it failed.
    fn unlock(root: &Path, secret: &UnlockSecret) -> Result<Self, CryptoError>;

    /// Rewraps the passphrase slot.
    ///
    /// MUST NOT touch any object and MUST NOT invalidate the recovery slot:
    /// after this call every object file is byte-identical and the recovery
    /// code still works.
    ///
    /// # Errors
    /// Returns [`CryptoError`] if the new slot cannot be derived or written.
    fn change_passphrase(&self, root: &Path, new: &SecretString) -> Result<(), CryptoError>;

    /// The sealer bound to this unlocked vault secret.
    fn sealer(&self) -> &Self::Sealer;

    /// The HMAC key for object naming (spec §4.4).
    ///
    /// Naming lives *above* the seam, but the key is derived from the vault
    /// secret, which lives below it — so the backend supplies the key and the
    /// core owns the algorithm.
    fn name_key(&self) -> &NameKey;
}
