//! Golden vectors for the crypto spec's §9 conformance list.
//!
//! The values in `tests/vectors/` were computed from the specification text
//! by an independent implementation, not by this crate — so a change in this
//! crate that silently alters an on-disk name fails here rather than
//! migrating a user's vault out from under them. Task 16's `tools/decrypt.py`
//! is written against the same files.
//!
//! Vector 2 (object naming) pins **both** `K_name` rows of spec §4.4. The
//! native backend is M2 work, but its row is computable today, and §9 says
//! why it is pinned now: the per-backend table is the single likeliest thing
//! in the spec to be implemented as though the two constructions were one,
//! and they differ in both the HKDF mode and the `info`.

use journal_crypto::naming::{
    name_key_from_age_identity, name_key_from_dek, object_name, ContentKind,
};
use secrecy::SecretString;
use serde_json::Value;
use zeroize::Zeroizing;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const NAMING_VECTORS: &str = include_str!("vectors/naming.json");

/// The `b"JRNLNAM1"` domain separator of spec §4.4, as hex.
const NAME_DOMAIN_HEX: &str = "4a524e4c4e414d31";

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| format!("vector field `{key}` missing"))
}

fn bytes32(hex_str: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let decoded = hex::decode(hex_str)?;
    Ok(<[u8; 32]>::try_from(decoded.as_slice())?)
}

/// Rebuilds the `K_name` of one vector case from the vault secret its
/// backend row starts from (spec §4.4).
fn case_name_key(
    vectors: &Value,
    backend: &str,
) -> Result<journal_crypto::backend::NameKey, Box<dyn std::error::Error>> {
    match backend {
        "age" => {
            let identity = SecretString::from(field(vectors, "age_identity_string")?.to_owned());
            Ok(name_key_from_age_identity(&identity))
        }
        "native" => {
            let dek = Zeroizing::new(bytes32(field(vectors, "native_dek_hex")?)?);
            Ok(name_key_from_dek(&dek))
        }
        other => Err(format!("unknown backend `{other}` in the naming vectors").into()),
    }
}

fn content_kind(kind: u64) -> Result<ContentKind, String> {
    match kind {
        2 => Ok(ContentKind::Entry),
        3 => Ok(ContentKind::Attachment),
        other => Err(format!("object kind {other} is not content-addressed")),
    }
}

#[test]
fn object_naming_matches_the_golden_vectors_under_both_backends() -> TestResult {
    let vectors: Value = serde_json::from_str(NAMING_VECTORS)?;
    let content = hex::decode(field(&vectors, "content_hex")?)?;
    let content_hash_hex = field(&vectors, "content_sha256_hex")?;

    let cases = vectors.get("cases").and_then(Value::as_array).ok_or("vectors have no cases")?;
    assert_eq!(cases.len(), 4, "both backends, both content-addressed kinds");

    for case in cases {
        let backend = field(case, "backend")?;
        let kind_u8 = case.get("object_kind").and_then(Value::as_u64).ok_or("case has no kind")?;
        let kind = content_kind(kind_u8)?;
        let label = format!("{backend} backend, object kind {kind_u8}");

        let name_key = case_name_key(&vectors, backend)?;
        assert_eq!(
            hex::encode(name_key.expose()),
            field(case, "k_name_hex")?,
            "{label}: K_name derivation drifted from spec §4.4 — the two backend rows use \
             different HKDF modes and different info, and are not interchangeable",
        );

        // The pinned HMAC input is what an independent implementation has to
        // reproduce byte for byte; check the vector says what §4.4 says it
        // should before trusting the tag it produced.
        assert_eq!(
            field(case, "hmac_input_hex")?,
            format!("{NAME_DOMAIN_HEX}{kind_u8:02x}{content_hash_hex}"),
            "{label}: the vector's HMAC input is not `JRNLNAM1 || kind || SHA-256(content)`",
        );

        let name = object_name(&name_key, kind, &content);
        assert_eq!(
            hex::encode(name.tag()),
            field(case, "name_tag_hex")?,
            "{label}: name tag changed — this renames every object in every existing vault",
        );
        assert_eq!(
            name.encoded(),
            field(case, "name")?,
            "{label}: the on-disk encoding of the name tag changed",
        );
    }

    Ok(())
}

#[test]
fn the_two_backends_derive_different_naming_keys() -> TestResult {
    // §4.4's table is per backend on purpose: the age row runs full HKDF over
    // a Bech32 *encoding*, the native row Expand-only over a uniform DEK. A
    // single shared construction would pass every other test in this file.
    let vectors: Value = serde_json::from_str(NAMING_VECTORS)?;
    let age = case_name_key(&vectors, "age")?;
    let native = case_name_key(&vectors, "native")?;
    assert_ne!(hex::encode(age.expose()), hex::encode(native.expose()));
    Ok(())
}
