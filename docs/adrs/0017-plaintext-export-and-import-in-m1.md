# 17. Ship plaintext export and import in M1

## Status

Accepted — 2026-08-15 (design doc §2 F7, §9; first revision 2026-08-14, where
it was moved *into* M1)

## Context

Export was originally a later-milestone nicety, and there is a real argument
for delaying it: an export writes the user's entire journal to disk in
cleartext, in an app whose whole premise is that this never happens. Every
other decision here works to keep plaintext off disk
([ADR-0014](0014-in-memory-search-index.md) exists for exactly that reason).

The counter-argument won. For a zero-knowledge app with no server, "can I get
my data out if this app breaks, or if I stop trusting you?" is *the* trust
question, and the answer cannot be "trust us". Withholding export also hides a
data-loss risk behind a leak risk — and the leak risk is the one that has a
straightforward mitigation.

## Decision

Plaintext export and import are **requirement F7 and ship in M1**.

- `export_vault` writes a plaintext tree of Markdown files plus images, behind
  an explicit confirmation that names what is about to be written and where.
  The confirmation cannot be bypassed at the command layer.
- Import of that same layout ships alongside, so the round trip is real rather
  than a one-way exit — export → import into a fresh vault must reproduce the
  same entries and attachments.
- The independent decryptor of
  [ADR-0006](0006-spec-first-crypto-with-an-independent-decryptor.md) is the
  second escape hatch, usable even if this app will not start.

## Consequences

- The trust claim is checkable by the user rather than taken on faith, and the
  data outlives the project.
- An exported tree is exactly as sensitive as the journal itself and has none
  of the vault's protections. The mitigation is informed consent at the moment
  of export — which is why the confirmation names the destination path.
- M1's leak sweep (Task 16) must not be confused by exports: the sweep asserts
  that the *app's own* data directory contains no plaintext canary. A
  user-requested export to a chosen path is out of that scope by design.
- Import gives a migration path in from other journaling apps for free, if
  their output is reshaped into the same layout.
