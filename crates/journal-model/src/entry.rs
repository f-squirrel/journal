//! The journal entry: the plaintext content sealed as an `ObjectKind::Entry`
//! object (crypto spec §4.3).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::attachment::Attachment;
use crate::error::ModelError;
use crate::ids::EntryId;

/// The current on-disk format version for [`Entry`]. Bump this, and add a
/// migration or a hard refusal, before changing any field below — this
/// crate has no silent-migration path (workspace security conventions;
/// ADR-0016).
pub const ENTRY_FORMAT_VERSION: u16 = 1;

/// A journal entry: Markdown body plus the metadata the in-memory search
/// index reads after decrypting (design doc §6).
///
/// This is the **content** that gets sealed as an `ObjectKind::Entry`
/// object — not the sealed bytes themselves, which are `journal-crypto`'s
/// concern. `id` is a stable identifier chosen once at creation; because
/// entries are content-addressed (crypto spec §4.4), every edit is sealed
/// under a new object name, so the manifest ([`crate::manifest::Manifest`])
/// tracks `id -> current object name` rather than relying on the object
/// name to identify "this entry" across edits.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub format_version: u16,
    pub id: EntryId,
    pub title: String,
    /// The entry body, in Markdown, with images referenced as
    /// `journal://blob/<name>`.
    pub body: String,
    pub tags: Vec<String>,
    /// Unix seconds, UTC. Carried forward unchanged across edits.
    pub created_at: i64,
    /// Unix seconds, UTC. The write time of this particular revision.
    pub updated_at: i64,
    pub attachments: Vec<Attachment>,
}

/// Redacts `title` and `body` so `tracing::debug!(?entry)` and friends can
/// never leak plaintext journal content (workspace security conventions:
/// "No key material, entry bodies, titles, or passphrases in `tracing`
/// output, at any level").
impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("format_version", &self.format_version)
            .field("id", &self.id)
            .field("title", &format_args!("<redacted, {} chars>", self.title.chars().count()))
            .field("body", &format_args!("<redacted, {} chars>", self.body.chars().count()))
            .field("tags", &self.tags)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("attachments", &self.attachments)
            .finish()
    }
}

impl Entry {
    /// Serializes to the canonical on-disk representation: the bytes that
    /// become the sealed object's content (crypto spec §4.4).
    ///
    /// # Errors
    /// Returns [`ModelError`] if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ModelError> {
        serde_json::to_vec(self).map_err(|source| ModelError::Serialize { kind: "entry", source })
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
            .map_err(|source| ModelError::Deserialize { kind: "entry", source })?;
        if probe.format_version != ENTRY_FORMAT_VERSION {
            return Err(ModelError::UnsupportedVersion {
                kind: "entry",
                version: probe.format_version,
            });
        }

        let entry: Self = serde_json::from_slice(bytes)
            .map_err(|source| ModelError::Deserialize { kind: "entry", source })?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, ENTRY_FORMAT_VERSION};
    use crate::attachment::Attachment;
    use crate::error::ModelError;
    use crate::ids::{EntryId, ObjectName, OBJECT_NAME_LEN};

    fn sample() -> Entry {
        Entry {
            format_version: ENTRY_FORMAT_VERSION,
            id: EntryId::new_random(),
            title: "Field notes".to_string(),
            body: "Saw a heron on the walk in.\n\n![heron](journal://blob/abc)".to_string(),
            tags: vec!["birds".to_string(), "walks".to_string()],
            created_at: 1_755_216_000,
            updated_at: 1_755_216_000,
            attachments: vec![Attachment {
                name: ObjectName::from_bytes([3u8; OBJECT_NAME_LEN]),
                content_type: "image/jpeg".to_string(),
            }],
        }
    }

    #[test]
    fn round_trips_byte_identical() -> Result<(), Box<dyn std::error::Error>> {
        let entry = sample();
        let bytes = entry.to_bytes()?;
        let parsed = Entry::from_bytes(&bytes)?;
        assert_eq!(entry, parsed);
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = br#"{"format_version":1,"id":"3d6f0c1a-9b2e-4a3f-8c1d-1a2b3c4d5e6f","title":"t","body":"b","tags":[],"created_at":0,"updated_at":0,"attachments":[],"unexpected":true}"#;
        let result = Entry::from_bytes(json);
        assert!(matches!(result, Err(ModelError::Deserialize { kind: "entry", .. })));
    }

    #[test]
    fn rejects_unsupported_format_version() -> Result<(), Box<dyn std::error::Error>> {
        let mut entry = sample();
        entry.format_version = 2;
        let bytes = serde_json::to_vec(&entry)?;
        let result = Entry::from_bytes(&bytes);
        assert!(matches!(
            result,
            Err(ModelError::UnsupportedVersion { kind: "entry", version: 2 })
        ));
        Ok(())
    }

    #[test]
    fn newer_version_with_unknown_field_is_unsupported_version_not_deserialize_error() {
        // A document from a genuinely newer format version legitimately
        // carries a field this build doesn't know about — that must be
        // `UnsupportedVersion`, not a generic "malformed document" error.
        let json = br#"{"format_version":2,"id":"3d6f0c1a-9b2e-4a3f-8c1d-1a2b3c4d5e6f","title":"t","body":"b","tags":[],"created_at":0,"updated_at":0,"attachments":[],"mood":"happy"}"#;
        let result = Entry::from_bytes(json);
        assert!(matches!(
            result,
            Err(ModelError::UnsupportedVersion { kind: "entry", version: 2 })
        ));
    }

    #[test]
    fn debug_redacts_title_and_body() {
        let entry = sample();
        let debug = format!("{entry:?}");
        assert!(!debug.contains(&entry.title));
        assert!(!debug.contains("heron"));
        assert!(debug.contains("<redacted"));
    }
}
