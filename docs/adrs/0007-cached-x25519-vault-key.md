# 0007. Derive a cached X25519 key at unlock instead of scrypt per file

## Status

Accepted — 2026-09-08

Supersedes [0006](0006-age-scrypt-vault-encryption.md).

## Decision

`journal-vault` keeps a long-lived `age::x25519` keypair as the vault's real
encryption key. The passphrase protects only that key, not the entries:

- **Vault creation** generates an identity with
  `age::x25519::Identity::generate()`, seals its serialized form
  (`identity.to_string()`, an `AGE-SECRET-KEY-1…` `SecretString`) under the
  passphrase using `age`'s scrypt recipient, and writes the result to a key
  file at the vault root. That file replaces the `.vault-canary` of ADR
  0006 and serves the same two purposes: it distinguishes first run from
  returning, and a wrong passphrase is detected by its failure to decrypt.
- **Unlock** performs exactly one scrypt operation — decrypting the key
  file — then parses the plaintext back with
  `age::x25519::Identity::from_str`. The identity and its `to_public()`
  recipient are held for the session in place of the passphrase.
- **Every entry** is encrypted to that X25519 recipient and decrypted with
  that identity. No scrypt, no passphrase, no per-file key derivation.

Both sides of the key file's scrypt work factor are constants, not
benchmarks. The `Recipient` calls `set_work_factor` with a pinned
`KEY_FILE_WORK_FACTOR` (18, what the Go implementation of age writes
unconditionally, ~0.4s on a desktop) instead of letting `age` time the
writing machine; the `Identity` calls `set_max_work_factor` with a
deliberate `KEY_FILE_MAX_WORK_FACTOR` (20) instead of inheriting `age`'s
"this machine's benchmark, plus 4".

Pinning the *write* side was not in the first draft of this decision, and
is what makes the design safe: leaving it benchmarked would reproduce the
cross-device lockout below on the one file that matters most, where —
unlike an entry — there is no second copy of the key to fall back on. That
the pinned factor stays within the cap is a compile-time assertion.

The passphrase is still never written to disk, and `secrecy::SecretString`
still holds it (and now the serialized key) in memory.

## Reasons

ADR 0006 accepted per-file scrypt as a cost — "measurably slower as entry
count grows" — and deferred this design. Two things it did not account for
turned that into a correctness problem rather than a performance one.

**The work factor is machine-calibrated and stored per file.** `age`'s
`target_scrypt_work_factor()` benchmarks the current machine on every
`Recipient::new`/`Identity::new` to land near one second, and writes the
chosen `log_n` into each file's header; `Identity::new` accepts at most
`target + 4` on the way back in. An entry written on a fast desktop
therefore fails to decrypt on a slower phone. Because `unlock_vault` mapped
every decrypt error to `WrongPassphrase`, the user was told their
passphrase was wrong — permanently, on a device where it was correct. For a
project targeting desktop, iOS and Android with sync planned, that is
permanent data inaccessibility, not a slow list.

**The work factor is attacker-controlled on the way in.** `log_n` is read
from the file header, and `age`'s own documentation warns that large values
allow "attempted decryption of a malicious file that takes hours and tens
of GiB of RAM". `list_entries` reads every `*.json` file in the vault
directory, which becomes semi-untrusted input once sync lands. Capping the
accepted work factor explicitly is the fix, and it is far easier to reason
about when exactly one file in the vault carries an scrypt stanza at all.

**The cost was also worse than "slower".** Measured in release, scrypt runs
1.1–1.5s per operation while X25519 runs ~0.2ms — roughly 5000× per entry.
At `log_n ≈ 18` scrypt also needs `128·r·N` ≈ 256 MiB *per concurrent
operation*, so the obvious mitigation of decrypting the list in parallel
costs gigabytes of resident memory and is not viable on mobile.

**ADR 0006's stated reason for deferring does not hold.** It expected this
design to need "its own spike to confirm how to construct an
`age::x25519::Identity` from raw derived bytes (likely via bech32-encoding
… not a clean, verified path in `age`'s public API today)". No raw-bytes
construction is required: generating an identity and storing its own
serialized string uses only `generate`, `to_public`, `to_string` and
`FromStr`, all public API, with no HKDF and no hand-rolled bech32. The
approach was verified end to end against `age` 0.12.1 before this ADR.

**There is nothing to migrate.** A mismatch between the Rust and TypeScript
field names meant `save_entry` had never succeeded across the Tauri bridge,
so no build has ever written an entry file. The on-disk format has an
installed base of zero, which makes this the cheapest possible moment to
change it and removes the incremental-migration concern ADR 0006 designed
for.

*(Overtaken between this ADR and the implementation: fixing that field-name
mismatch is what made saves start working, so by the time this landed the
dev machine did hold real encrypted entries. They were confirmed
disposable and deleted. The conclusion stands — no migration — but the
premise is now "the installed base was one developer's scratch entry",
not zero.)*

## Alternatives considered

- **Keep per-file scrypt and pin a fixed `log_n`.** Setting an explicit
  work factor on both `Recipient` and `Identity` would fix the cross-device
  lockout without changing the scheme. It leaves the ~1s-per-entry cost and
  the 256 MiB-per-operation memory profile untouched, so `list_entries`
  stays linear in seconds and cannot be parallelised on mobile. Rejected:
  it fixes the smaller half of the problem and locks in the larger one.
- **Bounded-parallel scrypt.** Decrypting 2–3 entries at a time caps memory
  around 0.5–0.75 GB and cuts wall time ~3×. Rejected: still seconds for a
  modest journal, still a mobile memory risk, and it adds a concurrency
  budget to tune forever in exchange for a constant factor.
- **Derive the X25519 key from the passphrase via HKDF** rather than
  generating it randomly and storing it. This keeps the passphrase as the
  sole recovery secret — losing the key file would not matter. Rejected:
  it needs the raw-bytes `Identity` construction that ADR 0006 was right to
  call unclear in `age`'s public API, and it makes changing the passphrase
  a full re-encryption of every entry, where storing the key makes it a
  rewrite of one small file.
- **Keep the canary and add a separate key file.** Two files, two scrypt
  operations at unlock, and two ways to be half-restored. Rejected: the
  sealed key file already fails to decrypt under a wrong passphrase, which
  is the entire job the canary was doing.

## Consequences

The key file holds the only copy of the vault key. Losing it means the
entries are unrecoverable **even with the correct passphrase** — under ADR
0006 the passphrase alone was sufficient. This is the standard trade for a
cached-key design, and it is accepted here, but it means:

- `unlock` must refuse to create a new vault when the key file is absent
  and entry files are present, rather than silently starting a fresh vault
  and orphaning them.
- The login UI's existing "no way to recover this passphrase" warning
  should also say that the vault directory must be backed up as a whole.

Changing the passphrase becomes cheap (re-seal one file) and is now a
plausible future feature, where under ADR 0006 it would have meant
re-encrypting every entry.

The attacker-controlled-work problem has no test behind it, and the
explicit `KEY_FILE_MAX_WORK_FACTOR` is its whole mitigation. Proving it
would mean either paying the absurd cost the cap exists to refuse, or
forging a header whose MAC is checked only *after* that cost is paid.
Stated here rather than covered by a test that pretends otherwise.

Unoptimized scrypt is roughly 40x slower than optimized, which would make
a `cargo test` or `make run` unlock take ~17s at the pinned factor. The
root `Cargo.toml` therefore raises `opt-level` for the KDF's crates in the
dev profile. Calibrating the constant against release timings is only
honest if development pays a comparable cost.
