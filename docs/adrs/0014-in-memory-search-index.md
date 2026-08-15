# 14. The search index is derived, disposable, and never written to disk

## Status

Accepted — 2026-08-15 (design doc §6, §9; first revision 2026-08-14)

## Context

Full-text search over a journal is table stakes, and the standard answer is a
SQLite FTS5 index on disk, updated incrementally as entries change. The
original design said exactly that.

It also directly contradicted M1's own done-criterion: *every byte the app
writes anywhere on disk is unreadable without the passphrase*. A plaintext FTS
table over entry bodies would leave the whole journal in cleartext beside a
vault we went to considerable trouble to encrypt — the single element of the
design that negated the milestone's guarantee.

An incremental on-disk index has a second, quieter problem: it is a second
source of truth that can drift out of sync with the vault.

## Decision

Two layers with a deliberate split. **The vault is the source of truth**;
**the index is derived and disposable** — rebuildable from the vault at any
time, never synced, never authoritative.

At M1 the index is a SQLite `:memory:` database with FTS5 over decrypted titles
and bodies, **built on unlock and dropped on lock**. No part of it touches
disk. Rebuild-from-vault is the *only* path — there is no incremental on-disk
state to drift.

If a full rebuild of a realistic vault stops being fast enough to hide behind
the unlock screen, the replacement is an **encrypted** on-disk index (SQLCipher
or an encrypted-page VFS). A plaintext one is not on the table.

## Consequences

- Search is fast without inventing a second thing to encrypt correctly, and the
  desktop and mobile cases get the same default.
- Unlock pays a rebuild cost proportional to vault size, and memory holds the
  decrypted index for as long as the vault is unlocked.
- The threshold that would force an encrypted on-disk index is a
  **measurement, not a guess**: the M1 Task 10 test prints rebuild time for a
  1,000-entry vault.
- Search results are testable against a brute-force scan of the vault, and "the
  index is empty after lock" is an assertion rather than an aspiration.
- Dropping the index is part of the lock teardown of
  [ADR-0005](0005-ipc-boundary-and-lock-teardown.md).
