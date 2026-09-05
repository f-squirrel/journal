---
name: adr
description: Record architecture decisions as short markdown files in docs/adrs/ — what was chosen, why, and what alternatives were considered. Use whenever a real architectural decision gets made or discovered (a language, framework, library, storage format, protocol, or structural choice, and the reasoning or alternatives behind it), when the user asks "record this decision", "does this need an ADR", "why did we choose X", or after any change that introduces or changes such a decision. Always ask the user for confirmation before creating the file.
---

# ADR

Architecture Decision Records live in `docs/adrs/` as short, individual
markdown files — one per decision. Each one is a quick, honest record of
*what* was chosen and *why*, so nobody has to reverse-engineer the reasoning
later from code or chat history.

## When a decision needs a record

It's a decision worth recording if it:

- picks a language, framework, runtime, or major library (e.g. "backend is
  Rust", "UI is Tauri + React")
- picks a data format, protocol, storage layout, or API shape
- is not obviously the only option — there were real alternatives, and
  someone chose between them for a reason
- would be expensive or confusing to silently reverse later
- is the kind of thing a future contributor would reasonably ask
  "why is it done this way?" about

It's **not** worth recording if it's a small, easily-reversible implementation
detail, a bug fix, or something with no real alternative.

When in doubt, ask the user rather than skipping it.

## Before creating a record

**Always ask the user first.** Something like:

> This looks like an architectural decision (using X because Y). Want me to
> record it as an ADR?

Only write the file once they confirm. Don't create ADRs silently.

## Format

Keep it short — a paragraph or two per section, not an essay.

File: `docs/adrs/NNNN-short-title-with-dashes.md`, using the next unused
number (zero-padded to 4 digits), e.g. `docs/adrs/0001-use-rust-for-backend.md`.

```markdown
# NNNN. Title in the imperative

## Status

Accepted — YYYY-MM-DD

## Decision

What was chosen, in one or two sentences.

## Reasons

Why this, briefly.

## Alternatives considered

Other options and why they were passed over (if any were seriously
considered — omit this section if there weren't real alternatives).
```

If `docs/adrs/README.md` exists, add a one-line entry linking the new file. If
it doesn't exist yet, create it with a short intro and a table (`Number |
Title | Status`) when writing the first ADR.

## Superseding a decision

Never edit an accepted ADR's Decision/Reasons to match new code. If a past
decision is being reversed, write a new ADR and change the old one's Status
line to `Superseded by ADR-NNNN`.

## Checking after work

After finishing a change, briefly check whether it made (or changed) an
architectural decision as described above. If so, follow "Before creating a
record" and ask the user.
