---
name: adr-guide
description: Keeps work in this repo aligned with the architecture decision records in docs/adrs/, in both directions — read the governing ADR BEFORE writing or changing code, and AFTER a change check whether it violated an accepted ADR or made a new architectural decision that needs one recorded. Use before editing anything under crates/, apps/desktop/, ui/, .github/workflows/, Cargo.toml, deny.toml, rust-toolchain.toml, or docs/superpowers/specs/; before proposing a design, adding a dependency or crate, changing an on-disk or wire format, adding a Tauri IPC command, touching key material, or moving work between milestones; and after writing code, before committing or opening a PR. Also use when the user asks "why is it done this way", "does this break an ADR", "do we need an ADR for this", "record this decision", or pushes back on a constraint that an ADR already priced. docs/adrs/ is the source of truth for decisions — a decision that exists only in code, a spec section, or a commit message is not recorded.
---

# ADR Guide

[`docs/adrs/`](../../../docs/adrs/README.md) is this repo's **source of truth
for decisions**: what was chosen, why, what it costs, whether it still stands.
The specs under `docs/superpowers/specs/` are the source of truth for
*implementable detail* derived from those decisions — byte offsets, field
layouts, KDF parameters.

Several things in this codebase look like defects and are priced, deliberate
trade-offs: M1 has no Argon2id, a "public" key is treated as secret, the search
index is rebuilt on every unlock, a header field is reserved and unused. The
ADR says what the alternative cost. Read it before you "fix" one.

## Before writing code

Find the records that govern what you're about to touch, and read them. Not the
whole folder — these:

| You're touching | Read first |
|---|---|
| `journal-crypto` seal/open, `backend.rs`, anything taking a `Binding` | [0007](../../../docs/adrs/0007-pluggable-sealing-backend-seam.md), [0009](../../../docs/adrs/0009-bind-every-object-to-its-identity.md) |
| Key types, key slots, unlock, passphrase change, recovery code | [0008](../../../docs/adrs/0008-age-backend-for-m1.md), [0010](../../../docs/adrs/0010-luks-style-key-slots.md), [0011](../../../docs/adrs/0011-recovery-code-format.md), [0012](../../../docs/adrs/0012-device-portable-kdf-parameters.md) |
| Object naming, blob paths, content addressing | [0013](../../../docs/adrs/0013-keyed-object-names.md) |
| Vault header, format versions, parse/reject logic | [0016](../../../docs/adrs/0016-versioned-formats-fail-closed.md) |
| Search, `index.rs`, anything that writes to disk | [0014](../../../docs/adrs/0014-in-memory-search-index.md) |
| Deletion, manifest, tombstones, GC | [0015](../../../docs/adrs/0015-tombstones-and-blob-gc.md) |
| Tauri commands, IPC types, blob protocol, auto-lock | [0005](../../../docs/adrs/0005-ipc-boundary-and-lock-teardown.md), [0004](../../../docs/adrs/0004-tauri-2-and-react-for-the-ui.md) |
| Export / import | [0017](../../../docs/adrs/0017-plaintext-export-and-import-in-m1.md) |
| Sync, providers, conflict handling (M2) | [0018](../../../docs/adrs/0018-sync-provider-trait.md) |
| New crate, new directory, cross-crate dependency | [0003](../../../docs/adrs/0003-rust-core-in-a-cargo-workspace.md) |
| New third-party dependency | [0020](../../../docs/adrs/0020-toolchain-lints-and-supply-chain.md), [0021](../../../docs/adrs/0021-mit-license.md) |
| CI, lints, `deny.toml`, toolchain | [0020](../../../docs/adrs/0020-toolchain-lints-and-supply-chain.md) |
| "Should we do X now or later?" | [0019](../../../docs/adrs/0019-milestone-sequencing.md), [0008](../../../docs/adrs/0008-age-backend-for-m1.md) |
| Docs, plans, specs, commit messages | [0022](../../../docs/adrs/0022-spec-and-plan-driven-workflow.md), [0001](../../../docs/adrs/0001-record-architecture-decisions.md) |

The full index is [`docs/adrs/README.md`](../../../docs/adrs/README.md).

## After writing code: two questions

Ask both, every time, before committing.

### 1. Did this violate an accepted ADR?

Scan the diff against this list. Each line is a violation of a record that
already exists — if you wrote one, stop.

- A `seal`/`open` path that doesn't take a `Binding`, or returns plaintext
  before the binding verifies, or compares the preamble non-constant-time → 0009
- Key material, key-slot bytes, or a vault file path crossing Tauri IPC; an
  image as base64 in an IPC payload; entry text, titles, passphrases, or keys
  reaching `tracing` at any level — including via a derived (unredacted)
  `Debug`/`Display` on a type that holds them, which leaks the moment
  anything logs the value, not just at an existing `tracing::` call site → 0005
- `Serialize` or `Debug` on a key type; key material not wrapped in
  `zeroize`/`secrecy` → 0008, 0009
- The age X25519 **recipient** written to disk, logged, exported, or returned
  across IPC — it is key material, not a public key → 0009
- An object named by a bare hash of its content → 0013
- The search index, or any decrypted content, written to disk → 0014
- An object deleted without a manifest tombstone → 0015
- A parser that repairs, guesses, or best-effort-opens instead of rejecting;
  opening an unknown `format_major`; writing to a vault that failed validation;
  a version check that runs *after* a strict (`deny_unknown_fields`) parse
  instead of before it, so a document from a genuinely newer format version
  fails as a generic parse error instead of the intended
  "unsupported version" → 0016
- scrypt work factor calibrated rather than pinned, or a KDF parameter above
  the 256 MiB ceiling → 0012
- **Adding Argon2id to M1 "to fix F4"** — that is a documented deferral, not an
  oversight → 0008
- A hand-rolled crypto primitive, container, or encoding → 0008
- A GPL-3.0-only dependency → 0021
- `unwrap()` / `expect()` without a deliberate, justified allow → 0020
- Byte layouts, framing, or KDF parameters chosen at the call site instead of
  taken from the crypto spec → 0006

**A violation is never resolved by editing the ADR.** If the code is right and
the record is wrong, that is a *reversal* — see below. Default is that the
record is right.

### 2. Did this make a new decision?

It's a decision needing a record if it:

- would make an existing ADR's **Consequences** section wrong
- is expensive to reverse once users have vaults on disk — on-disk format,
  object naming, key hierarchy, directory layout
- changes the trust boundary, or what the project does and does not defend
  against
- adds or removes a crate, a platform, or a dependency with license/advisory
  weight
- moves work between milestones, or changes what a milestone promises
- picks between two viable approaches where a future reader would reasonably
  ask "why this one, and what did the other cost?"

It's **not** a decision if it: follows an existing ADR; is a local
implementation choice that's cheap to reverse and has no cross-cutting
consequence; is a bug fix; or is already specified byte-for-byte in a spec.

When in doubt, it's a decision — an unnecessary ADR is cheap, an unrecorded one
is what this folder exists to prevent.

## What to do

**On a violation.** Stop and say which ADR, quoting the line. Don't silently
rewrite the code to comply either — the user may be deliberately reversing the
decision. Ask (`AskUserQuestion` where supported) with the specific record and
the specific diff, and offer the two paths: comply, or supersede the ADR.

**On a new decision.** Draft the record with **Status: Proposed**, show the
user the Context/Decision/Consequences, and only mark it `Accepted` once they
agree. The decision is theirs; the writing-down is yours.

**On a reversal.** Write a **new** ADR that supersedes the old one and set the
old record's status to `Superseded by ADR-NNNN`. Never edit an accepted
record's reasoning — an ADR is what was decided at a point in time, not a
description of current code.

**On a spec that contradicts an accepted ADR** on the decision itself (not on
byte-level detail): that's an undocumented reversal. Resolve it explicitly —
don't follow whichever document you happened to read first.

## Writing the record

Nygard format per [adr.github.io](https://adr.github.io/), matching the
existing files:

```markdown
# N. Title in the imperative

## Status

Proposed | Accepted — YYYY-MM-DD | Superseded by ADR-NNNN

## Context

The forces at play, and the problem. What made the obvious answer wrong.

## Decision

What we will do, in the active voice.

## Consequences

What becomes easier, what becomes harder, what we give up. Be honest about
the costs — a Consequences section with no costs in it is not finished.
```

- File name `docs/adrs/NNNN-title-with-dashes.md`, next free number, never
  renumbered or reused.
- Add a row to the table in [`docs/adrs/README.md`](../../../docs/adrs/README.md).
- Keep it to the decision. An ADR that reproduces a spec's field tables is
  over-specified — link to the spec section instead.
- Link sibling records the way the existing ones do.

## The reminder hook

`.claude/settings.json` carries a `PostToolUse` hook on `Edit|Write|MultiEdit`
that fires when the edited path is under `crates/`, `apps/`, or `ui/`, and
injects a one-line reminder to run this skill against the diff. It is a
prompt, not a gate — it cannot see what changed and blocks nothing. The
checklist above is still what does the work.

## Related skills

- `docs-review-guide` — the broader docs sync check at branch-finish / PR time.
  This skill is the per-change one; that skill's ADR item points back here.
- `conventional-commit-guide` — an ADR-only change is `docs(adrs): ...`.
