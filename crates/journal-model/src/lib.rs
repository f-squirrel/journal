//! Domain types shared across the journal workspace: entries, attachments,
//! manifests, and object IDs. Pure data types and their (de)serialization —
//! no crypto, no I/O. See `crates/journal-model/README.md` and Task 3 of
//! `docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`.

pub mod attachment;
pub mod entry;
pub mod error;
pub mod ids;
pub mod manifest;

pub use attachment::Attachment;
pub use entry::{Entry, ENTRY_FORMAT_VERSION};
pub use error::ModelError;
pub use ids::{EntryId, ObjectName, OBJECT_NAME_LEN};
pub use manifest::{Manifest, ManifestEntry, Tombstone, MANIFEST_FORMAT_VERSION};
