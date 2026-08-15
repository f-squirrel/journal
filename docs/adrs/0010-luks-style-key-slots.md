# 10. Unwrap the vault secret through a list of key slots

## Status

Accepted — 2026-08-15 (design doc §5; crypto spec §5.2, §6.2, 2026-08-14)

## Context

The vault has to be openable by a passphrase from day one and by a recovery
code from day one ([ADR-0011](0011-recovery-code-format.md)), and by an OS
keychain/keystore-held device key from M3. A format with a single wrapped key
makes the third of those a format migration — the expensive mistake this
project is specifically trying to avoid, since by M3 there are real vaults on
real disks.

## Decision

Follow LUKS: the vault secret is wrapped independently by **each** unlock
method, one slot per method, all wrapping the same secret.

- **Native backend (M2):** the header carries 1–8 fixed-width slot records,
  each `{slot_type, kdf_id, lanes, mem_kib, iterations, slot_id, salt,
  wrap_nonce, wrapped_dek}`. Slots bind to their random `slot_id`, **not** to
  their position, so removing or reordering a slot cannot invalidate another.
  A reader that meets an unknown `slot_type` ignores that slot rather than
  rejecting the header — that is what makes M3's device slot additive — and
  MUST NOT write to such a header.
- **age backend (M1):** age's specification requires that an scrypt stanza be
  the only stanza in its file, so a single age file cannot hold two slots. The
  slot list is therefore a **directory**: `slots/passphrase.age` and
  `slots/recovery.age`, each an age file whose plaintext is the same vault
  identity string. M3's device slot is `device.age`.

Unlock tries **only** the slot matching the attempt type — a passphrase is
never run against the recovery slot — and returns a single generic failure that
reveals nothing about which slot was tried, how many exist, or how far the
attempt got. Nothing is written to disk on a failed unlock: no counter, no
timestamp. The KDF cost is the rate limit.

Slot writes are crash-atomic (temp file in the same directory, fsync, rename,
fsync the directory), and vault creation writes `header.jvh` **last**.

## Consequences

- Adding an unlock method is a new slot (or a new file), never a format
  migration.
- **Changing the passphrase rewraps one slot and touches no object.** The
  recovery code keeps working, and after a passphrase change every object file
  is byte-identical — an assertion the tests make. Rotating the vault secret
  itself is the expensive operation, because it re-derives every per-object key
  and every object name.
- Under age, the "slot list" is a directory listing, so the number of unlock
  methods configured is visible to anyone holding the vault. Consistent with
  the shape-metadata concession of
  [ADR-0002](0002-local-first-zero-knowledge-encryption.md).
- The crash-atomicity rule is not incidental: a torn `passphrase.age` costs the
  user their passphrase and leaves the recovery code as the only way back in.
  It was missing from the second revision of the spec and added in the third.
