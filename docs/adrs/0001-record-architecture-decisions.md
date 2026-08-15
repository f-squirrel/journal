# 1. Record architecture decisions

## Status

Accepted — 2026-08-15

## Context

The project's architectural reasoning lives in two long design documents —
[`docs/superpowers/specs/2026-08-14-journaling-app-design.md`](../superpowers/specs/2026-08-14-journaling-app-design.md)
and [`docs/superpowers/specs/2026-08-14-crypto-design.md`](../superpowers/specs/2026-08-14-crypto-design.md)
— plus a milestone plan and the revision notes appended to each. Those
documents are the right home for *normative* detail (byte layouts, task
checklists), but they are a poor index of *decisions*: a reader who wants to
know why M1 has no Argon2id, or why object names are keyed, has to locate the
answer inside a section written for a different purpose, and a decision that
was reversed is visible only as a diff.

That gap has a cost specific to this repository. Several settled trade-offs
look like defects at a glance — a missing requirement (F4), a "public" key
treated as secret, a search index rebuilt on every unlock — and both humans and
coding agents keep proposing to "fix" them. The reasoning exists; it is just
not addressable.

## Decision

Keep a folder of Architecture Decision Records at `docs/adrs/`, in the format
described at [adr.github.io](https://adr.github.io/) (Michael Nygard's
template): **Title, Status, Context, Decision, Consequences**.

- One file per decision, named `NNNN-title-with-dashes.md`, numbered
  sequentially and never renumbered.
- [`docs/adrs/README.md`](README.md) indexes every record.
- An accepted ADR is immutable history. Changing our mind means writing a new
  ADR that supersedes the old one, and updating the old one's Status to
  `Superseded by ADR-NNNN` — never editing the original's reasoning.
- ADRs record *what was decided and why*. They are not normative: where an ADR
  and a spec disagree about a byte layout, a KDF parameter, or an object name,
  **the spec wins**, and the ADR should be superseded to match.
- Records 0002–0022 were written retrospectively on 2026-08-15 from decisions
  already made and documented elsewhere. Their dates are the date of record,
  and each cites the source that ratified it.

## Consequences

- There is one place to look for "why is it like this?", and one place to add
  a decision to when making a new one.
- Retrospective records carry a risk of tidying history into something
  cleaner than it was. Where a decision was reversed during review, the ADR
  says so rather than presenting the final position as the only one considered.
- A double-write burden: substantive decisions now land in both a spec and an
  ADR. The ADRs stay short and link out to keep that cost small, and the specs
  remain the single source of implementable truth.
- New work should check this folder before proposing a change to crypto,
  storage layout, or the IPC boundary — an existing ADR is likely to have
  already priced the alternative.
