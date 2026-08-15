# 9. Bind every sealed object to its identity, and treat the age recipient as key material

## Status

Accepted — 2026-08-15 (crypto spec §3.2, §5.4, §6.5, §8, third revision
2026-08-14)

## Context

The threat model ([ADR-0002](0002-local-first-zero-knowledge-encryption.md))
assumes a storage provider that may be hostile. Ciphertext alone does not stop
such a provider from *rearranging* a vault: serving object A's bytes under
object B's name, restoring an older version of one entry, transplanting an
object from another vault, or claiming an older format version whose parsing
rules were weaker. None of that requires breaking any cipher.

The native M2 backend gets the answer for free from AEAD associated data. The
M1 age backend does not: age fixes `aad = ""`.

## Decision

Every sealed object is bound to a `Binding` — `{format_major, format_minor,
vault_id, kind, object_id}` — by whatever mechanism the backend has, and
`ObjectSealer::open` **MUST NOT return plaintext before the binding verifies**.

- **Native backend (M2):** `Binding` becomes the 60-byte `ObjectAD` passed as
  XChaCha20-Poly1305 associated data. `K_obj` is additionally derived from the
  object ID, so an implementation that gets the derivation right and the AAD
  wrong still fails closed.
- **age backend (M1):** `Binding` becomes a **52-byte preamble prepended to the
  plaintext**. On open, the sealer rejects output shorter than 52 bytes,
  rebuilds the expected preamble from the caller's `Binding`, compares it in
  constant time (`subtle`), and on mismatch returns `CryptoError::BindingMismatch`
  and zeroizes the buffer.

Because `Binding` is a required parameter of the only available entry point
([ADR-0007](0007-pluggable-sealing-backend-seam.md)), a caller cannot open an
object without stating which object they believe it is.

**Under the age backend the X25519 recipient is key material.** age
authenticates no sender, so anyone holding `identity.to_public()` can mint a
well-formed object with a valid preamble under any `Binding`. The recipient
therefore MUST never reach disk, logs, IPC, an export path, or a future
sync/sharing feature. It is a public key by type only.

## Consequences

- Object substitution, per-object rollback, cross-vault transplantation, and
  format downgrade all fail closed under both backends. **Whole-vault rollback
  is not covered** — see [ADR-0016](0016-versioned-formats-fail-closed.md).
- The M1 defence rests on an invariant this project imposes (the recipient
  never leaves the process), not on a property of age. That is a strictly
  weaker position than M2's symmetric DEK, where forging requires the same
  secret that reads — and it is one of the concrete reasons M2 is worth doing.
- The preamble costs 52 bytes per object and makes stock-`age` output messy
  ([ADR-0008](0008-age-backend-for-m1.md)).
- Object naming hashes the object's **content**, never `preamble || content` —
  the preamble contains the name tag, so hashing it would be circular. This was
  a real defect in the second revision of the spec, corrected in the third.
- "Add an `open()` without a binding" is permanently off the table, including
  for tests and tooling.
