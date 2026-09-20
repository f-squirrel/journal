# 0014. Search by decrypting on demand, with no persistent index

## Status

Accepted — 2026-09-20

## Decision

`Vault::search` answers a query by decrypting every entry blob, matching
case-insensitive substrings against the title and the markdown, and returning
a `SearchMatch` per hit. It keeps no index — not on disk, and not cached in
memory between calls. The frontend debounces the `search_entries` command by
200 ms; nothing else stands between the query and the journal.

## Reasons

Searching has to read the entries. `list_entries` returns headers without
`markdown` on purpose (ADR 0003, one encrypted blob per entry), so a filter
over what the frontend already holds could only ever match titles — and a
journal search that cannot match what an entry *says* is not the feature.

Given that, the question was only whether to keep what it decrypts. It is not
worth it: over 1,000 entries of ~1 kB, a release build spends ~100 ms on a
search and ~93 ms on `list_entries`, because both decrypt every blob. The app
already runs that listing on every unlock and after every sync pass. An index
would save a cost that is already being paid on a schedule nobody complains
about, in exchange for invalidating it correctly on four separate events:
every save, every delete, every lock, and every entry the sync engine pulls in
— and that last one is invisible from inside the vault, because the engine
writes through the bare `FsStore` *underneath* the `NotifyingStore` the vault
holds (ADR 0009, ADR 0012). A stale search result is a silent wrong answer
about the user's own journal.

The ~1 s of scrypt that makes unlocking slow is spent once, on the key blob
alone (ADR 0007); entries are sealed to the X25519 key and cost
sub-milliseconds each. Without that, none of the above would hold.

## Alternatives considered

**A cached in-memory index, built on first search and dropped on lock.** The
original plan for this feature. Rejected on the measurement above: it saves
~100 ms per query and buys four invalidation paths, one of which cannot be
observed from the layer that would own the cache.

**A persistent index (SQLite or similar).** ADR 0009 deferred this
deliberately and nothing here reopens it. An index on disk is a second copy of
the plaintext, so it must be encrypted, invalidated, and taught to the sync
engine as a blob that must never be merged — three new problems, on no
evidence that the app needs any of them.

**Filtering the headers in the frontend.** Cheapest by far, and it matches
titles only. Ruled out above.

This decision is worth revisiting when there is a journal large enough for a
search to be felt — a few thousand entries, where the cost approaches a
second. The place to look is `Vault::search`, which records these numbers.
