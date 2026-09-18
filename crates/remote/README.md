# journal-remote

A cloud provider's copy of the journal — the abstraction only. `RemoteStore`
is what the sync engine needs from a provider, and it is deliberately not a
`BlobStore`: besides `get`/`put`/`delete` it has a change feed
(`changes(since)`, deletions included, paged with opaque `Cursor`s) and
conditional writes (`Precondition::{Absent, Matches(Revision)}`) that report a
lost race as `RemoteError::Conflict` instead of overwriting.

The trait is synchronous, moves only ciphertext, and holds no implementation:
`journal-dropbox` is the first, and Drive and CloudKit are meant to fit the
same shape. Retry and backoff are not here — errors are typed
(`Unauthorized`, `RateLimited`, `Transient`, …) and the caller decides.

`MemoryRemote`, behind the `testing` feature, is an in-memory implementation
that enforces the contracts. It can simulate both ways a provider loses a
write race (`ConflictMode::Atomic` refuses; `DetectAfterWrite` writes, then
reports), a backend with no content hash, small pages, and injected failures
(`fail_next`, `fail_after`). See ADR 0009 for where this sits in the stack.
