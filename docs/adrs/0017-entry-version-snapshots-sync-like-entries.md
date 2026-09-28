# 0017. Sync entry version snapshots like entries, with no engine changes

## Status

Accepted — 2026-09-28

## Decision

Entry version history (`crates/vault/src/snapshot.rs`) stores each past
version as its own blob, named `<id>.snap-<updated_at millis>.json`, in the
same `BlobStore` the vault already writes entries and the settings blob into.
No new store, no new sync-engine code: `journal-sync-engine` lists, diffs and
transfers a snapshot blob exactly as it does any other blob it doesn't
recognise, because sync already treats every blob generically except
`.vault-key`. The one place that does need to know snapshots exist is
`src-tauri`'s `VaultResolver`, which gets a branch that resolves a same-name
conflict between two snapshots by keeping whichever ciphertext is already
local — both sides decrypt to the same plaintext, so no key is needed to
settle it.

## Reasons

The alternative — keeping history local-only — needs a second `BlobStore`
that the sync engine is never given, which is a materially bigger, separate
change than this feature otherwise requires. Layering snapshots as ordinary
blobs in the existing store gets them synced automatically, for free, at the
cost of that store now growing up to `MAX_SNAPSHOTS_PER_ENTRY` (20) extra
blobs per entry.

That cost is real, not free, and worth stating in numbers rather than waving
at: every sync pass already lists, diffs and transfers every blob in the
store, so a heavily-edited journal can now carry up to 20x as many blobs
through that path as it would with entries alone. Each snapshot upload also
counts toward `SyncStatusPayload.pending_other`, so the sync rail shows
"pending" more often than it did before this feature — a real, user-visible
side effect of the decision, not just an internal cost.

## Alternatives considered

- **A local-only history store**, kept out of sync entirely. Rejected: it
  needs a second `BlobStore` the sync engine never touches, which is a
  bigger and separate design than layering snapshots into the existing one.
  It would also mean an entry's history does not follow it to a second
  device, which the user did not ask for.
- **A device id in snapshot names**, so two devices' snapshots of the same
  version never collide. Rejected: it would store the same version twice in
  every listing and complicate pruning, just to avoid a conflict that
  resolves trivially (see below).

## Consequences

Two devices can independently snapshot the same version of an entry (same
id, same `updated_at`) before syncing, which collides on blob name because
`age` encryption is randomized. Without a fix this would park forever as an
unresolvable conflict, since the existing resolver only understands entries
and the settings blob. `VaultResolver::resolve` now checks `is_snapshot`
first, before touching the vault at all, and keeps whichever ciphertext is
already local — correct because both sides decrypt to the same plaintext,
and needs no key, so it also resolves while the vault is locked.

Retention (the newest 20 snapshots per entry) is enforced per device, not
globally: two devices pruning the same entry's history independently can end
up holding slightly different sets of old snapshots after a sync pass. This
is cosmetic — snapshots are read-only history, never the source of truth for
an entry — not a correctness issue.
