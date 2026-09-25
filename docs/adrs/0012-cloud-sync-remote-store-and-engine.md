# 0012. Sync through a separate `RemoteStore` trait and a base-state engine

## Status

Accepted — 2026-09-20

Builds on [0009](0009-blob-store-seam-and-local-first-sync.md), which planned
local-first sync over the blob store without choosing how. Written before the
two-device checks of conflict, remote deletion, disconnect and revoke were run
by hand against Dropbox (`.claude/plans/cloud-sync.md`, T9 results); those
have run only against `MemoryRemote` and a scripted HTTP client so far. If
they turn up behaviour that differs from what is recorded here, amend this
record.

## Decision

Sync moves the encrypted blobs of the local `BlobStore` to a cloud provider
through a second trait, `RemoteStore` (`journal-remote`). The engine that
reconciles the two (`journal-sync`) is synchronous, holds no key, and
compares each blob with a per-device **base** (the revision and hash both
sides last agreed on) instead of using clocks or tombstones. The rules it
applies:

- **D1: keep both on a conflict.** The engine parks the other side's bytes
  and hands them to a `ConflictResolver` the app supplies, since only the app
  holds the key. The app's resolver keeps the newer entry (by `updated_at`)
  under its id and saves the older one as "<title> (conflicted copy)". It
  defers while the vault is locked, and unlocking starts a pass. No
  last-writer-wins.
- **D2: a lost write race is reported, never silent.** A conditional write
  that loses returns `RemoteError::Conflict`. Atomic providers (Dropbox)
  refuse the write. Providers that can only detect the race after writing
  (Drive) return the overwritten bytes in the error.
- **D3: remote deletions come from the change feed and the base.** A blob
  that has a base but is gone from the remote was deleted there. There are no
  tombstone blobs. A cursor the remote no longer honours
  (`RemoteError::CursorExpired`) makes the engine list everything again, and
  a blob missing from that listing counts as deleted.
- **D4: an edit beats a delete.** The edit is uploaded again. A journal must
  not lose text.
- **D5: `.vault-key` is never merged.** If this device and the remote hold
  different keys, sync stops with `KeyMismatch` and changes nothing. Choosing
  which journal to keep is not built yet (`docs/known-issues.md`).
- **D8: the trait and the engine are synchronous.** The engine runs on its
  own thread. Each adapter hides its client: Dropbox uses the official
  `dropbox-sdk` on its blocking `ureq` routes, with no tokio in its tree.

Two parts that were not in the original plan are also part of this decision:

- **A list of providers.** `src-tauri` defines a `Provider` trait (id, name,
  how to connect, the account id, the redirect port, the remote), and a build
  lists the providers it offers. Adding one means an adapter crate, one file
  under `src-tauri/src/sync/`, and one line in `lib.rs`.
- **A provider-neutral login** (`journal-credential`). The saved login holds
  the refresh token, the provider's id and the account's id, in a 0600 file
  outside the synced store. The engine's state (a cursor and bases) describes
  one account's remote. It is kept only while the provider and the account
  both stay the same. Kept across accounts, the new folder's listing would
  read as every entry deleted.

## Reasons

A shared remote needs two things a local directory does not: a change feed
that reports deletions, and conditional writes that report a lost race.
Putting them on `BlobStore` would force the local store to fake both.
Keeping the remote a separate trait also leaves `BlobStore` and everything
above it unaware that sync exists.

Comparing against a base, not a clock, tells "changed here", "changed there"
and "changed on both" apart without trusting device clocks. It also finds
deletions without leaving tombstones behind, which would pile up forever in a
flat app folder.

A synchronous engine matches the synchronous crates below it
(`journal-vault`, `journal-store`). An async trait would need `async-trait` or
boxed futures, and every local read would then go through `spawn_blocking`.
Blobs are a few KB each, so running transfers concurrently would gain little.

Keeping the engine key-less means sync can run while the vault is locked, and
the engine only ever sees ciphertext. Conflict resolution is the one step
that needs plaintext, so it is the one step left to the app.

## Alternatives considered

- **Make the remote a `BlobStore`.** Rejected. The trait has no change feed
  and no conditional write, so a lost race would silently overwrite the other
  device's work.
- **Tombstone blobs for deletions.** Rejected. Tombstones never go away, they
  are more blobs for every device to download, and the provider's change feed
  already reports deletions.
- **Last-writer-wins by timestamp.** Rejected. It silently drops one side's
  text, and it depends on device clocks.
- **An async `RemoteStore`.** Rejected for the reasons above (see D8).
- **Keep the engine state per provider, not per account.** This is how it was
  first built. It lost data when a second account of the same provider was
  connected. Fixed by recording the account id, which needs Dropbox's
  `account_info.read` scope. Reading the id out of the token response instead
  was also possible, but the SDK drops it, so that would have meant doing the
  code exchange by hand.

## Amendment — 2026-09-26: the open entry pane

D1 governs the resolver, the one writer that sees both sides of a conflict. The
entry pane is a second writer, and until now its autosave overwrote whatever
the vault held, including an edit a sync pass had just pulled from another
device. The next pass then pushed the pane's version, so the pulled edit was
lost everywhere. Every save from the pane is now conditional on the
`updated_at` the pane last saw: `save_entry` takes `expected_updated_at`,
`Vault::save_entry_if` checks it, and a mismatch is `VaultError::Stale`.

When a save is refused, the version on disk keeps the id. The pane's latest
draft is saved as a new "<title> (conflicted copy)", and the pane reloads what
is on disk and says so. A pane with no unsaved edits reloads itself on
`sync://changed` instead. This differs from D1, where the *newer* version keeps
the id. The pane cannot order its draft against a write made on another
device, so it never overwrites anything, which keeps D1's guarantee that no
text is lost. D1 is unchanged for the resolver. One known limit: the check and
the write are not atomic against the engine's pulls, which write beneath the
vault, so a pull landing in that window of a few milliseconds is still
overwritten.
