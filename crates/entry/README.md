# journal-entry

Core `Entry`/`Header`/`Id` types shared across the backend: a UUID-backed
`Id`, and an `Entry` (`Header` + markdown body) that (de)serializes with the
header fields flattened. No I/O — just the data model.

`SyncStatusPayload`/`SyncState` are the shape of what the app tells the
frontend about sync; deriving them from the engine's status is `src-tauri`'s
job.

The off-by-default `ts` feature derives the TypeScript wire types in
`src/bindings/` (run `make bindings`).
