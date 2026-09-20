// The shapes below cross the Tauri `invoke` bridge, so they are generated from
// the Rust definitions in `crates/entry` by ts-rs rather than hand-written —
// a field rename on the Rust side then breaks `tsc` instead of silently
// failing at runtime. Regenerate with:
//
//     cargo test -p journal-entry --features ts
//
// The app imports these names from here, not from `./bindings`, so the
// generated file names stay an implementation detail.
export type { Header as EntryHeader } from "./bindings/Header";
export type { Entry as JournalEntry } from "./bindings/Entry";
export type { SearchMatch } from "./bindings/SearchMatch";
export type { SyncStatusPayload } from "./bindings/SyncStatusPayload";
export type { SyncState } from "./bindings/SyncState";
export type { ProviderInfo } from "./bindings/ProviderInfo";
