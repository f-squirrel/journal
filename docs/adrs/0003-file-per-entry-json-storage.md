# 0003. Store journal entries as one JSON file per entry

## Status

Accepted — 2026-09-06

## Decision

Persist journal entries as individual JSON files (`<id>.json`) in a directory
under the Tauri app data dir, implemented in a new `journal-storage` crate
(`crates/storage/`) and exposed to the frontend via Tauri commands
(`list_entries`, `get_entry`, `save_entry`, `delete_entry`) in
`src-tauri/src/lib.rs`. Previously entries only lived in React state
(`AppStateContext`) with no persistence at all.

## Reasons

The app's planned sync feature (Dropbox/Google Drive/iCloud) syncs
directories of files. One file per entry maps directly onto that model —
each entry diffs, syncs, and conflicts independently — with no extra work
needed when sync is built later.

## Alternatives considered

- **SQLite** (via `rusqlite`/`sqlx`): gives transactions, indexing, and
  querying, but a single database file doesn't sync well through
  file-syncing services — concurrent edits from two devices need real
  merge/conflict handling instead of just picking the newer file.
- **Single JSON blob** for all entries: simpler than one-file-per-entry, but
  every save rewrites the whole file, and it has the same whole-file sync
  conflict problem as SQLite, just without the query benefits.

Both were rejected in favor of file-per-entry given the app's small
per-user data size, where DB-level querying/indexing isn't needed.
