# 2. Local-first, zero-knowledge encryption with the cloud as a dumb blob store

## Status

Accepted — 2026-08-15 (ratified in the design doc §1, §3, 2026-08-14)

## Context

A journal is among the most sensitive documents a person keeps, and it needs to
be available on more than one device. The two usual answers are a hosted
service that encrypts at rest with keys it also holds, or a server-side account
system with a second factor. Both make the operator part of the trust boundary:
a subpoena, a breach, or a policy change reaches the user's entries.

We wanted the opposite property — that the storage provider is uninteresting to
attack — and we were willing to pay for it with the one thing that property
costs.

## Decision

Encrypt everything on the user's device with a key derived from a passphrase
the app never transmits. Cloud storage (Dropbox first) holds ciphertext blobs
and nothing else: no filenames, titles, tags, or timestamps in cleartext. There
is no account, no server-side component, and no key escrow.

We defend against a compromised or subpoenaed cloud provider, a network
attacker, and theft of a locked device. We explicitly do **not** defend against
a compromised OS while the vault is unlocked, an attacker who knows the
passphrase, traffic analysis, shape metadata (object count, sizes, mtimes —
which leak entry length and writing cadence), keys reaching swap or a
hibernation image, plaintext in the WebView heap, or whole-vault rollback by a
party with write access to the store.

That non-defense list is published rather than implied — in the design doc §3,
in the README, and (per the M1 plan's Task 17) in `SECURITY.md`.

## Consequences

- **Losing the passphrase means losing the data.** The only escape hatch is the
  recovery code of [ADR-0011](0011-recovery-code-format.md), which is a second
  wrapping of the same key, not an escrow — no one else holds a copy.
- Adding a sync provider is cheap, because a provider is a dumb key/value
  store ([ADR-0018](0018-sync-provider-trait.md)) rather than a party to the
  security model.
- Features that assume a server — password reset, shared journals, server-side
  search, abuse moderation — are off the table by construction, not by
  omission.
- The non-defense list is a commitment: narrowing or widening it is a change to
  the product's stated guarantee and requires updating design doc §3, not just
  code.
- Every later decision inherits one test: does a party holding the vault
  directory learn anything from it? That is what forces keyed object names
  ([ADR-0013](0013-keyed-object-names.md)) and an index that never touches disk
  ([ADR-0014](0014-in-memory-search-index.md)).
