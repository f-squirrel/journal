//! The vault manifest: `id -> current object` for every entry, plus delete
//! tombstones (crypto spec §4.3, `ObjectKind::Manifest`; ADR-0015).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::{EntryId, ObjectName};

/// The current on-disk format version for [`Manifest`]. Bump this, and add
/// a migration or a hard refusal, before changing any field below —
/// this crate has no silent-migration path (workspace security
/// conventions; ADR-0016).
pub const MANIFEST_FORMAT_VERSION: u16 = 1;

/// Where an entry's current revision lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    /// The content-addressed name of the entry's current sealed object.
    pub object_name: ObjectName,
}

/// A record that an entry was deleted (ADR-0015) — not the absence of an
/// object, since content addressing plus "copy whatever the other side is
/// missing" sync would otherwise resurrect it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tombstone {
    /// Unix seconds, UTC.
    pub deleted_at: i64,
}

/// The decrypted content of the manifest object: what a client reads to
/// know which entries exist (ADR-0015) — the object directory on its own is
/// not authoritative.
///
/// This is the **content** that gets sealed as the `ObjectKind::Manifest`
/// object (`object_id` = 20 zero bytes, crypto spec §4.3) — not the sealed
/// bytes themselves, which are `journal-crypto`'s concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format_version: u16,
    pub entries: BTreeMap<EntryId, ManifestEntry>,
    pub tombstones: BTreeMap<EntryId, Tombstone>,
}

impl Manifest {
    /// Serializes to the canonical on-disk representation: the bytes that
    /// become the sealed manifest object's content (crypto spec §4.4).
    ///
    /// # Errors
    /// Returns [`ModelError`] if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ModelError> {
        serde_json::to_vec(self)
            .map_err(|source| ModelError::Serialize { kind: "manifest", source })
    }

    /// Parses the canonical on-disk representation.
    ///
    /// Unknown fields and unsupported format versions are rejected rather
    /// than silently accepted or migrated (workspace security conventions;
    /// ADR-0016). The `format_version` is checked *before* the strict,
    /// `deny_unknown_fields` parse: a document from a newer format version
    /// legitimately carries fields this build doesn't know about, and that
    /// must surface as `UnsupportedVersion`, not as a generic "malformed
    /// document" error from the strict parse tripping over them first.
    ///
    /// # Errors
    /// Returns [`ModelError`] if the bytes are malformed or carry an
    /// unsupported `format_version`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ModelError> {
        #[derive(Deserialize)]
        struct VersionProbe {
            format_version: u16,
        }

        let probe: VersionProbe = serde_json::from_slice(bytes)
            .map_err(|source| ModelError::Deserialize { kind: "manifest", source })?;
        if probe.format_version != MANIFEST_FORMAT_VERSION {
            return Err(ModelError::UnsupportedVersion {
                kind: "manifest",
                version: probe.format_version,
            });
        }

        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|source| ModelError::Deserialize { kind: "manifest", source })?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::{Manifest, ManifestEntry, Tombstone, MANIFEST_FORMAT_VERSION};
    use crate::error::ModelError;
    use crate::ids::{EntryId, ObjectName, OBJECT_NAME_LEN};

    fn sample() -> Manifest {
        let mut entries = std::collections::BTreeMap::new();
        entries.insert(
            EntryId::new_random(),
            ManifestEntry { object_name: ObjectName::from_bytes([5u8; OBJECT_NAME_LEN]) },
        );
        let mut tombstones = std::collections::BTreeMap::new();
        tombstones.insert(EntryId::new_random(), Tombstone { deleted_at: 1_755_216_000 });
        Manifest { format_version: MANIFEST_FORMAT_VERSION, entries, tombstones }
    }

    #[test]
    fn round_trips_byte_identical() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = sample();
        let bytes = manifest.to_bytes()?;
        let parsed = Manifest::from_bytes(&bytes)?;
        assert_eq!(manifest, parsed);
        Ok(())
    }

    #[test]
    fn deleted_entry_leaves_a_tombstone_and_no_entry() -> Result<(), Box<dyn std::error::Error>> {
        let mut manifest = sample();
        let (id, _) = manifest
            .entries
            .iter()
            .next()
            .map(|(id, entry)| (*id, entry.clone()))
            .ok_or("expected a seeded entry")?;
        manifest.entries.remove(&id);
        manifest.tombstones.insert(id, Tombstone { deleted_at: 1_755_216_100 });

        assert!(!manifest.entries.contains_key(&id));
        assert!(manifest.tombstones.contains_key(&id));
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = br#"{"format_version":1,"entries":{},"tombstones":{},"extra":1}"#;
        let result = Manifest::from_bytes(json);
        assert!(matches!(result, Err(ModelError::Deserialize { kind: "manifest", .. })));
    }

    #[test]
    fn rejects_unsupported_format_version() -> Result<(), Box<dyn std::error::Error>> {
        let mut manifest = sample();
        manifest.format_version = 99;
        let bytes = serde_json::to_vec(&manifest)?;
        let result = Manifest::from_bytes(&bytes);
        assert!(matches!(
            result,
            Err(ModelError::UnsupportedVersion { kind: "manifest", version: 99 })
        ));
        Ok(())
    }

    #[test]
    fn newer_version_with_unknown_field_is_unsupported_version_not_deserialize_error() {
        // A document from a genuinely newer format version legitimately
        // carries a field this build doesn't know about — that must be
        // `UnsupportedVersion`, not a generic "malformed document" error.
        let json = br#"{"format_version":2,"entries":{},"tombstones":{},"synced_at":123}"#;
        let result = Manifest::from_bytes(json);
        assert!(matches!(
            result,
            Err(ModelError::UnsupportedVersion { kind: "manifest", version: 2 })
        ));
    }
}
