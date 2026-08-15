# 16. Explicit versioning, fail-closed parsing, and a reserved rollback counter

## Status

Accepted — 2026-08-15 (design doc §5; crypto spec §4.2, §5.6, §8, 2026-08-14)

## Context

This app will exist in several versions at once — an old desktop build, a new
one, and eventually phones — all pointed at the same vault, possibly through a
sync provider that is not trusted. Two failure modes matter more than the rest:
a newer client's vault being half-understood and then *written to* by an older
client, and a parser that repairs damaged input into something plausible.

There is also a threat the per-object binding of
[ADR-0009](0009-bind-every-object-to-its-identity.md) explicitly does not
cover: an attacker with write access serving an older but internally consistent
snapshot of the *whole* vault.

## Decision

**Versioning.** The 44-byte header carries `format_major`, `format_minor` and
`backend_id`. A client that meets an unknown `format_major` **refuses to open
the vault and never writes a byte to it**. A higher `format_minor` than
understood MAY be opened **read-only**. An unknown `backend_id` is refused — a
client that does not implement a backend does not guess. On-disk types
serialize explicitly and versioned: no `#[serde(flatten)]`, no silent
acceptance of unknown fields, golden-file tests so an accidental format change
fails loudly instead of migrating silently.

**Fail-closed parsing.** Every parse rule rejects rather than repairs. There is
no best-effort open, no field the reader guesses at, and no path that writes to
a vault it could not fully validate. An age file that fails to parse or decrypt
is rejected for that object — never retried under another key or backend.

**Header integrity comes from the objects, not the header.** `format_major`,
`format_minor` and `vault_id` appear in every object's `Binding`, so tampering
with them makes every object fail to open. `header_checksum` is unkeyed and is
a **corruption detector only** — it exists so a bit-rotted header reports "this
vault's header is corrupt" instead of "wrong passphrase", which is the
difference between a user restoring a backup and a user believing they forgot
their passphrase.

**`rollback_counter` is reserved at M1.** Writers write 0; readers ignore the
value and MUST NOT reject a non-zero one — but an M1 client that reads a
non-zero counter **opens the vault read-only** and tells the user the vault
appears to have been written by a newer client. The mechanism ships with sync
in M2.

## Consequences

- No half-migrated vaults, and no old client silently corrupting a new format.
- Whole-vault rollback stays a **stated non-defense at M1**. The header field
  is reserved now because adding it later would be a format-version change.
- The read-only latch is **attacker-triggerable and accepted as such**:
  `rollback_counter` is in no binding and the checksum is unkeyed, so anyone
  holding the vault can freeze M1 clients into read-only. That is a denial of
  service, the same class as tampering with `backend_id`, and no unkeyed header
  field can prevent it. What it must not do is fail *silently* — a client that
  latches read-only must say so rather than presenting a vault whose saves
  quietly do nothing.
- Header tampering surfaces at the first object read, as a corrupt vault rather
  than a tamper alarm. That trade is deliberate and is what removes the need
  for a header MAC (whose key the reader would not have until after unlocking a
  slot).
- Users occasionally meet a hard refusal instead of a degraded open. That is
  the intended behaviour, and error messages must explain it.
