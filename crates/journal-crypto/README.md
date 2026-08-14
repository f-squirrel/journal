# journal-crypto

Encryption primitives for the vault: Argon2id key derivation and
calibration, the AEAD envelope format, the key hierarchy (key slots,
recovery code), and key-material hygiene. No hand-rolled primitives —
everything here wraps audited crates.

**Status:** stub. Only a module doc comment and a compile-check test exist;
see Tasks 4–6 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md)
for the implementation checklist. The byte-level design is ratified in
[`../../docs/superpowers/specs/2026-08-14-crypto-design.md`](../../docs/superpowers/specs/2026-08-14-crypto-design.md)
and is normative for everything in this crate.

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
- `slots.rs` — the LUKS-style key-slot list (`{type, kdf_params, slot_id,
  salt, wrap_nonce, wrapped_DEK}`, crypto spec §4) for passphrase, recovery
  code, and (M3+) device key.

## Non-negotiable conventions

- No hand-rolled crypto. Only `argon2`, `chacha20poly1305`, `hkdf`, `hmac`,
  `sha2`, `zeroize`, `secrecy`, `getrandom`, `subtle`, and `base32`
  (`Alphabet::Crockford`, for recovery codes and object names).
- Key types must implement neither `Serialize` nor `Debug` (enforced by a
  `trybuild` compile-fail test) and must zeroize on drop.
- A vault whose header parameters fall below the documented Argon2id floor
  is rejected, not silently accepted.
- Formats are not chosen at the call site. Envelope framing, associated
  data, header layout, KDF bounds, and object naming come from the crypto
  spec; changing one is a format-version change.
- Full threat model and key hierarchy: §5 of
  [`../../docs/superpowers/specs/2026-08-14-journaling-app-design.md`](../../docs/superpowers/specs/2026-08-14-journaling-app-design.md),
  specified byte for byte in
  [`../../docs/superpowers/specs/2026-08-14-crypto-design.md`](../../docs/superpowers/specs/2026-08-14-crypto-design.md).

## Used by

`journal-store`, for encrypting/decrypting vault objects.
