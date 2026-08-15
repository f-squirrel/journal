# Agentic Development Workflow

This project uses the [superpowers](https://github.com/obra/superpowers) plugin
for planning and executing work with Claude. This doc explains where things
live so planning, execution, and history stay easy to follow across sessions.

## Lifecycle

1. **Brainstorm** — `superpowers:brainstorming` (or `/brainstorm`) turns a
   rough idea into a validated design through Socratic back-and-forth.
   Output: `specs/YYYY-MM-DD-<topic>-design.md`.
2. **Plan** — `superpowers:writing-plans` (or `/write-plan`) turns a design
   (or a smaller idea that doesn't need a full spec) into a checklist of
   bite-sized tasks with exact file paths and verification steps.
   Output: `plans/YYYY-MM-DD-<feature-name>.md`.
3. **Register** — add a row to [`plans/INDEX.md`](plans/INDEX.md) so the
   plan is visible without opening the file.
4. **Execute** — `superpowers:executing-plans` (often via
   `superpowers:subagent-driven-development`) works through the plan's
   checklist, usually in an isolated branch/worktree
   (`superpowers:using-git-worktrees`). Checkboxes in the plan file are
   ticked off as each task completes — the plan file is the source of truth
   for task-level progress.
5. **Review** — `superpowers:requesting-code-review` before merging.
6. **Finish** — `superpowers:finishing-a-development-branch` merges (or
   opens a PR) and cleans up the branch/worktree.
7. **Close out** — update the plan's row in `plans/INDEX.md` to `Done` (or
   `Abandoned` with a one-line reason).

## Recording decisions

This lifecycle tracks *work*, not *decisions*. Architectural decisions — the
ones that outlive the plan that produced them — are recorded separately, as
ADRs in [`docs/adrs/`](../adrs/README.md), which is the source of truth for
what was decided and why.

That cuts across the lifecycle rather than sitting at one step: a brainstorm
usually settles several decisions at once, and execution occasionally forces a
new one. Whenever it happens, add the record — a decision that exists only in a
spec section, a plan's prose, or a commit message is not recorded. Specs and
plans stay authoritative for what they are good at: implementable detail and
task-level progress respectively.

## Directory layout

```
docs/superpowers/
├── README.md         this file
├── specs/             design docs from brainstorming (one per topic)
│   └── YYYY-MM-DD-<topic>-design.md
└── plans/             implementation plans (one per feature/task)
    ├── INDEX.md       status of every plan, newest first
    └── YYYY-MM-DD-<feature-name>.md
```

Architecture decision records live outside this tree, in
[`docs/adrs/`](../adrs/README.md), because they are not tied to the lifecycle
of any one plan.

A spec is optional — small tasks can go straight to a plan. A plan is not
optional; even a small task gets a short checklist so execution and review
have something concrete to check off and against.

## Status values (`plans/INDEX.md`)

- `Draft` — plan written, not started
- `In Progress` — actively being executed
- `Blocked` — waiting on something external
- `Done` — merged
- `Abandoned` — won't do; note why

Keep `INDEX.md` as the single glance-able view of project state. The plan
files themselves stay immutable history once done — don't rewrite a
finished plan to reflect scope changes; write a new one and link back if
needed.
