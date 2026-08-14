# journal-model

Domain types shared across the workspace: journal entries, attachments, the
vault manifest, tombstones, and object IDs. Pure data types and their
(de)serialization — no crypto, no I/O.

**Status:** stub. Only a module doc comment and a compile-check test exist;
see Task 3 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md).

## Planned contents

- `entry.rs` — journal entry (Markdown body, metadata).
- `attachment.rs` — embedded image/attachment references.
- `manifest.rs` — the vault manifest and delete tombstones.
- `ids.rs` — object-ID types.

## Conventions

- Serialization is explicit and versioned: no `#[serde(flatten)]`, no
  silent acceptance of unknown fields. A format change should fail loudly
  (golden-file tests), never migrate silently.
- No I/O and no dependency on `journal-crypto` or `journal-store` — this
  crate defines shapes, not behavior. `journal-store` depends on this crate,
  not the other way around.

## Used by

`journal-store` (persists these types as encrypted objects) and
`apps/desktop` (exposes them over Tauri IPC).
