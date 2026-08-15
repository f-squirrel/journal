# 6. Specify the format first, then prove it with an independent decryptor

## Status

Accepted — 2026-08-15 (ratified in the crypto spec §9, 2026-08-14; M1 Tasks 2
and 16)

## Context

The vault format is the most expensive thing in this project to get wrong.
Once a user has entries on disk, a format mistake is a migration for every
existing vault, and a *security* mistake in the format may not be detectable at
all from the outside. Meanwhile the app's central promise — "your data is
retrievable without this app" (F7) — is worth nothing if the only description
of the format is the Rust source that produced it.

Writing the format down after implementing it tends to produce a document that
describes the code's accidents as if they were choices.

## Decision

The cryptographic format is specified **before** it is implemented, in
[`docs/superpowers/specs/2026-08-14-crypto-design.md`](../superpowers/specs/2026-08-14-crypto-design.md),
at a level of detail sufficient to implement it twice independently: RFC 2119
keywords, zero-based byte offsets, little-endian integers, fixed-width fields.
That document is normative — implementations follow it rather than re-deriving
a format at the call site.

Two mechanisms keep the document honest:

- **Golden vectors** checked into `crates/journal-crypto/tests/vectors/`,
  covering the recovery code (including sloppy transcriptions), object naming
  under *both* backends, the 52-byte binding preamble, and a complete
  age-backend vault openable end to end — written at the real pinned work
  factor, not a cheaper one.
- **`tools/decrypt.py`**, an independent decryptor written from the spec alone,
  never from the Rust source (M1 Task 16). If it cannot open a vault the app
  wrote, the spec is wrong or incomplete. It doubles as the user's escape
  hatch.

## Consequences

- A spec bug surfaces as a failing test rather than as an unopenable vault
  years later, and the acceptance test for the *specification* is a program,
  not a review.
- The independent decryptor is a real deliverable with real cost, including a
  rule that its author must not read the Rust — which has to be respected on
  the honor system.
- Golden vector 4 costs the test suite roughly a second of CPU and 128 MiB per
  open, twice. That is the price of the vector exercising the parameters the
  app actually ships.
- Changing framing, binding, header layout, KDF parameters, or object naming is
  a **format-version change**, not a refactor — the spec, the vectors, and the
  decryptor all move together.
- The format is documented well enough that a hostile fork, an auditor, or a
  future maintainer can re-implement it without this codebase. That is the
  point of F7.
