---
name: coverage-check
description: Run at the end of any task that added or changed Rust or TS/TSX code. Measures line coverage with `make coverage` (about a minute, about ten lines of output) and reports what the change left untested, then offers to write the missing tests. Skip it for docs- or config-only changes.
---

# Checking that a feature is tested

`make coverage` runs the Rust and Playwright suites under coverage and prints
only the answer: the Rust / web / total percentages, the change in the total
since the last local run, and the **changed lines that no test executed**
(the working tree against `COV_BASE`, default `HEAD`). It takes about a
minute. The full log goes to `coverage/run.log`.

First-time prerequisites: `cargo binstall cargo-llvm-cov`,
`rustup component add llvm-tools-preview --toolchain nightly`,
`npx playwright install chromium`.

## Process

1. Run it before committing. If the feature is already committed, set
   `COV_BASE` to the commit it started from:
   `COV_BASE=$(git merge-base HEAD develop) make coverage`.
2. Run `make coverage` and nothing else with it. Read the ~10 lines it prints.
3. **Clean** (total at or above the floor, no negative delta, `patch` has no
   uncovered lines, no `not in the report` lines): say so in one line and
   stop.
4. **Gaps**: show the `uncovered, changed:` lines, say which deserve a test
   and which do not (unreachable defensive branches, `run()` glue in
   `src-tauri`, the one-shot `backfill.rs`), and **ask before writing tests**.
   After the user agrees and the tests are written, re-run once — not in a
   loop.

## Reading the result

- The floor is 85% on the combined total; Codecov gates it, and the script
  marks `BELOW 85% FLOOR` when the total is under it.
- The delta (`+0.3 vs last run`) is a *local* signal: it compares with the
  previous run on this machine, not with the base commit as Codecov's no-drop
  check does. A fall after several features can hide behind a rise, and a
  second run with no change in between shows `+0.0` and forgets a drop the
  first one reported — so report the delta from the first run after a feature.
- `path: not in the report` on a TS file means no e2e spec ever loads it, so
  the totals cannot show it.
- TS coverage is what Playwright reaches against the browser fallback: a TS
  gap is a missing e2e spec (use the fallback's `?fail=`/`?slow=` hooks for
  failure paths), a Rust gap is a unit test in the crate.
- Rust has no branch coverage: a line that ran one arm of an `if` counts as
  covered.

## Gotchas for new tests

- A new `#[cfg(test)]` module carries
  `#[cfg_attr(coverage_nightly, coverage(off))]`, and its crate root starts
  with `#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]`.
- e2e specs import `test`, `expect` and `Page` from `./coverage`, not
  `@playwright/test`, or their coverage is silently not collected.

## Do not

- Open the HTML reports, read `coverage/run.log` unless the run failed, or run
  with `V=1`: each defeats the token budget.
- Write tests before the user says yes.

## If the run fails

The printed tail is a failing test, a missing tool, or port 5183 in use (a
stray Vite server — stop it and re-run). Report it; do not paper over it and
do not report coverage. If the failing test is one the feature did not touch,
re-run once before calling it a failure: the e2e suite runs `fullyParallel`,
and a flake is not a coverage result.

## Once per feature, by the coordinator

The web half starts Vite on the fixed port 5183 (`--strictPort`) and never
reuses a running server under coverage, so two coverage runs on one machine
collide — including from separate git worktrees. Run this once, after the
work packages are merged into the working tree, never from several parallel
agents at once. That also measures the whole feature together, which is what
`COV_BASE` expects.
