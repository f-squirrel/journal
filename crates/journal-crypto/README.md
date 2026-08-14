# journal-crypto

Encryption primitives for the vault: Argon2id key derivation and
calibration, the AEAD envelope format, the key hierarchy (key slots,
recovery code), and key-material hygiene. No hand-rolled primitives —
everything here wraps audited crates.

**Status:** stub. Only a module doc comment and a compile-check test exist;
see Tasks 4–6 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md)
and the ratified crypto spec (Task 2, not yet written) for the byte-level
design.

## Planned contents

- `kdf.rs` — Argon2id over vault-header parameters, plus device calibration
  clamped between a documented floor and the weakest-supported-device
  ceiling.
- `aead.rs` — XChaCha20-Poly1305 seal/open over a versioned envelope, with
  object ID and format version bound as associated data.
- `derive.rs` — HKDF for per-object keys and the object-naming key
  (`K_name`).
- `key.rs` — key types wrapped in `zeroize`/`secrecy`; never `Serialize` or
  `Debug`.
- `slots.rs` — the LUKS-style key-slot list (`{type, kdf_params, salt,
  wrapped_DEK}`) for passphrase, recovery code, and (M3+) device key.

## Non-negotiable conventions

- No hand-rolled crypto. Only `argon2`, `chacha20poly1305`, `hkdf`,
  `zeroize`, `secrecy`, `getrandom`.
- Key types must implement neither `Serialize` nor `Debug` (enforced by a
  `trybuild` compile-fail test) and must zeroize on drop.
- A vault whose header parameters fall below the documented Argon2id floor
  is rejected, not silently accepted.
- Full threat model and key hierarchy: §5 of
  [`../../docs/superpowers/specs/2026-08-14-journaling-app-design.md`](../../docs/superpowers/specs/2026-08-14-journaling-app-design.md).

## Used by

`journal-store`, for encrypting/decrypting vault objects.
