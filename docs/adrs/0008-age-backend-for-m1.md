# 8. Ship the `age` backend at M1, deferring Argon2id (F4) to M2

## Status

Accepted — 2026-08-15 (crypto spec §1.1, §5, §10.1, second revision
2026-08-14). Supersedes nothing; the native format it postpones is fully
specified in crypto spec §6 and remains the M2 target.

## Context

The original design (design doc §5) called for a hand-written envelope format
at M1: Argon2id slot KEKs, XChaCha20-Poly1305 objects, a slot record layout, and
AEAD framing — all of it ours to write, test, and get right before a single
entry could be saved. Requirement **F4** names Argon2 explicitly.

A survey of existing containers found exactly one mature candidate. `age` has a
reviewed specification, a maintained MIT/Apache-2.0 Rust implementation, and a
recipient-stanza header that is genuinely the same idea as our planned slot
list. The alternatives surveyed (`aerovault`, `ferrocrypt`, `enc_file`,
`rust-keyvault`) are single-author and months old, and at least one is
GPL-3.0-only, which an MIT project cannot take as a dependency
([ADR-0021](0021-mit-license.md)).

`age` has three known limitations: scrypt rather than Argon2id, one scrypt
stanza per file, and no caller-supplied associated data.

## Decision

**M1 ships an `age` backend** (`backend_id = 1`), behind the seam of
[ADR-0007](0007-pluggable-sealing-backend-seam.md):

- The vault secret is an age X25519 identity; every object is an ordinary age
  file encrypted to it.
- The one-scrypt-stanza-per-file limit is solved by making the slot *list* a
  directory — `slots/passphrase.age`, `slots/recovery.age`
  ([ADR-0010](0010-luks-style-key-slots.md)).
- The missing associated data is solved by an in-plaintext binding preamble
  ([ADR-0009](0009-bind-every-object-to-its-identity.md)).
- scrypt work factors are **pinned**, not calibrated
  ([ADR-0012](0012-device-portable-kdf-parameters.md)).

**Requirement F4 is not met at M1 and is deferred to M2**, recorded as such in
design doc §2, crypto spec §1.1, and the M1 plan's "Deferred to M2" section.
This is a documented deferral, not an oversight: it must not be "fixed" by
deriving an age identity from an Argon2id output, a path considered and
rejected in crypto spec §10.1.

## Consequences

- M1's scope loses the envelope framing, the AEAD plumbing, and the slot record
  format — replaced by a reviewed, widely implemented container whose
  primitives (RustCrypto ChaCha20-Poly1305, reviewed by NCC Group) we do not
  own.
- **scrypt instead of Argon2id.** A respected memory-hard KDF and a modest
  quantitative step down — but a requirement unmet, and stated as one.
- **The binding fails open if a caller forgets to verify**, where AEAD
  associated data fails closed. The required-`Binding` API exists to make
  forgetting structurally impossible.
- **age authenticates no sender**, so the X25519 *recipient* is key material,
  not a public key — see [ADR-0009](0009-bind-every-object-to-its-identity.md).
- **Interop is partial.** A stock `age -d` opens any object but yields the
  content with 52 bytes of binary preamble in front of it; `tools/decrypt.py`
  strips it.
- The M2 migration re-seals and renames every object (crypto spec §7). M2
  builds from a ratified format instead of re-deriving one under sync
  deadlines, which is the substance of what this deferral bought.
