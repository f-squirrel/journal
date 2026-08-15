//! A reference from a journal entry to an attachment blob (crypto spec
//! §4.3, `ObjectKind::Attachment`).
//!
//! This type does not carry the blob's bytes — those are the attachment
//! object's own content, sealed and stored separately by `journal-store`.
//! It carries only what an [`crate::entry::Entry`] needs to point at one:
//! which object, and how to render it.

use serde::{Deserialize, Serialize};

use crate::ids::ObjectName;

/// One embedded image or file, referenced from an entry's Markdown body via
/// `journal://blob/<name>` (design doc §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    /// The content-addressed name of the sealed attachment object.
    pub name: ObjectName,
    /// MIME type as supplied by the UI at attach time (e.g. `"image/png"`),
    /// so the blob protocol handler (plan Task 12) can set `Content-Type`
    /// without sniffing decrypted, attacker-influenced bytes.
    pub content_type: String,
}

#[cfg(test)]
mod tests {
    use super::Attachment;
    use crate::ids::{ObjectName, OBJECT_NAME_LEN};

    fn sample() -> Attachment {
        Attachment {
            name: ObjectName::from_bytes([9u8; OBJECT_NAME_LEN]),
            content_type: "image/png".to_string(),
        }
    }

    #[test]
    fn round_trips_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let attachment = sample();
        let json = serde_json::to_string(&attachment)?;
        let parsed: Attachment = serde_json::from_str(&json)?;
        assert_eq!(attachment, parsed);
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = r#"{"name":"0909090909090909090909090909090909090909","content_type":"image/png","extra":true}"#;
        let result: Result<Attachment, _> = serde_json::from_str(json);
        assert!(result.is_err(), "an unknown field must not be silently accepted");
    }
}
