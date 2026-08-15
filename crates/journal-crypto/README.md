# journal-crypto

Encryption for the vault: the backend seam, key slots, object sealing, object
naming, and recovery codes. No hand-rolled primitives — everything here wraps
audited crates, and at M1 the container itself is `age`.

**Status:** the backend seam (`backend.rs`) and object naming (`naming.rs`)
exist; the age backend and the recovery code do not yet. See Tasks 5–6 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md)
for the implementation checklist. The byte-level design is ratified in
[`../../docs/superpowers/specs/2026-08-14-crypto-design.md`](../../docs/superpowers/specs/2026-08-14-crypto-design.md)
and is normative for everything in this crate.

## Structure

The crate is split the way the spec is — a backend-independent core over a
pluggable sealing backend:

```
backend.rs      the seam: KeyStore, ObjectSealer, Binding, CryptoError
naming.rs       K_name derivation and HMAC object names
key.rs          key types: zeroize/secrecy, no Debug       (planned)
age_backend/    M1: age files, scrypt slots                (planned)
native/         M2: Argon2id + XChaCha20 envelope          (planned)
tests/vectors/  golden vectors from the spec's §9 list
```

`age_backend` and `native` are interchangeable behind `backend.rs`. Everything
above the seam — the vault layout, the header, object naming, the manifest,
the recovery code — is identical under both, which is what keeps the M2
migration a slot migration rather than a rewrite.

## M1 uses age, and that has a cost

M1 seals objects with [`age`](https://github.com/C2SP/C2SP/blob/main/age.md)
to save implementation time. Two consequences are deliberate and documented,
not bugs:

- **No Argon2id at M1.** age's passphrase recipient is scrypt-only, so
  requirement F4 is deferred to M2. Don't "fix" this by deriving an age
  identity from an Argon2id output — that path was considered and rejected
  (spec §10.1).
- **Binding lives in the plaintext.** age fixes `aad = ""`, so the age backend
  prepends a 52-byte binding preamble and verifies it on open (spec §5.4).
- **The X25519 recipient is secret.** age authenticates no sender, so anyone
  holding `identity.to_public()` can forge a well-formed object under any
  `Binding`. It is a public key by type only — never persist, log, export, or
  send it across IPC (spec §8). The M2 native backend's symmetric DEK drops
  this rule.

## Non-negotiable conventions

- No hand-rolled crypto, and no hand-rolled container or encoding. Only `age`,
  `base32`, `hkdf`, `hmac`, `sha2`, `zeroize`, `secrecy`, `getrandom`,
  `subtle`, plus `argon2` and `chacha20poly1305` when the native backend
  lands.
- Key types must implement neither `Serialize` nor `Debug` (enforced by a
  `trybuild` compile-fail test) and must zeroize on drop.
- `seal` and `open` always take a `Binding`. There is no binding-free path,
  and `open` never returns plaintext before the binding verifies.
- scrypt work factors are **pinned**, never left to age's per-device
  calibration — age rejects files whose work factor exceeds the decrypting
  device's target by more than 4, which would make a desktop-created vault
  unopenable on a phone.
- Unlock failures are indistinguishable from one another and write nothing to
  disk.
- Formats are not chosen at the call site. Framing, binding, header layout,
  KDF parameters, and object naming come from the spec; changing one is a
  format-version change.
- `K_name` is derived **per backend** (spec §4.4): full HKDF over the age
  identity string, HKDF-Expand-only over the native DEK. They are not one
  construction, and `tests/vectors/naming.json` pins both rows.
- Object names are keyed tags, never a bare hash of the content — a raw
  content hash is a confirmation oracle (ADR-0013).
- Full threat model and key hierarchy: §5 of
  [`../../docs/superpowers/specs/2026-08-14-journaling-app-design.md`](../../docs/superpowers/specs/2026-08-14-journaling-app-design.md),
  specified byte for byte in
  [`../../docs/superpowers/specs/2026-08-14-crypto-design.md`](../../docs/superpowers/specs/2026-08-14-crypto-design.md).

## Used by

`journal-store`, for sealing/opening vault objects and naming them.
