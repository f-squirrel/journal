# 0006. Encrypt entries at rest with `age`, using per-file scrypt

## Status

Superseded by [ADR 0007](0007-cached-x25519-vault-key.md) — 2026-09-08

## Decision

Journal entries (see [0003](0003-file-per-entry-json-storage.md)) are now
encrypted at rest with the [`age`](https://docs.rs/age/0.12.1/age/) crate,
gated by a passphrase the user enters at login. The whole entry — including
the title — is encrypted, not just the markdown body.

A new `journal-vault` crate wraps `age`:

- Every `encrypt`/`decrypt` call constructs an
  `age::scrypt::Recipient`/`Identity` directly from the passphrase and does
  the scrypt work then and there. There is no key-derivation or caching
  step, and no static keypair held across calls.
- A canary file (a fixed magic string encrypted with the passphrase) at the
  vault root is how the app tells "wrong passphrase" from garbage output,
  and how it distinguishes first run (no canary → create vault) from
  returning (canary exists → verify passphrase against it).
- There's no separate salt file: `age::scrypt`'s ciphertext header already
  embeds the salt and work factor for that stanza.
- The passphrase is held in memory as `secrecy::SecretString`, not a bare
  `String`, for the lifetime of the unlocked session, giving zero-on-drop
  behavior for free when the vault is locked or the app exits.
- There is a single vault, no OS keychain integration, and no passphrase
  recovery mechanism — forgetting the passphrase means losing all entries.

`journal-storage`'s public API (`list_entries`/`get_entry`/`save_entry`/
`delete_entry`) is unchanged in shape. `list_entries` now skips (logs and
excludes) any entry file that fails to decrypt or parse, rather than
failing the whole call, so one corrupt file doesn't take down the list.

## Reasons

`age::scrypt` applies its own ~1s-by-design scrypt work factor on every
encrypt/decrypt call, which will make `list_entries` measurably slower as
entry count grows. That's a known, accepted tradeoff for now rather than
something to design around up front — see "Alternatives considered" for the
faster design that was set aside.

Using `age`'s own recipient-stanza tagging as the format marker (rather than
inventing a version byte or wrapper format) means a future move to a faster
scheme doesn't require a hard cutover: `age::Decryptor` already supports
trying a list of candidate identities against a file and using whichever
stanza matches, so `journal-vault`'s decrypt path can grow a second
candidate `Identity` type later, and old (scrypt) and new files both decrypt
through the same call — enabling incremental, lazy migration instead of a
one-time batch conversion.

The canary file avoids needing to attempt decryption of a real entry (which
may not exist yet, e.g. on first run) just to validate a passphrase, and
gives a clean, distinct "wrong passphrase" signal instead of a generic
decrypt failure.

## Alternatives considered

- **Cached static X25519 key, derived once per unlock**: scrypt once at
  unlock time → HKDF → 32-byte seed → an `age::x25519::Identity`, then use
  plain, KDF-free X25519 encryption for every entry file during that
  session. This is the right fix once per-file scrypt cost becomes a real
  problem, but it was deliberately deferred: it needs its own spike to
  confirm how to construct an `age::x25519::Identity` from raw derived
  bytes (likely via bech32-encoding into `AGE-SECRET-KEY-1...` and
  `FromStr`, not a direct public constructor), which isn't a clean,
  verified path in `age`'s public API today. The stanza-tagging decision
  above means this can be added later without breaking existing files.
- **Multiple vaults / vault switching**: not needed for a single-user
  journaling app; deferred rather than designed against.
- **OS keychain-backed unlock** (Keychain/Credential Manager/Secret
  Service): a natural follow-up once this ships, but out of scope here —
  the passphrase is never persisted in any form today.
- **Passphrase reset/recovery**: explicitly not supported; stated in the
  login UI rather than half-implemented.
