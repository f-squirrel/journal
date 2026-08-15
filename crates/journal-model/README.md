# journal-model

Domain types shared across the workspace: journal entries, attachments, the
vault manifest, tombstones, and object IDs. Pure data types and their
(de)serialization — no crypto, no I/O.

**Status:** Task 3 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md)
is implemented: entry, attachment, manifest, tombstone, and object-ID types,
with explicit versioned serialization and golden-file tests. Nothing seals,
stores, or indexes these yet — that's `journal-crypto` and `journal-store`.

## Contents

- `entry.rs` — `Entry`: a journal entry's Markdown body plus title, tags,
  and timestamps. This is the plaintext content sealed as an
  `ObjectKind::Entry` object (crypto spec §4.3); `id` is a stable identifier
  that survives edits, distinct from the content-addressed object name.
- `attachment.rs` — `Attachment`: a reference from an entry to an
  attachment blob's content-addressed name and MIME type. The blob's own
  bytes are a separate, unwrapped object.
- `manifest.rs` — `Manifest`, `ManifestEntry`, `Tombstone`: `id -> current
  object` for every entry, plus delete tombstones (ADR-0015).
- `ids.rs` — `EntryId` (stable per-entry identifier) and `ObjectName` (the
  raw 20-byte content-addressed object tag, crypto spec §4.4; encoding it
  to the on-disk 32-character path is `journal-crypto`'s job).
- `error.rs` — `ModelError`.

Each of `Entry` and `Manifest` carries an explicit `format_version` field,
serializes via `to_bytes`/`from_bytes` (JSON), rejects unknown fields
(`#[serde(deny_unknown_fields)]`), and rejects an unsupported version rather
than migrating silently — workspace security conventions, ADR-0016. Golden
fixtures live under `tests/golden/`.

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
