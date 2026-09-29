#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]

//! The contract between the backend and the frontend: every type that crosses
//! the `invoke` bridge, and the one crate `src-tauri` names them through.
//!
//! Types that exist only for the bridge live here. The entry types are
//! re-exported from `journal-entry` rather than defined here, because they are
//! also what the vault stores: one serde impl serves both the wire and the
//! encrypted blob on disk. The settings wire types are re-exported from
//! `journal-settings` for the same reason: the journal half is also what the
//! vault stores. `make bindings` generates `src/bindings/` from all three
//! crates.

mod sync;

pub use journal_entry::{
    Entry, Header, Id, ImageFormat, MediaId, MediaMeta, MediaVariant, SearchMatch,
};
pub use journal_settings::{
    ColorMode, DeviceSettings, JournalSettings, Settings, TimeFormat, WeekStart,
};
pub use sync::{ProviderInfo, SyncState, SyncStatusPayload};
