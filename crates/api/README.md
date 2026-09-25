# journal-api

The contract between the Rust backend and the React frontend: every type that
crosses Tauri's `invoke` bridge, and the one crate `src-tauri` names them
through.

It defines the types that exist only for the bridge: `SyncStatusPayload` and
`SyncState` (what the `sync_status` command and the `sync://status` event
carry) and `ProviderInfo` (what `list_providers` returns). It re-exports
`Entry`, `Header`, `Id` and `SearchMatch` from `journal-entry` rather than
defining them, because the vault stores those too: one serde impl serves both
the wire and the encrypted blob on disk. `src-tauri` depends on this crate,
not on `journal-entry`.

Where a new type goes: if the vault stores it, `journal-entry`; if it only
crosses the bridge, here.

No logic and no I/O. Deriving the payload from `journal-sync-engine`'s
blob-level `SyncStatus` (blob names to entry ids, errors to one `SyncState`)
is `src-tauri/src/sync/status.rs`'s job, so this crate does not depend on the
engine.

The off-by-default `ts` feature derives the TypeScript wire types in
`src/bindings/` and turns on `journal-entry`'s own; `make bindings` runs both
crates.
