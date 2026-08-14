# specs

Design docs produced by the brainstorming stage of the
[superpowers workflow](../README.md), one per topic. A spec captures a
validated design reached through Socratic back-and-forth — the *why* behind
an architecture or format decision — before any implementation plan is
written against it.

Naming: `YYYY-MM-DD-<topic>-design.md`.

## Current specs

- [`2026-08-14-journaling-app-design.md`](2026-08-14-journaling-app-design.md)
  — the project's overall architecture, threat model, cryptography, storage
  format, and sync design. The reference doc for how and why the app is
  built the way it is.
- [`2026-08-14-crypto-design.md`](2026-08-14-crypto-design.md) — the
  cryptographic scheme the design doc's §5 sketches, specified to the byte
  and split into a backend-independent core (vault layout, header, keyed
  object naming, recovery code, the `Binding` contract) over a pluggable
  sealing backend. M1 ships the `age` backend; the native Argon2id +
  XChaCha20 backend is the M2 target and is specified alongside it.
  Normative — and supersedes the design doc's §5 wherever they differ. M1
  Task 16's independent decryptor is written from this document alone.

A spec is optional for small tasks — see the parent
[`docs/superpowers/README.md`](../README.md) for when to write one, and
[`../plans/`](../plans/) for the implementation plans that reference these
specs.
