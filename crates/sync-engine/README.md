# journal-sync-engine

The sync engine: replicates the journal's encrypted blobs between the local
`BlobStore` and a `journal-remote` `RemoteStore`. Local-first (ADR 0009) — the
vault never waits on it. It holds no key and moves ciphertext only; anything
that needs plaintext, i.e. deciding what a conflict means, is the
`ConflictResolver` hook the app implements.

`SyncEngine::sync_once` runs one pass. Each blob is compared with a *base* —
the remote revision and local hash from the last time the two agreed, kept per
device in a state file outside the synced store, never uploaded. "Changed"
means different from the base; no clocks are compared. So a delete never
outranks an edit, a conflict is parked for the resolver rather than settled by
last-writer-wins, and losing the state file costs extra comparison, not text.
The sealed key blob (`KEY_BLOB`) is never merged: two different keys stop the
pass with `SyncError::KeyMismatch`. Base state is written per blob after it
succeeds and the cursor last, so any failed pass can simply be re-run. A cursor
the remote no longer honours (`RemoteError::CursorExpired`) is not a failure:
the pass lists everything from scratch, the same way a first run does, and a
blob missing from that listing is read as deleted.

`SyncEngine::status` says which blobs are not uploaded yet (`pending`) and
which conflicts wait on the resolver (`conflicted`), for the UI's per-entry
marks; `mark_dirty` and `NotifyingStore` feed it from local saves. Retry,
backoff and scheduling are not here: errors come back typed
(`SyncErrorKind`) and the app's worker decides.

`erase_state` deletes the state file and the parked conflicts beside it, for an
app that is disconnecting; the local store is not touched.

Give the engine the store *beneath* `NotifyingStore`, so its own pulls are not
counted as local edits. Tests run two devices (two `FsStore`s) against one
`MemoryRemote`.
