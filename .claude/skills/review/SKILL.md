---
name: review
description: Multi-agent code review for this repo — three independent Opus reviewers (idiomatic Rust, security/secrets/data leaks, general correctness) followed by auditor agents that verify every finding against the code, with results written to .claude/review/. Use when the user asks to "review", "run a review", "audit the code", "review my changes", "review the whole repo", or "review <crate/file/feature>". With no argument it reviews uncommitted changes; "all"/"repo" reviews the whole repository; anything else is treated as the named target (paths, crates, a feature, a commit range).
---

# Multi-agent review

Three reviewers look at the same scope from different angles, each in its own
Opus agent so none of them anchors on another's findings. Every finding then
goes to an auditor agent that tries to disprove it against the actual code.
Only what survives the audit lands in the final report. Every issue carries a
concrete suggested fix — a finding without one is not finished.

You (the orchestrating session) do not review the code yourself. You resolve
the scope, launch the agents, and consolidate their output.

## 1. Resolve the scope

Read the argument the user gave (`$ARGUMENTS`, or the words of the request):

| Argument | Scope | How to collect it |
| --- | --- | --- |
| *(none)*, "changes", "uncommitted", "diff" | Uncommitted work | `git diff HEAD` plus untracked files from `git status --porcelain` (read those whole). If both are empty, say so and stop. |
| "all", "repo", "whole repo", "everything" | Whole repository | `git ls-files`, excluding `src/bindings/`, `Cargo.lock`, `package-lock.json`, `src-tauri/gen/`, `src-tauri/icons/`, and other generated or binary files. |
| a commit range or ref (`main..`, `HEAD~3`, a SHA) | Those commits | `git diff <range>` and `git log --oneline <range>` |
| paths, crate names, or a feature ("crates/sync-engine", "the OAuth flow", "idle lock") | The named thing | Map names to paths yourself (crate `sync-engine` → `crates/sync-engine/`; a feature → grep for it and use CLAUDE.md's architecture section). Confirm the file list only if the mapping is genuinely ambiguous. |

Create the run directory:

```
.claude/review/<YYYY-MM-DD-HHMM>-<slug>/
```

`<slug>` is short: `uncommitted`, `repo`, `crates-sync`, `oauth-flow`. Write
`scope.md` there: the argument as given, the mode, the resolved file list,
and — for diff modes — the full diff saved as `scope.diff` next to it, so every
agent reviews exactly the same thing.

Decide which reviewers apply:

- **Rust idioms** — only if the scope contains `.rs` files or `Cargo.toml`.
  Otherwise skip it and record "no Rust in scope" in `scope.md`.
- **Security** and **General** — always.

## 2. Launch the reviewers — in parallel, one message

Launch each applicable reviewer with the Agent tool in a **single message**
so they run concurrently: `subagent_type: "general-purpose"`,
`model: "opus"`. Each prompt is the **shared preamble** below followed by
that reviewer's **focus** section. Fill in the run directory and output file.

### Shared preamble (every reviewer)

> You are reviewing code in the Journal repo (the current working directory),
> a Tauri v2 journaling app: React/TS frontend, Rust workspace backend under
> `crates/` and `src-tauri/`. Read `CLAUDE.md` first — it states the
> architecture and several deliberate decisions (no data migrations, pinned
> scrypt work factor, snake_case wire types, alpha status). Do not report a
> documented deliberate decision as a defect unless you can show it is
> actually broken. Check `docs/adrs/` before calling a design choice wrong.
>
> Scope: read `<run-dir>/scope.md` (and `<run-dir>/scope.diff` if present).
> In a diff scope, review the changed lines, but read the surrounding code and
> callers — a change can break something outside the hunk. Report issues in
> unchanged code only if the change makes them reachable or worse.
>
> You are read-only: do not edit any project file. You may run read-only
> commands (`git`, `grep`, `cargo check`, `cargo clippy`, `cargo test`,
> `npx tsc --noEmit`) to confirm a suspicion.
>
> Report only issues you can point to in the code, with a concrete failure
> scenario. No style nits a formatter would fix, no "consider adding tests"
> without naming the untested behaviour, no speculative "might be a problem".
> Fewer real findings beat many weak ones. If you find nothing, say so.
>
> Write your report to `<run-dir>/<output-file>` in exactly this format:
>
> ```markdown
> # <Reviewer name> review
>
> Scope: <one line>
>
> ## <PREFIX>-1 — <short title>
>
> - **Severity:** critical | high | medium | low
> - **Location:** `path/to/file.rs:123`
> - **Problem:** what is wrong, in two or three sentences.
> - **Failure scenario:** concrete inputs/state → the wrong outcome.
> - **Suggested fix:** the change to make, as prose plus a code block
>   (a diff or the replacement code). Must be specific enough to apply.
> ```
>
> Number findings `<PREFIX>-1`, `<PREFIX>-2`, … most severe first. End with a
> one-paragraph `## Notes` section for anything you checked and found sound
> that a later reader might otherwise re-check. Reply to me with only the
> file path and the finding count per severity.

### Rust idioms — prefix `RUST`, output `rust.md`

> Focus: idiomatic, maintainable Rust. Look for:
>
> - Error handling: `unwrap`/`expect`/`panic!` on fallible paths reachable at
>   runtime; lost error context; `String` errors where a `thiserror` enum
>   fits; `Box<dyn Error>` leaking across crate APIs; `?` conversions that
>   swallow the cause.
> - Ownership: needless `clone()`, `to_string()`/`to_owned()` where a borrow
>   works, `&String`/`&Vec<T>`/`&Box<T>` parameters instead of `&str`/`&[T]`/
>   `&T`, `Arc<Mutex<_>>` where plain ownership works, holding a lock guard
>   across I/O or an `.await`.
> - API shape: missing or wrong trait impls (`Default`, `From`, `Display`,
>   `AsRef`), newtypes that should exist or that leak their inner type,
>   `pub` that should be `pub(crate)`, builder/constructor misuse, bool
>   parameters that should be enums.
> - Iterators and control flow: manual loops that are a clear `iter` chain,
>   `match` that is `if let`/`let else`/`?`, needless `collect()`.
> - Concurrency: blocking calls on async runtime threads (CLAUDE.md requires
>   the `blocking` helper in `src-tauri` commands), mutex poisoning handling,
>   `Send`/`Sync` bounds.
> - `unsafe`, `#[allow(...)]` that hides a real warning, dead code.
>
> Run `cargo clippy --workspace --all-targets -- -W clippy::pedantic` and use
> the output as leads, not as findings: report only lints that point at a
> real readability or correctness problem, and explain why.

### Security — prefix `SEC`, output `security.md`

> Focus: secrets and user data. This app holds a passphrase, an age X25519
> identity, OAuth refresh/access tokens, and decrypted journal entries.
> Look for:
>
> - **Exposing secrets**: secrets or plaintext in logs (`log::`/`println!`/
>   `dbg!`, the `dropbox_sdk::oauth2` debug target), in `Debug`/`Display`
>   impls, in error messages or `#[error]` strings that reach the UI, in
>   panic messages, in URLs/query strings, in files with loose permissions
>   (the credential file must be 0600), in the synced `entries/` dir, or in
>   the production frontend bundle (the dev fallback and fault hook must not
>   ship — see `make check-no-fallback`).
> - **Keeping secrets in memory**: passphrases, identities, tokens and
>   decrypted entries held longer than needed; no `zeroize`/`secrecy` wrapper
>   where one is cheap; copies made by `clone()`, `format!`, `String`
>   reallocation, or serde; secrets kept after `lock`; the React side keeping
>   the passphrase or plaintext in state, refs, or closures after lock/unmount.
> - **Leaking data**: plaintext written to disk, temp files, or the remote;
>   encrypted-at-rest guarantees bypassed (a code path that stores an entry
>   without the cipher); metadata leaks (titles/ids in blob names or logs);
>   the Tauri IPC surface (`capabilities/default.json`, commands callable
>   while locked), CSP (`tauri.conf.json`, ADR 0011), the OAuth loopback
>   listener (`127.0.0.1:53682`: state/PKCE verification, what it serves, how
>   long it listens), path traversal around `BlobName`, TOCTOU on key/vault
>   creation.
> - Crypto misuse: nonce/key reuse, work factor handling, comparing secrets
>   with non-constant-time equality where it matters.
>
> For each finding state the attacker or accident model (local malware,
> another user on the machine, a malicious web page, a compromised cloud
> account, a crash dump, a log shared in a bug report). Rate severity by what
> is actually exposed and to whom, not by the category name.

### General — prefix `GEN`, output `general.md`

> Focus: a standard code review for correctness. Look for:
>
> - Logic bugs, off-by-one, wrong conditions, unhandled states, races
>   (autosave vs. delete vs. lock, sync passes vs. vault saves).
> - Error handling that reports success for a failed operation, swallows a
>   rejection, or leaves state inconsistent (CLAUDE.md: optimistic updates
>   must roll back on rejection).
> - React: stale closures, effect dependency mistakes (e.g. `loadEntry`
>   identity must stay stable), state updates after unmount, keys.
> - Contract drift between Rust and TypeScript (`src/bindings/` must match —
>   run `make check-bindings` if types changed), and between code and
>   `CLAUDE.md`/ADRs.
> - Missing tests for behaviour the change introduces — name the behaviour
>   and where the test belongs (`cargo test` in the crate, or a Playwright
>   spec in `e2e/`).
> - Simplification: duplicated logic, dead code, an abstraction that does
>   not pay for itself.
>
> Leave Rust idioms and security to the other reviewers unless an issue is
> primarily a correctness bug.

## 3. Audit — in parallel, one message

When all reviewers have returned, launch one auditor per non-empty report,
again in a single message: `subagent_type: "general-purpose"`,
`model: "opus"`. Auditors must not have seen the reviewer's reasoning beyond
the report itself — pass only the paths.

> You are auditing a code review of the Journal repo
> (the current working directory). Read `CLAUDE.md`, then
> `<run-dir>/scope.md` (and `scope.diff` if present), then the review at
> `<run-dir>/<report>.md`.
>
> Your job is to **try to disprove each finding**. For every finding, open
> the cited code and its callers and decide:
>
> - **CONFIRMED** — the problem is real and the failure scenario can happen
>   in this codebase as it stands.
> - **PLAUSIBLE** — the code is as described, but the scenario depends on
>   something you could not verify (a platform behaviour, a future backend).
> - **REJECTED** — the finding is wrong: the cited code does not do that,
>   another layer already prevents it, it contradicts a documented deliberate
>   decision in `CLAUDE.md`/`docs/adrs/`, or it is out of scope.
>
> Then check the **suggested fix**: does it actually fix the problem, compile
> in principle, and avoid breaking something else (callers, tests, the wire
> format, a `CLAUDE.md` invariant)? Mark it `ok`, `needs change` (give the
> corrected fix), or `wrong` (give a replacement). Also correct the severity
> if it is off.
>
> Read-only: do not edit project files. You may run `cargo check`,
> `cargo test`, `npx tsc --noEmit`, `git`, `grep`.
>
> Write `<run-dir>/audit-<report>.md`: for each finding ID, its verdict,
> the one-to-three-sentence evidence (with `file:line`), the fix verdict,
> the final severity, and the corrected fix when you changed it. Reply to me
> with only the file path and the counts per verdict.

## 4. Consolidate

Read the reports and their audits yourself and write `<run-dir>/REPORT.md`:

```markdown
# Review — <scope, one line>

<date> · reviewers: <which ran> · <N> findings raised, <M> confirmed,
<P> plausible, <R> rejected

## Findings

### <ID> — <title> (<final severity>, <CONFIRMED|PLAUSIBLE>)

`path:line`

<problem, one short paragraph>

**Fix:** <the audited fix, including the code block>

…

## Rejected

| ID | Title | Why rejected |
| --- | --- | --- |
```

Rules for consolidation:

- Include only CONFIRMED and PLAUSIBLE findings in **Findings**, most severe
  first, CONFIRMED before PLAUSIBLE at equal severity.
- Use the auditor's severity and fix wherever the auditor changed them.
- Merge duplicates across reviewers (the same line flagged by Security and
  General) into one entry that lists both IDs.
- Keep the Rejected table short but present, so the user can overrule an
  auditor.
- Table separator rows use `| --- |` with spaces (markdownlint MD060).

Then reply to the user with: the path to `REPORT.md`, the counts, and the
titles of the CONFIRMED high/critical findings (one line each). Do not paste
the whole report, and do not start applying fixes unless the user asks.

`.claude/review/` is gitignored — the reports are local working files, not
something to commit.
