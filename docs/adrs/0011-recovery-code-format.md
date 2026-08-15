# 11. A 160-bit app-generated recovery code in Crockford Base32

## Status

Accepted — 2026-08-15 (crypto spec §4.5, 2026-08-14; alphabet choice confirmed
with the user during M1 Task 2 review, diverging from the plan's original
wording)

## Context

[ADR-0002](0002-local-first-zero-knowledge-encryption.md) means a forgotten
passphrase is unrecoverable. That is the correct trade for a zero-knowledge
app, but shipping it with no second door at all would make ordinary human
forgetfulness catastrophic.

The second door must not become a weaker first door: anything user-chosen,
anything derived from the passphrase, and anything stored by us would each
undo the property we are protecting.

## Decision

At vault creation the app generates a **160-bit** recovery code from the OS
CSPRNG — never user-chosen, never derived from the passphrase, shown exactly
once — and wraps the vault secret with it in its own key slot
([ADR-0010](0010-luks-style-key-slots.md)).

Encoding is **Crockford Base32**: 160 / 5 = exactly 32 characters, no padding
and no leftover bits, displayed as 8 groups of 4:

```
K7M2-9XQ4-BVN3-8HTP-Z5RW-6JYC-D2GK-4NFB
```

The alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ` omits `I`, `L`, `O` and `U`, so
a code never contains both members of a confusable pair, and a reader who
transcribes `O` for `0` or `l` for `1` produces a string the decoder maps back
to the right value. `U` is excluded so codes cannot spell obscenities. Parsing
strips hyphens and whitespace, requires exactly 32 characters, and defers case
folding and confusable mapping to the `base32` crate — not to a hand-written
normalizer. The **decoded 20 bytes** are what the backend consumes, so
formatting can never affect the derived key.

There is **no check character**.

## Consequences

- A user who keeps the printed code survives a forgotten passphrase; nobody
  else gains anything, because this is a second wrapping of the same secret and
  **not an escrow** — a vault whose passphrase and recovery code are both lost
  is gone.
- At 160 bits the KDF pass over the code is defence in depth, not the security
  boundary. Brute-forcing the code is not a threat worth modelling.
- No check character means a typo costs one KDF run and reports the same
  generic failure as a wrong passphrase. Accepted deliberately: a check symbol
  would widen the alphabet and add another thing for the independent decryptor
  to reproduce, to save a second on a path most users walk once or never.
- "Crockford Base32" is a citable standard, so an independent implementation
  reaches for its own library rather than reimplementing our prose.
- The UI must show the code once, require confirmation that it was written
  down, and never persist it (M1 Task 14).
