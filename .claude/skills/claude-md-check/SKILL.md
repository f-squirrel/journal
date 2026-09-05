---
name: claude-md-check
description: After making changes to this repo, check whether the project's CLAUDE.md needs updating — only for high-value, durable facts (project/directory structure, build/run/test commands and tools, top-level architecture). Use at the end of any task that added/moved/removed a directory or crate, changed how the project is built/run/tested, or changed a major architectural component or convention described in CLAUDE.md. Skip it for routine changes that don't affect those sections.
---

# Keeping CLAUDE.md in sync

CLAUDE.md exists to orient a future Claude session fast: where things live,
how to build/run/test, and the shape of the architecture. It should track
only durable, high-value facts — not every change made in a session.

## When to flag an update

After finishing a change, check whether it touched any of:

- **Project structure** — a new/removed/renamed top-level directory, crate,
  or package; a new convention for "where does X go" (e.g. adding the
  `crates/` workspace was worth documenting; adding one new page component
  under an already-documented `pages/` convention is not).
- **Commands/tooling** — a new or changed way to build, run, test, or lint;
  a new required toolchain; a changed npm script or Makefile target.
- **Architecture** — a new major component, a changed communication pattern
  between frontend/backend, a changed single-source-of-truth for state, or a
  first-time integration (persistence, sync, crypto) landing in the code.

If none of these changed, say nothing — don't manufacture an update just
because a task finished.

## What NOT to put in CLAUDE.md

- Implementation details inside a single file or function.
- Routine feature work that fits a convention CLAUDE.md already describes.
- In-progress or experimental work that isn't settled yet.
- The reasoning behind an architectural *decision* — that belongs in an ADR
  under `docs/adrs/` (see the `adr` skill). CLAUDE.md may gain at most a
  one-line pointer to the new ADR, never the rationale itself.
- Anything obvious from reading the code.

## Process

1. Compare what changed against CLAUDE.md's existing sections.
2. If it's in scope (see above), draft the smallest edit that keeps CLAUDE.md
   accurate — usually one or two lines, matching its existing terse style.
   Don't add new prose sections or restate things it already says correctly.
3. Show the user the proposed edit and confirm before applying it — CLAUDE.md
   is checked into the repo and shapes every future session, so treat it like
   the `adr` skill treats new ADRs: propose, don't silently commit.
