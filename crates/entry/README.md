# journal-entry

Core `Entry`/`Header`/`Id` types shared across the backend: a UUID-backed
`Id`, and an `Entry` (`Header` + markdown body) that (de)serializes with the
header fields flattened, plus `SearchMatch`, what `Vault::search` returns. No
I/O — just the data model.

One serde impl serves both the encrypted on-disk payload and the `invoke`
wire format, so renaming a field changes both. The app reaches these types
through `journal-api`, which re-exports them.

The off-by-default `ts` feature derives the TypeScript wire types in
`src/bindings/` (run `make bindings`).
