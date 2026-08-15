# Architecture Decision Records

This folder records the architectural decisions behind this project — what was
decided, why, what was rejected, and what the decision costs.

Format: [adr.github.io](https://adr.github.io/) (Michael Nygard's template) —
each record has **Title, Status, Context, Decision, Consequences**. See
[ADR-0001](0001-record-architecture-decisions.md) for the conventions.

**ADRs are not normative.** They record decisions and reasoning; the
implementable truth lives in the specs. Where an ADR and a spec disagree about
a byte layout, a KDF parameter, or an object name, the **spec wins** and the
ADR should be superseded to match:

- [`docs/superpowers/specs/2026-08-14-journaling-app-design.md`](../superpowers/specs/2026-08-14-journaling-app-design.md)
  — product, threat model, architecture
- [`docs/superpowers/specs/2026-08-14-crypto-design.md`](../superpowers/specs/2026-08-14-crypto-design.md)
  — the cryptographic format, byte for byte (normative)
- [`docs/superpowers/plans/INDEX.md`](../superpowers/plans/INDEX.md)
  — what is actually implemented vs. planned

## Index

| # | Decision | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-local-first-zero-knowledge-encryption.md) | Local-first, zero-knowledge encryption with the cloud as a dumb blob store | Accepted |
| [0003](0003-rust-core-in-a-cargo-workspace.md) | Put the core in Rust, split across a Cargo workspace | Accepted |
| [0004](0004-tauri-2-and-react-for-the-ui.md) | Tauri 2 with a React/TypeScript UI | Accepted |
| [0005](0005-ipc-boundary-and-lock-teardown.md) | Keys never cross the IPC boundary, and locking is a teardown | Accepted |
| [0006](0006-spec-first-crypto-with-an-independent-decryptor.md) | Specify the format first, then prove it with an independent decryptor | Accepted |
| [0007](0007-pluggable-sealing-backend-seam.md) | A pluggable sealing backend behind `KeyStore` / `ObjectSealer` | Accepted |
| [0008](0008-age-backend-for-m1.md) | Ship the `age` backend at M1, deferring Argon2id (F4) to M2 | Accepted |
| [0009](0009-bind-every-object-to-its-identity.md) | Bind every sealed object to its identity; the age recipient is key material | Accepted |
| [0010](0010-luks-style-key-slots.md) | Unwrap the vault secret through a list of key slots | Accepted |
| [0011](0011-recovery-code-format.md) | A 160-bit app-generated recovery code in Crockford Base32 | Accepted |
| [0012](0012-device-portable-kdf-parameters.md) | KDF parameters are bounded by the weakest device, not the strongest | Accepted |
| [0013](0013-keyed-object-names.md) | Name objects with a keyed HMAC tag, not a raw content hash | Accepted |
| [0014](0014-in-memory-search-index.md) | The search index is derived, disposable, and never written to disk | Accepted |
| [0015](0015-tombstones-and-blob-gc.md) | Delete with manifest tombstones, reclaim blobs with garbage collection | Accepted |
| [0016](0016-versioned-formats-fail-closed.md) | Explicit versioning, fail-closed parsing, and a reserved rollback counter | Accepted |
| [0017](0017-plaintext-export-and-import-in-m1.md) | Ship plaintext export and import in M1 | Accepted |
| [0018](0018-sync-provider-trait.md) | Sync through a four-method provider trait, Dropbox first | Accepted (M2) |
| [0019](0019-milestone-sequencing.md) | Prove crypto and storage before adding sync and mobile | Accepted |
| [0020](0020-toolchain-lints-and-supply-chain.md) | Pin the toolchain, deny warnings in CI, and audit dependencies | Accepted |
| [0021](0021-mit-license.md) | Ship free and open source under MIT | Accepted |
| [0022](0022-spec-and-plan-driven-workflow.md) | Plan work as specs and checklists, commit in Conventional Commits | Accepted |

Records 0002–0022 were written retrospectively on 2026-08-15 from decisions
already ratified in the specs and the M1 plan; each cites its source.

## Adding a record

1. Copy the structure of an existing record: `# N. Title`, then `## Status`,
   `## Context`, `## Decision`, `## Consequences`.
2. Name the file `NNNN-title-with-dashes.md` with the next free number.
   Numbers are never reused or renumbered.
3. Add a row to the table above.
4. To reverse a decision, write a **new** ADR and set the old one's status to
   `Superseded by ADR-NNNN`. Don't edit the original's reasoning — an ADR is a
   record of what was decided at a point in time.
