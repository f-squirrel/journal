# 21. Ship free and open source under MIT

## Status

Accepted — 2026-08-15 (design doc §1; `LICENSE`, `Cargo.toml`)

## Context

An app that asks users to trust it with their journal, while insisting that
nobody — including its authors — can read that journal, has to be auditable.
A closed-source zero-knowledge claim is a promise; an open-source one is a
checkable statement. The question was which license, and the two candidates
were a permissive one and a copyleft one.

## Decision

MIT, for the app and every crate in the workspace (`license = "MIT"` in
`[workspace.package]`, full text in `LICENSE`).

The direct consequence is a dependency constraint: **this project cannot take a
GPL-3.0-only crate as a dependency**, regardless of merit. `cargo deny check`
enforces the allowed license set in CI
([ADR-0020](0020-toolchain-lints-and-supply-chain.md)).

## Consequences

- The security claims of [ADR-0002](0002-local-first-zero-knowledge-encryption.md)
  are verifiable by anyone, which is the only way they mean anything.
- Combined with the format spec of
  [ADR-0006](0006-spec-first-crypto-with-an-independent-decryptor.md), a user's
  data outlives this project even if the project is abandoned: the format is
  documented and the code is forkable.
- The crate survey behind [ADR-0008](0008-age-backend-for-m1.md) ruled out at
  least one candidate on license grounds alone before merit was considered.
  That will happen again.
- A permissive license permits a closed-source fork. Accepted: the alternative
  buys less than it costs for an app whose value is in being trusted, not in
  being exclusive.
