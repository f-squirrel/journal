# 15. Delete with manifest tombstones, reclaim blobs with garbage collection

## Status

Accepted — 2026-08-15 (design doc §6, §7; first revision 2026-08-14)

## Context

[ADR-0013](0013-keyed-object-names.md) makes objects immutable and content
addressed, and reduces sync to "copy whatever the other side is missing." Those
two properties are individually good and jointly incompatible with deletion:
if absence means "not yet copied", then deleting an entry on one device is
undone by the next sync, which faithfully restores it. A journal that
resurrects deleted entries is worse than one that cannot delete at all.

Separately, editing an entry to remove an image leaves the image blob in the
vault, referenced by nothing.

## Decision

**A delete is a record, not an absence.** Deleting an entry writes a tombstone
into the manifest; the manifest, not the object directory, is what a client
reads to know what exists. Tombstones propagate through sync like any other
manifest change.

**Unreferenced blobs are reclaimed by an explicit garbage-collection pass**,
not by deletion at the point of edit. At M1, with a single local replica, GC is
safe to run locally. Once replicas exist, a blob is only safe to drop once
every known replica has seen the tombstone that orphaned it.

Manifest writes are serialized through a single owner so two writers cannot
interleave.

## Consequences

- Deletion survives sync, and a deleted entry stays deleted.
- The manifest grows monotonically with tombstones. For a personal journal that
  is negligible; a compaction story would only be needed at implausible scale.
- **GC safety once replicas exist is an open question** (design doc §9): a
  client needs a way to establish that every replica has seen a tombstone
  before dropping the blob it orphaned. Until that exists, M2 must be
  conservative about collecting.
- A tombstone reveals that *something* was deleted and when, to anyone holding
  the vault — consistent with the shape-metadata concession of
  [ADR-0002](0002-local-first-zero-knowledge-encryption.md).
- Tests assert the observable behaviour: a deleted entry leaves listings *and*
  leaves a tombstone; GC drops unreferenced blobs and keeps referenced ones.
