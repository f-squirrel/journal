# 22. Plan work as specs and checklists, commit in Conventional Commits

## Status

Accepted — 2026-08-15 (see [`docs/superpowers/README.md`](../superpowers/README.md)
and `.claude/skills/conventional-commit-guide/SKILL.md`)

## Context

This project is built largely through sessions with coding agents, which have
no memory between sessions and a strong tendency to re-derive decisions that
were already settled — or to "fix" a deliberate trade-off such as M1's missing
Argon2id ([ADR-0008](0008-age-backend-for-m1.md)). Human contributors joining
later face the same problem in slower motion.

The evidence that the process matters is in the repository's own history: the
first M1 plan had seven tasks that were multi-day epics with no file paths and
verification listed after implementation, and a critical review of it produced
six format-level changes — keyed object names, the key-slot list, the
non-plaintext index, tombstones, a corrected claim about re-keying, and the
KDF ceiling — every one of them cheaper to make in a document than in a shipped
vault format.

## Decision

Work follows the `superpowers` lifecycle, with artifacts checked into the repo:

1. **Brainstorm** → `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`
2. **Plan** → `docs/superpowers/plans/YYYY-MM-DD-<feature>.md`, a checklist of
   bite-sized tasks with exact file paths and **verification steps written
   before the implementation step**
3. **Register** the plan in [`plans/INDEX.md`](../superpowers/plans/INDEX.md),
   the single glance-able view of project state
4. **Execute** on a branch, ticking checkboxes as the source of truth for
   task-level progress
5. **Review**, then **finish** the branch, then set the plan's row to `Done`

A spec is optional for small work; a plan is not. Finished plans are immutable
history — scope changes get a new plan that links back, never a rewrite.
Every task carries a docs-review step so documentation and code stay in step.
Commit messages follow **Conventional Commits**.

Architecture decisions additionally get an ADR in this folder
([ADR-0001](0001-record-architecture-decisions.md)).

## Consequences

- Any session — human or agent — can reconstruct project state from
  `INDEX.md`, the current plan's checkboxes, and this ADR folder.
- Test-first is structural rather than aspirational: the verification step is
  written into the plan before the implementation step is.
- Documents are reviewed before code is written, which is where the format-level
  corrections above came from and why they were cheap.
- Real overhead: a plan for small work, and a docs-review pass per task. The
  plan for trivial work is short, and the alternative — stale documentation in
  a repository whose documentation is load-bearing for correctness — costs more.
- Plans, specs and ADRs are checked in, so they are reviewed in pull requests
  like code.
