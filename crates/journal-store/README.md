# journal-store

The on-disk encrypted vault: file layout, atomic writes, the manifest,
entries, attachments/blobs, and an in-memory-only search index. This is
where `journal-model` types meet `journal-crypto` to become ciphertext on
disk.

**Status:** stub. Only a module doc comment and a compile-check test exist;
see Tasks 7–10 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md).

## Planned contents

- `layout.rs` — header, encrypted manifest, blob directory layout under the
  vault root.
- `atomic.rs` — temp file + fsync + rename (+ parent-directory fsync) so a
  crash mid-write can't corrupt the vault.
- `entries.rs` — create/read/update/delete for entries, each going through
  `journal-crypto`; deletes write a manifest tombstone rather than dropping
  the object.
- `blobs.rs` — attachment storage, named `HMAC(K_name, plaintext_hash)` (not
  a raw plaintext hash — that would be a confirmation oracle), plus
  garbage collection of unreferenced blobs.
- `index.rs` — an in-memory SQLite `:memory:` FTS5 index over decrypted
  titles/bodies, rebuilt from the vault on unlock and dropped on lock. It is
  derived and disposable, and at M1 it is **never written to disk** — see
  §6 of the design doc for why.

## Conventions

- An unknown major vault format version refuses to open and never writes —
  no partial upgrades, no half-migrated vaults.
- Manifest writes are serialized through a single owner; two writers must
  not interleave.
- The index is rebuild-only — there is no incremental on-disk state that
  can drift out of sync with the vault.
- Depends on `journal-model` (data shapes) and `journal-crypto`
  (encryption); never encrypts or serializes ad hoc.

## Used by

`apps/desktop`, via Tauri commands.
