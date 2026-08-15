//! Errors from parsing and serializing the on-disk model types.

use thiserror::Error;

/// Errors from (de)serializing a model type's canonical on-disk bytes.
///
/// This crate never accepts a document it cannot fully validate: an unknown
/// field or an unsupported `format_version` is always rejected, never
/// migrated silently (workspace security conventions; ADR-0016).
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("malformed {kind} document")]
    Deserialize {
        kind: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to serialize {kind} document")]
    Serialize {
        kind: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("unsupported {kind} format version {version}")]
    UnsupportedVersion { kind: &'static str, version: u16 },
}
