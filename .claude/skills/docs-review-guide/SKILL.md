---
name: docs-review-guide
description: Checks that this repo's docs — both AI-facing (CLAUDE.md, docs/superpowers/specs, docs/superpowers/plans, INDEX.md, SKILL.md files) and human-facing (root README.md, ui/README.md, apps/desktop/README.md, crates/*/README.md) — still match the latest code changes, and flags anything a change makes stale or false. Use this before finishing a development branch or opening/updating a PR, after completing a plan task or checklist item, whenever the user asks to "update docs", "check the docs", "sync docs", "review documentation", or "does this need a doc update", and any time a change touches something a doc asserts (workspace layout, crate list, build/lint commands, security-critical conventions, roadmap/status, §3 non-goals). If a change appears to contradict what a doc currently claims, stop and ask the user via AskUserQuestion rather than silently resolving it either direction.
---

# Docs Review Guide

This repo documents itself in two audiences at once, and they drift
independently if only one gets updated:

- **AI-facing docs** — read by Claude Code sessions to orient themselves:
  root [`CLAUDE.md`](../../CLAUDE.md), [`docs/superpowers/README.md`](../../docs/superpowers/README.md),
  every file in `docs/superpowers/specs/`, every file in `docs/superpowers/plans/`
  (including [`plans/INDEX.md`](../../docs/superpowers/plans/INDEX.md)), and
  the `SKILL.md` files under `.claude/skills/`.
- **Human-facing docs** — read by a person skimming the repo: root
  [`README.md`](../../README.md), `ui/README.md`, `apps/desktop/README.md`,
  and each `crates/*/README.md`.

Several facts are asserted in more than one place on purpose (e.g. the
workspace layout diagram appears in both `CLAUDE.md` and root `README.md`;
the security model summary in `README.md` mirrors the design spec's threat
model). When you touch one copy, check the other.

## When to run this

- Before `superpowers:finishing-a-development-branch` / opening or updating
  a PR.
- After checking off a task in a plan file, or changing a plan's status.
- Any time a change touches something a doc currently asserts — a new or
  removed crate/directory, a build/lint/test command, a security-critical
  convention, a "not implemented yet" / roadmap / status claim, or the §3
  "does not defend against" list in the design spec.
- On explicit request ("update docs", "check the docs are current", "does
  this need a README change", etc).

## What to check

Work from the actual diff (`git diff` against the branch's base, or the
set of files just changed this session), not from memory of the whole repo.
For each changed area, ask whether any doc makes a claim the change now
falsifies:

1. **Workspace layout / crate list** — new or removed top-level directory,
   crate, or app → update the layout block in both `CLAUDE.md` and
   `README.md`, and the "planned, does not exist yet" note if a planned
   crate (`journal-sync`, `journal-dropbox`) now exists.
2. **Build/test/lint commands** — new script, changed command, new check
   added to CI → update the command block in `CLAUDE.md` and in
   `README.md`'s "Checks CI runs" / per-package `README.md`s.
3. **Security-critical conventions** — any change touching IPC commands,
   key material types, serialization of on-disk formats, or logging →
   re-read `CLAUDE.md`'s "Security-critical conventions" list and the
   design spec's §3 non-goals. If the change is consistent, no doc edit
   needed. If it isn't, this is a contradiction — see below, don't just
   edit the rule away.
4. **Status / roadmap claims** — implementing something previously listed
   as not-yet-implemented → update the "Current status" line in
   `CLAUDE.md`, the "Status" line and Roadmap table in `README.md`, and the
   relevant row(s) in `plans/INDEX.md` (status, notes, branch/PR).
5. **Plan checklists** — a plan file's own checkboxes should reflect what's
   actually done; don't leave a task unchecked once its verification step
   passes, and don't check one off that isn't actually satisfied.
6. **SKILL.md descriptions** — if a workflow file this session relied on
   (another skill, a CI step, a script path) moved or changed behavior,
   check whether any `SKILL.md` still points at the old path/behavior.

## When something contradicts

If the diff and a doc disagree — the doc says X is not implemented but the
code now does it in a way the doc didn't anticipate, the code does
something the security conventions list forbids, a plan is marked `Done`
but the checklist isn't fully checked, etc. — do not pick a side yourself.
Stop and ask the user (`AskUserQuestion` if the conversation supports it,
otherwise ask directly) with the specific doc passage and the specific
code/diff that conflicts with it. Let them decide whether the doc is wrong
and should be updated, or the code is wrong and should change. Only edit
docs unilaterally when the update is a mechanical sync (a new crate added
to a layout diagram, a command that changed) with no judgment call
involved.
