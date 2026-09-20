---
name: feature-plan
description: Brainstorm a feature or idea with the user and turn it into a concrete implementation plan, saved to .claude/plans/ as a hand-off brief other agents can execute without this conversation. Use when the user asks to "plan", "brainstorm", "design", "think through", "scope out" or "write up a plan for" a feature, change or idea, or says a plan should be handed off to other agents. Not for small edits the user has already fully specified — just make those.
---

# Feature plan

Two jobs in one skill: **brainstorm** an idea with the user until the design is
settled, then **write the plan** so an agent that has never seen this
conversation can carry it out. The plan is the deliverable. Nothing gets
implemented here.

Plans live in `.claude/plans/<slug>.md`. That directory is gitignored — a plan
is a local working document, not project history. Anything that should outlive
the work belongs in an ADR (`adr` skill) or `CLAUDE.md` (`claude-md-check`
skill), and the plan's last work package says so.

Do **not** use plan mode (`EnterPlanMode`, or the built-in `/plan`) for this:
it blocks the file write that is the whole point, and its approval step leads
into implementing in the same session, where this skill hands off instead.
Talk in the conversation, write the file at the end. If the session is
already in plan mode, brainstorm as usual, then ask the user to leave it
before you write the file.

The request is `$ARGUMENTS`, or the words of the user's message.

## 1. Ground yourself before asking anything

A question the codebase can answer is a wasted question, and a plan written
from memory of `CLAUDE.md` goes stale in exactly the places that matter.

1. Read `CLAUDE.md` and `docs/adrs/README.md`; read any ADR the idea touches.
   They state deliberate decisions (alpha stance and no migrations, snake_case
   wire types, `HashRouter`, no search index, the crate layering) that a plan
   must not quietly contradict.
2. `ls .claude/plans/`. If a plan already covers this idea, or half of it,
   **update that file** rather than starting a second one. If it overlaps,
   say so and reference it.
3. Read the code the idea would touch: the actual modules, types, commands
   and tests, not just their descriptions. Note file paths and names as you
   go — the plan needs them.
4. Check the current state of the tree (`git status`, `git log --oneline -5`)
   so the plan is written against what is really there, including
   uncommitted work.

Keep a list of **verified facts** as you read. They go into the plan under
"What is true today". Mark anything you inferred rather than saw.

## 2. Brainstorm with the user

Restate the goal in one or two sentences and check it is the right one. Then:

- **Ask only what is the user's to decide** — product behaviour, scope, a
  trade-off with real cost on both sides. Use `AskUserQuestion`, one to four
  questions at a time, 2–4 options each, the recommended option first and
  marked. Everything you can settle from the code or a sensible default,
  settle yourself and record it as a decision or assumption in the plan.
- **Propose approaches, not a survey.** When there is a real fork, lay out
  two or three, what each costs, and recommend one. If there is one obvious
  design, say that and move on.
- **Test the design against the repo's deliberate decisions** (from step 1).
  Anything that would drop existing local data, change the on-disk or wire
  format, or add a dependency needs the user's explicit yes — ask, and record
  the answer. Anything ADR-worthy gets flagged for the `adr` skill; never
  create the ADR file without confirmation.
- **Cut scope.** For each piece ask whether the goal fails without it. What
  is not needed becomes a named non-goal, not a quiet omission.
- **Size the idea.** If it is really several features, plan them as separate
  sections with their own dependencies (as `next-features.md` does), or as
  separate plans. One plan should be executable start to finish by a handful
  of agents.
- Stop brainstorming when the design is settled. Do not keep asking to look
  thorough.

## 3. Decompose into work packages

Break the design into **work packages (WPs)**. A good WP:

- is one coherent change an agent can finish in a sitting and *verify on its
  own* — it ends with a command that passes, not "looks right";
- names the **files** it creates or edits and the **steps** to take, at the
  level of "add `Vault::search` in `crates/vault/src/search.rs` that returns
  `Vec<SearchMatch>`", with signatures, types, names and small code sketches
  where the shape matters. Not "implement search";
- carries its **own tests** — the test is part of the WP that introduces the
  behaviour, not a tail at the end;
- states **Depends on** and **Can run in parallel with**. Two WPs that touch
  the same file are not parallel-safe; say which one goes first or how to
  merge.

Order by dependency, and put the thing everything else stands on first — the
model, the wire type, the trait — because later WPs are written against it.

**Pin the contract.** Anything two WPs must agree on — type and field names,
command names and signatures, file paths, make targets, env vars, event
names, error strings the UI matches on — goes in one "Contract" section and is
declared not to change. That is what lets WPs run in parallel without drifting.

Finish with a **docs WP**: the `claude-md-check` skill for `CLAUDE.md`,
whether an ADR is warranted (ask the user first), and — when the plan added or
changed Rust or TS/TSX code — the `coverage-check` skill, run once by the
coordinator after the WPs are merged, not by each parallel agent.

Use this repo's own verification commands in "Done when", not generic ones.
Pick what actually applies:

| Change touches | Verify with |
| --- | --- |
| Rust crates | `make rust-test` (there is no linter to cite) |
| Types crossing `invoke` | `make bindings`, then `make check-bindings` |
| Frontend | `npm run build` (runs `tsc`) |
| UI behaviour | `make test-e2e`; new spec under `e2e/`, using the fallback's `?fail=`/`?slow=` hooks where a failure path needs seeing |
| Formatting | `make format` |
| Bundle hygiene (fallback code) | `make check-no-fallback` |
| Any Rust or TS/TSX code | `make coverage` (the `coverage-check` skill) |

UI work also gets "check at phone width" (the layout collapses below `md`).

## 4. Write the plan

Save to `.claude/plans/<slug>.md`, where `<slug>` is short kebab-case naming
the work (`idle-auto-lock`, `code-coverage`). Use this structure; drop a
section only when it truly has nothing to say.

````markdown
# Plan: <what this delivers, in a phrase>

Status: **draft, not started.** <One line on where it stands.> Read the
repo's `CLAUDE.md` first, in particular <the sections that matter>, and
<ADRs / other plans that matter>.

## Goal

<What is delivered and what the user will be able to do or see. Two to five
sentences. Include the constraint that shaped it, if any.>

## What is true today (verified while planning)

<Bullets of facts read from the code, with paths. Flag surprises and traps.
This is what stops the next agent re-deriving it, or trusting a stale doc.>

## Design

<The chosen approach and why, in prose and small sketches — types,
signatures, data flow. Then "Alternatives considered": each rejected option
and the reason, in a line or two. Where the design leans on or departs from
an ADR or a deliberate decision in CLAUDE.md, say which.>

## Decisions

<Questions the user answered, with the answer and the reason, and any still
open. Mark what blocks which WP.>

## Contract between work packages

<Names, signatures, paths, targets, env vars every WP depends on. "Do not
change these." Omit for a single-WP plan.>

## Work packages

<Order, and which can run in parallel, in a sentence or a small table:

| WP | Title | Depends on | Parallel with |
| --- | --- | --- | --- |
>

### WP1 — <title>

**Files:** `path/one.rs` (new), `path/two.ts` (edit).

1. <Concrete step, with names and signatures.>
2. <…>
3. Tests: <the cases that must exist, by name or behaviour.>

**Done when:** <commands that pass, and the observable result. Specific
enough that a reviewer can check it without asking.>

### WP2 — <title>
…

## Non-goals and known gaps

<What this deliberately does not do, and what is left over that a later
change should pick up. Each with the reason.>

## Open questions

<Anything still undecided, recommended answer first. Empty when there are
none — say "None.">

## Handoff rules

- Read `CLAUDE.md` and the ADRs named above before starting.
- Nobody commits. Leave changes in the working tree and end with a suggested
  Conventional Commits message; the coordinator commits with the `commit`
  skill.
- Do not create an ADR file without the user's confirmation.
- If something here turns out to be wrong, stop and report it rather than
  improvising around it. Record what you actually built at the end of your
  WP, for the next one.
````

Style for the body:

- **Write for a cold reader.** Assume the agent has read `CLAUDE.md` and
  nothing else. No "as discussed", no "the approach we agreed" — say what it
  is. Spell out the *why* wherever an agent could be tempted to "fix" a
  deliberate choice.
- **Be concrete, not exhaustive.** Names, paths, signatures, cases and
  commands — yes. Re-explaining how Tauri works — no. Where an existing
  module already shows the pattern, point to it ("same shape as
  `search_entries`") instead of restating it.
- **Say what is verified and what is guessed**, and where a number or claim
  can be measured, say to measure it rather than assert it.
- Table separator rows are written `| --- | --- |`, with spaces (markdown
  lint MD060).

## 5. Check it is ready to hand off

Before showing the plan, read it once as an agent who has only this file and
`CLAUDE.md`, and confirm:

- The goal and every non-goal are stated; nothing important is implied.
- Every WP names its files, gives its steps, and ends with a passing command.
- Every name two WPs share is in the Contract, once.
- Dependencies are explicit, and no two "parallel" WPs edit the same file
  without saying how to merge.
- Every open question says what it blocks and has a recommended answer.
- Anything that drops data, changes a format, adds a dependency or needs an
  ADR is marked as needing the user, with their answer if they have given it.
- Nothing in it depends on this conversation to make sense.

Fix the gaps, then tell the user where the file is and give them:

1. the goal, and the chosen design in a few lines;
2. the WP list with the dependency order, and what can run in parallel;
3. the decisions that are still open, if any.

Leave **Status: draft** until the user has read and approved it, then change
it to **approved**. Do not start implementing unless the user asks — and if
they do, hand WPs to agents one at a time (or in parallel where the plan says
so), each pointed at the plan file.

## Keeping the plan alive

The plan is also the hand-off log. When a WP is done, update its status in
place and add a short "What WP<n> actually built" note: names that differ from
the sketch, traps found, measurements taken. The next agent reads that, not
the diff. When all of it is done, set the status to **done** and list what was
carried over to `CLAUDE.md`, an ADR, or a follow-up plan.
