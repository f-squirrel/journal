# 7. A pluggable sealing backend behind `KeyStore` / `ObjectSealer`

## Status

Accepted — 2026-08-15 (ratified in the crypto spec §3, 2026-08-14; implemented
in `crates/journal-crypto/src/backend.rs`, M1 Task 4)

## Context

[ADR-0008](0008-age-backend-for-m1.md) adopts `age` for M1 to save
implementation time, with a native Argon2id + XChaCha20 format arriving in M2.
Two formats in the life of one product raises the obvious risk: the second one
becomes a rewrite, and the security properties of the first live implicitly in
whichever code happens to be installed.

There is a subtler risk too. The age container carries no associated data, so
the object-identity binding that AEAD gives for free must be enforced by our
own code — and a check a caller can skip is a check that will eventually be
skipped.

## Decision

Split the design into a **backend-independent core** and a **pluggable sealing
backend**, joined by two traits whose signatures are normative:

```rust
pub trait ObjectSealer {
    fn seal(&self, binding: &Binding, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>;
    fn open(&self, binding: &Binding, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError>;
}

pub trait KeyStore: Sized { /* create, unlock, change_passphrase, sealer, name_key */ }
```

Above the seam, identical under every backend: directory layout, the 44-byte
header, object kinds and IDs, the object-naming algorithm, the recovery code,
the manifest, tombstones. Below the seam: how the vault secret is wrapped by a
user secret, how a sealed object is framed, and which KDF and AEAD are used.

`Binding` is a **required argument** of both `seal` and `open`, with no
overload without it — see [ADR-0009](0009-bind-every-object-to-its-identity.md).
`change_passphrase` takes no vault root, so rewrapping into a *different*
vault's slot directory is not expressible.

## Consequences

- The security contract a backend must meet is written down once, as a type,
  instead of being rediscovered per backend. The crypto spec calls this the
  most important line in the document, and it is what keeps "we used age to
  save time" from also meaning "we quietly dropped a defence".
- **The seam saves design, not I/O.** `K_name` derives from the vault secret,
  which lives *below* the seam, so the two backends produce different names for
  the same content. The M2 migration re-seals and renames every object
  ([ADR-0016](0016-versioned-formats-fail-closed.md) and crypto spec §7) — it is
  not a slot swap, and an earlier draft that implied otherwise was corrected.
- A third backend (or a hardware-backed one) is a contained piece of work.
- Callers pay a small ergonomic tax: you cannot open an object without stating
  which object you believe it is. That tax is the feature.
