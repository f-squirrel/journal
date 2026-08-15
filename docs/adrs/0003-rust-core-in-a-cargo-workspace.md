# 3. Put the core in Rust, split across a Cargo workspace

## Status

Accepted — 2026-08-15 (ratified in the design doc §4, 2026-08-14; implemented
in M1 Task 1)

## Context

Requirements T1 (maximize shared code across platforms), T2 (security is a
first-class constraint) and T3 (core logic in Rust) pull in the same direction:
the parts of this app that are expensive to get wrong — key handling, the vault
format, sync reconciliation — should be written once, in a language with
memory safety and a credible audited-crypto ecosystem, and reused on every
platform rather than reimplemented per OS.

The open question was granularity: one core crate, or several.

## Decision

All security-relevant and platform-independent logic lives in Rust, organized
as a Cargo workspace with one crate per layer:

```
crates/journal-model/    entries, attachments, manifest — data shapes, no I/O
crates/journal-crypto/   sealing backend, keys, object naming, recovery code
crates/journal-store/    encrypted on-disk vault + derived search index
crates/journal-sync/     provider trait, change detection, conflicts   (M2)
crates/journal-dropbox/  Dropbox provider, OAuth PKCE                  (M2)
apps/desktop/            Tauri 2 shell: IPC commands, protocol handler
ui/                      React + TypeScript frontend
```

Dependencies point one way: `journal-model` knows nothing of the others,
`journal-store` depends on `journal-crypto` and `journal-model`, and the app
shell depends on all of them. Lints are declared once in
`[workspace.lints.clippy]` and inherited ([ADR-0020](0020-toolchain-lints-and-supply-chain.md)).

## Consequences

- A mobile build (M3/M4) reuses the entire core; only the shell changes.
- The crypto crate has a small, reviewable surface. "Does this touch key
  material?" is answerable by looking at one directory.
- `journal-model` holding no I/O and no crypto is what lets golden-file
  serialization tests exist at all — the shapes can be round-tripped without a
  vault.
- More crates means more `Cargo.toml` boilerplate and cross-crate refactors
  that would be free inside one crate. Accepted: the boundaries are the point.
- Two of the crates (`journal-sync`, `journal-dropbox`) are named here but do
  not exist yet. Documentation must distinguish planned from present, which
  the crate READMEs and
  [`docs/superpowers/plans/INDEX.md`](../superpowers/plans/INDEX.md) do.
