# 18. Sync through a four-method provider trait, Dropbox first

## Status

Accepted — 2026-08-15 (design doc §7, 2026-08-14). Implementation deferred to
M2; the conflict-resolution model gets its own spec before it is built.

## Context

F5 asks for Dropbox sync, extensible to other providers. The tempting shape is
a provider interface rich enough to exploit each service — server-side diffing,
change cursors, conflict detection, file-revision APIs. That shape makes the
first provider fast and every subsequent one a rewrite, and it spreads
correctness-critical logic across implementations we cannot test uniformly.

Sync is also the hardest correctness problem in the project, and the one most
likely to lose data if rushed.

## Decision

`journal-sync` owns the algorithm; providers are dumb blob stores exposing four
methods:

```rust
trait SyncProvider {
    async fn list(&self, prefix: &str) -> Result<Vec<RemoteObject>>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn put(&self, key: &str, body: &[u8]) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}
```

Dropbox is the first implementation, authenticating with **OAuth 2.0 + PKCE**
so no client secret ships in a binary. M4 adds a second provider specifically
to prove the trait.

Conflict handling is deferred to its own M2 spec. The starting position:
immutable object versions plus a manifest make most "conflicts" merges of
disjoint object sets; genuine same-entry divergence produces a **conflict copy
rather than silent data loss**; deletions propagate as manifest tombstones
([ADR-0015](0015-tombstones-and-blob-gc.md)). CRDT-based merging is a possible
later refinement, not a milestone commitment.

## Consequences

- Adding Google Drive, WebDAV, S3, or a plain synced folder is contained work,
  and a provider is not part of the security model — it only ever sees
  ciphertext under keyed names.
- All reconciliation logic lives in one crate, so it is tested once.
- The trait cannot exploit provider-specific features (delta cursors, native
  conflict detection), which costs efficiency on large vaults. Accepted.
- Sync is where several deferred items come due at once: whole-vault rollback
  protection ([ADR-0016](0016-versioned-formats-fail-closed.md)), GC safety
  across replicas, object-size padding, and fuzzing the envelope parser — all
  of which will be handling attacker-influenced bytes the moment sync exists.
- Nothing in M1 makes an outbound connection, and CI asserts it.
