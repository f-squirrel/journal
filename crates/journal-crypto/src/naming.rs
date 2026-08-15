//! Object naming: the keyed name tag of crypto spec §4.4 and the two
//! per-backend `K_name` derivations that feed it.
//!
//! Naming lives **above** the backend seam — the algorithm here is the same
//! under `age` and under the native backend — but the key it consumes comes
//! from below it, so [`crate::backend::KeyStore::name_key`] supplies the key
//! and this module owns the construction:
//!
//! ```text
//! h        = SHA-256(content)
//! tag      = HMAC-SHA-256(K_name, b"JRNLNAM1" || kind_u8 || h)
//! name_tag = tag[0..20]
//! name     = crockford_base32(name_tag), lowercased
//! ```
//!
//! `content` is the object's own bytes, never the age backend's
//! `preamble || content` of spec §5.5: the preamble carries `object_id`,
//! which *is* the name tag, so hashing it would be circular.
//!
//! **Why keyed** (spec §4.4, [ADR-0013]): an object stored under the bare
//! hash of its plaintext is a confirmation oracle — anyone holding the vault
//! directory could hash a file they suspect the user has and look for that
//! name, no decryption required. Keying with `K_name` keeps deduplication
//! inside one vault while making the name meaningless without the vault
//! secret.
//!
//! [ADR-0013]: ../../../docs/adrs/0013-keyed-object-names.md

use std::fmt;

use base32::Alphabet;
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::backend::{Dek, NameKey, ObjectKind, OBJECT_ID_LEN};

/// Domain separator for the naming HMAC (spec §4.4).
const NAME_DOMAIN: &[u8; 8] = b"JRNLNAM1";

/// Domain separator for every key derived from a vault secret (spec §4.4,
/// §6.3).
const KDF_DOMAIN: &[u8; 8] = b"JRNLKDF1";

/// `purpose = 2` — `K_name` (spec §6.3's table, shared by §4.4's age row).
const PURPOSE_NAME: u8 = 2;

/// A name tag is 160 bits, and Crockford Base32 packs 5 bits per character:
/// 32 characters exactly, with no padding and no leftover bits.
pub const OBJECT_NAME_CHARS: usize = 32;

/// The kinds of object that are content-addressed.
///
/// The manifest is deliberately absent. It is mutable, lives at a fixed path,
/// and its `object_id` is 20 zero bytes rather than a name tag (spec §4.3) —
/// so "the manifest's name" is not a value that exists, and making it
/// inexpressible keeps a caller from inventing one and using it as an
/// `object_id` in a [`crate::backend::Binding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Entry,
    Attachment,
}

impl From<ContentKind> for ObjectKind {
    fn from(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Entry => Self::Entry,
            ContentKind::Attachment => Self::Attachment,
        }
    }
}

/// The name of one sealed object: the 20-byte tag, plus its on-disk encoding.
///
/// The tag *is* the object's `object_id` in its [`crate::backend::Binding`]
/// (spec §4.4), so the name and the binding agree by construction. It is not
/// secret — it is a filename — but it is meaningless without `K_name`.
///
/// The same 20 bytes are `journal_model::ObjectName`, which that crate
/// carries so entries and manifests can point at objects. The two types are
/// separate because `journal-model` has no crypto dependency; the eventual
/// `journal-store` bridge converts between them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectName([u8; OBJECT_ID_LEN]);

impl ObjectName {
    /// Wraps a tag read back from a manifest or a vault directory.
    #[must_use]
    pub fn from_tag(tag: [u8; OBJECT_ID_LEN]) -> Self {
        Self(tag)
    }

    /// The raw 20-byte tag, for use as a `Binding`'s `object_id`.
    #[must_use]
    pub fn tag(&self) -> &[u8; OBJECT_ID_LEN] {
        &self.0
    }

    /// The 32-character lowercase Crockford Base32 name used on disk
    /// (spec §4.4).
    #[must_use]
    pub fn encoded(&self) -> String {
        base32::encode(Alphabet::Crockford, &self.0).to_lowercase()
    }
}

impl fmt::Display for ObjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encoded())
    }
}

impl fmt::Debug for ObjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectName({})", self.encoded())
    }
}

/// Names an object by the keyed tag of spec §4.4.
///
/// `content` is the object's plaintext bytes — the serialized entry or the
/// blob — and never a sealed object or a binding preamble.
///
/// # Panics
/// Never: HMAC accepts a key of any length, and this one is a fixed 32 bytes,
/// so the `expect` below is unreachable rather than a fallback.
#[must_use]
pub fn object_name(name_key: &NameKey, kind: ContentKind, content: &[u8]) -> ObjectName {
    let content_hash = Sha256::digest(content);

    // `new_from_slice` rejects nothing for HMAC — every key length is valid,
    // and this one is fixed at 32 bytes besides.
    #[expect(clippy::expect_used, reason = "HMAC accepts a key of any length")]
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(name_key.expose())
        .expect("HMAC-SHA-256 accepts a 32-byte key");
    mac.update(NAME_DOMAIN);
    mac.update(&[ObjectKind::from(kind) as u8]);
    mac.update(&content_hash);

    let tag = mac.finalize().into_bytes();
    let mut name_tag = [0u8; OBJECT_ID_LEN];
    name_tag.copy_from_slice(&tag[..OBJECT_ID_LEN]);
    ObjectName(name_tag)
}

/// Derives `K_name` for the `age` backend from the vault identity string
/// (spec §4.4's age row, §5.3).
///
/// Full HKDF — Extract **and** Expand — because the input is a Bech32
/// *encoding* of a key rather than a uniformly random key, and Extract is
/// what makes that safe. The ikm is the ASCII bytes of the **uppercase**
/// string `age::x25519::Identity::to_string()` returns; the case is folded
/// here rather than trusted, because a lowercased identity string derives a
/// different key and therefore a different name for every object in the
/// vault, and nothing downstream would notice.
///
/// Per RFC 5869 an absent salt is `HashLen` zero bytes, which is what the
/// spec's `salt = b""` means.
///
/// # Panics
/// Never: the only fallible step is expanding to 32 bytes, far below HKDF's
/// 255 × `HashLen` output limit.
#[must_use]
pub fn name_key_from_age_identity(identity: &SecretString) -> NameKey {
    let ikm = Zeroizing::new(identity.expose_secret().to_ascii_uppercase());
    let mut info = [0u8; 9];
    info[..8].copy_from_slice(KDF_DOMAIN);
    info[8] = PURPOSE_NAME;

    let hkdf = Hkdf::<Sha256>::new(None, ikm.as_bytes());
    NameKey::new(expand_key(&hkdf, &info))
}

/// Derives `K_name` for the native backend from the DEK (spec §4.4's native
/// row, §6.3).
///
/// Expand-only, because the DEK is already a uniformly random 256-bit key
/// (RFC 5869 §3.3), and with §6.3's fixed-width 30-byte `info` — the same
/// construction every other key below the DEK uses, not the age row's shorter
/// one. The two rows are not interchangeable.
///
/// The native backend itself is M2 work. This lives here now because the
/// derivation is computable today and spec §9's second vector pins it: the
/// per-backend table is the likeliest thing in the spec to be implemented as
/// though it were a single construction.
///
/// # Panics
/// Never: HKDF-SHA-256 needs a pseudorandom key at least `HashLen` long and
/// the DEK is exactly that, and 32 bytes of output is far below the 255 ×
/// `HashLen` limit.
#[must_use]
pub fn name_key_from_dek(dek: &Dek) -> NameKey {
    // Spec §6.3: b"JRNLKDF1" || purpose || object_kind || object_id, with the
    // last two zeroed for every purpose but 1.
    let mut info = [0u8; 30];
    info[..8].copy_from_slice(KDF_DOMAIN);
    info[8] = PURPOSE_NAME;

    #[expect(clippy::expect_used, reason = "a 32-byte PRK is SHA-256's own output length")]
    let hkdf = Hkdf::<Sha256>::from_prk(dek.expose().as_slice())
        .expect("HKDF-SHA-256 accepts a 32-byte pseudorandom key");
    NameKey::new(expand_key(&hkdf, &info))
}

/// HKDF-Expand to a 32-byte key, into a buffer that zeroizes on drop.
fn expand_key(hkdf: &Hkdf<Sha256>, info: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut okm = Zeroizing::new([0u8; 32]);
    // `expand` fails only for an output longer than 255 * HashLen = 8160
    // bytes.
    #[expect(clippy::expect_used, reason = "L = 32 is far below HKDF's 255 * HashLen limit")]
    hkdf.expand(info, okm.as_mut()).expect("HKDF-SHA-256 always produces 32 bytes");
    okm
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use sha2::{Digest, Sha256};
    use zeroize::Zeroizing;

    use super::{
        name_key_from_age_identity, name_key_from_dek, object_name, ContentKind, ObjectName,
        OBJECT_NAME_CHARS,
    };
    use crate::backend::{Dek, OBJECT_ID_LEN};

    /// The identity string of `tests/vectors/naming.json`, and a second one,
    /// both real Bech32 age keys.
    const IDENTITY: &str =
        "AGE-SECRET-KEY-1QYPQXPQ9QCRSSZG2PVXQ6RS0ZQG3YYC5Z5TPWXQERGD3C8G7RUSQGPQYEE";
    const OTHER_IDENTITY: &str =
        "AGE-SECRET-KEY-1YQSJYGEYY5NZW2PF9G4JCTFW9UCRZV3NXS6NVDEC8YARK0PA8CLSWXGG7Q";
    const CONTENT: &[u8] = b"# First light\n\nWoke early, walked the long way.\n";

    fn identity(text: &str) -> SecretString {
        SecretString::from(text.to_owned())
    }

    #[test]
    fn a_blobs_on_disk_name_equals_no_hash_of_its_plaintext() {
        // The confirmation-oracle regression test (ADR-0013). If this ever
        // fails, someone holding the vault directory can test a guess at a
        // file's contents by hashing it and looking for that name.
        let key = name_key_from_age_identity(&identity(IDENTITY));
        let name = object_name(&key, ContentKind::Attachment, CONTENT);

        let hash = Sha256::digest(CONTENT);
        let mut truncated_hash = [0u8; OBJECT_ID_LEN];
        truncated_hash.copy_from_slice(&hash[..OBJECT_ID_LEN]);
        assert_ne!(name.tag(), &truncated_hash, "the name is the content hash");
        assert_ne!(
            name.encoded(),
            ObjectName::from_tag(truncated_hash).encoded(),
            "the name encodes the content hash",
        );

        // Nor is it a hash of anything else an attacker can compute without
        // the vault secret: the kind byte and the domain separator are inside
        // the HMAC, not outside it.
        let mut hasher = Sha256::new();
        hasher.update(b"JRNLNAM1");
        hasher.update([3u8]);
        hasher.update(hash);
        let unkeyed = hasher.finalize();
        assert_ne!(name.tag()[..], unkeyed[..OBJECT_ID_LEN]);
    }

    #[test]
    fn the_same_content_under_two_vault_secrets_gets_two_names() {
        // Deduplication is deliberately vault-local (ADR-0013): two vaults
        // holding the same photograph must not agree on its name, or the
        // storage provider learns they hold the same file.
        let mine = name_key_from_age_identity(&identity(IDENTITY));
        let theirs = name_key_from_age_identity(&identity(OTHER_IDENTITY));
        assert_ne!(
            object_name(&mine, ContentKind::Attachment, CONTENT),
            object_name(&theirs, ContentKind::Attachment, CONTENT),
        );
    }

    #[test]
    fn the_same_content_under_one_secret_gets_one_name() {
        // The other half of the trade: inside a vault, an image added twice
        // is stored once.
        let key = name_key_from_age_identity(&identity(IDENTITY));
        assert_eq!(
            object_name(&key, ContentKind::Attachment, CONTENT),
            object_name(&key, ContentKind::Attachment, CONTENT),
        );
    }

    #[test]
    fn entries_and_attachments_live_in_separate_namespaces() {
        // `kind_u8` is inside the HMAC (spec §4.4) precisely so identical
        // bytes stored as an entry and as a blob do not collide.
        let key = name_key_from_age_identity(&identity(IDENTITY));
        assert_ne!(
            object_name(&key, ContentKind::Entry, CONTENT),
            object_name(&key, ContentKind::Attachment, CONTENT),
        );
    }

    #[test]
    fn the_encoded_name_is_32_crockford_characters() {
        let key = name_key_from_age_identity(&identity(IDENTITY));
        let encoded = object_name(&key, ContentKind::Entry, CONTENT).encoded();
        assert_eq!(encoded.chars().count(), OBJECT_NAME_CHARS);
        assert!(
            encoded.chars().all(|c| "0123456789abcdefghjkmnpqrstvwxyz".contains(c)),
            "`{encoded}` is not lowercase Crockford Base32 — i, l, o, u are absent by design",
        );
    }

    #[test]
    fn the_identity_strings_case_does_not_change_the_key() {
        // Spec §5.3: the ikm is the uppercase Bech32 string. A lowercased one
        // would otherwise rename every object in the vault, silently.
        let upper = name_key_from_age_identity(&identity(IDENTITY));
        let lower = name_key_from_age_identity(&identity(&IDENTITY.to_lowercase()));
        assert_eq!(upper.expose(), lower.expose());
    }

    #[test]
    fn the_age_and_native_rows_are_not_one_construction() {
        // Both start from 32 bytes of vault secret, and a single shared
        // derivation would look right everywhere except on disk.
        let dek = Dek::new(Zeroizing::new([7u8; 32]));
        let native = name_key_from_dek(&dek);
        let aged = name_key_from_age_identity(&identity(IDENTITY));
        assert_ne!(native.expose(), aged.expose());
    }

    #[test]
    fn a_tag_round_trips_through_the_public_constructor() {
        let tag = [0xa5u8; OBJECT_ID_LEN];
        assert_eq!(ObjectName::from_tag(tag).tag(), &tag);
    }
}
