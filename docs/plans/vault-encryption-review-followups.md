# Plan: follow-ups from the `age` vault encryption review

## Status

As of 2026-09-09. Each item's own section carries the detail; this is the
index.

| Item | Status |
| --- | --- |
| P0-0 bridge field names | Done — ts-rs codegen, `7ccc445` |
| P0-1 swallowed save/delete failures | Done — both steps, `2ef93cf` |
| P0-2 logging goes nowhere | Done — `717a2dc`, ADR 0008 `544bf80`; manual `make run` check outstanding |
| P0-3 missing key file creates a new vault | Done — `crates/vault` `open_or_create` |
| P0-4 per-file scrypt | Done — cached X25519 key in `crates/vault` |
| P1-1 commands on the main thread | Done — async commands over `spawn_blocking`; manual `make run` check outstanding |
| P1-2 every unlock failure is "Incorrect passphrase." | Done — backend strings are the copy, `login` rejects |
| P1-3 unconfirmed passphrase on vault creation | Done — confirm field, 12-char minimum |
| P1-4 non-atomic `save_entry` | Done — `FsStore::put` writes and renames |
| P2-1 mutex poisoning | Done — with P1-1 |
| P2-2 `fs::read` aborts the listing | Done — fixed by the ADR 0009 split; guard tests added |
| P2-3 corrupt key file vs wrong passphrase | Open |
| P2-4 `logout` leaves plaintext / races the backend | First half done with P0-1; race guard open |
| P2-5 `setSubmitting(false)` not in a `finally` | Done — with P1-2 |

Landed alongside these, outside the plan: the fault-injection hook and
`e2e/backend-failures.spec.ts` (with P0-1), `make bindings` /
`make check-bindings` / `make build`, and CI gating pushes on `cargo test`,
`tsc`, e2e and binding freshness.

## Context

Code review of `fd3407c` ("feat(vault): encrypt entries at rest with age,
passphrase-derived key") and `84fdc41` (ADR 0006) turned up a set of real
correctness and robustness issues in the new vault path. This plan lists
them in priority order with a concrete fix for each, so they can be picked
up independently.

The item IDs are stable, so two items that a later simplification pass
merged into others are kept as stubs (P2-1, and most of P2-4) rather than
renumbering everything around them.

### Revision: the encryption design is now in scope

The first draft of this plan treated the ADR 0006 design as settled and
listed only follow-ups around it. A second pass — with the project treated
as alpha, and nothing in `CLAUDE.md` or `docs/adrs/` held as fixed —
reopened that, for two reasons found while verifying the plan against the
code:

1. **There is no data to protect.** P0-0 below establishes that
   `save_entry` has never once succeeded under Tauri. No installed build
   has ever written an entry file, so the on-disk format has an installed
   base of zero. This is the cheapest moment the project will ever have to
   change it, and it makes the "no migration" paragraph in `CLAUDE.md`
   moot — there is nothing to migrate.
2. **Per-file scrypt is not just slow, it locks users out.** age picks the
   scrypt work factor by benchmarking the *current machine* on every call,
   and caps what it will accept on decrypt. A file written on a fast
   desktop is undecryptable on a slower phone — reported as "wrong
   passphrase", forever. That is permanent data inaccessibility on a
   project whose charter is desktop + iOS + Android with sync. See P0-4.

So P0-4 (replace per-file scrypt with a cached X25519 key) is now part of
this plan, and ADR 0006 is superseded by ADR 0007. P0-4 changes what
several other items mean; each says so.

### Correction: the on-disk format now has an installed base

Premise 1 above — "there is no data to protect", because `save_entry` had
never once succeeded — was true only up to the moment P0-0 landed. Fixing
P0-0 is precisely what made saves start working, so the premise falsified
itself on contact. As of 2026-09-09 the dev machine holds real encrypted
entries in `~/.local/share/com.fsquirrel.journal/entries/`.

Two consequences for the items still open:

1. **Changing the on-disk format now orphans real entries, silently.**
   `Header`'s single serde impl serves both the wire and the encrypted
   payload, so P0-0's naming decision rewrote the disk format too. An
   entry written minutes earlier under the intermediate `camelCase` no
   longer parses; `list_entries` skips it and logs to a subscriber that
   does not exist (P0-2). This is not a hypothetical — it happened
   *during* P0-0, and the skip is invisible until P0-2 lands.
2. **P0-4 will do it again, wholesale.** It replaces the scrypt-per-file
   format outright, so every entry written before it lands becomes
   undecryptable. Under ADR 0006 that is the entire entry corpus.

The no-migration stance below is **unchanged** — this is still alpha and a
compatibility shim is still the wrong answer. What changed is that wiping
the entries directory is no longer free. Per `CLAUDE.md`, say so and
confirm with the user before a change drops their local data, rather than
discovering it afterwards.

Landing P0-2 before P0-4 is now worth more than its position in the
commit sequence suggests: it is the only thing that makes a silent skip
visible at all.

### Explicitly out of scope

- **Migration of any kind.** Not from pre-encryption plaintext, and not
  from the ADR 0006 scrypt-per-file format either. Neither has an installed
  base (see above). Do not add legacy-format detection or conversion; delete
  the entries directory when the format changes under you in development.
- OS keychain unlock, multi-vault, sync, passphrase recovery.
- A machine-readable error taxonomy across the Tauri commands. An earlier
  draft of P1-2 proposed one; the simpler fix is to make the backend's own
  error strings user-facing and show them verbatim. See P1-2.

---

## How each item is proved

Every item below now carries a **Prove it** block: the test to write
*before* the fix, watch fail, and then watch pass unchanged. The working
rule when one of these is handed to an agent:

1. Write the test named in **Prove it**, run it, and paste the failure.
   A test that passes before the fix has proved nothing — say so and stop
   rather than carrying on to the fix.
2. Implement the fix.
3. Run the same test, unedited. Editing a test to make it pass counts as
   not having fixed the issue.

Two items have no honest failing test — P0-1 step 1 (a behaviour-neutral
move) and P1-4 (atomicity is `rename`'s guarantee, and racing threads can
only ever fail to disprove it). Both say so in their own block and name a
*guard* test instead: green before and after, there to stop a later change
from regressing the fix. Do not dress a guard up as a proof; a few other
items are provable in one half only and say which.

### Where tests go

- `crates/entry`, `crates/vault`, `crates/storage` — `#[cfg(test)] mod
  tests` at the bottom of `src/lib.rs`, as the latter two already do.
  `crates/entry` grew its test module in P0-0, with `serde_json` in
  `[dev-dependencies]`; ts-rs's generated `export_bindings_*` tests run
  alongside them under `--features ts`.
  `crates/storage` installs a capturing `log::Log` for P0-2's test;
  no `tracing-subscriber`, since P0-2 landed on `log`.
- `e2e/` — Playwright against the Vite dev server and the browser
  fallback. New spec files are picked up automatically (`testDir:
  "./e2e"`).
- `src-tauri` — no tests today. P0-2 and P1-1 add the first ones; they run
  under `cargo test -p journal`.

### The fault-injection hook (frontend)

Every frontend item needs a backend failure that the browser fallback
cannot produce today: it swallows errors by construction, and Playwright
never crosses the `invoke` bridge. So the fallback has to be able to fail
on demand. Land this **with P0-1 step 1**, the commit that moves the
fallback into `src/api/entries.ts` anyway:

- Parse the hook once at module load from `window.location.search`, and
  only on the `!isTauri()` path, so shipped builds cannot be steered by a
  URL.
- The app uses `HashRouter`, so the query string goes *before* the hash:
  `page.goto("/?fail=save")`, not `/#/?fail=save`.

| Param | Effect |
| --- | --- |
| `?vault=new` | `hasVault()` resolves `false`, giving the create-vault UI |
| `?fail=<ops>` | comma-separated ops; `unlock` `list` `get` `save` `delete` |
| `?failMessage=<text>` | the rejection message; default `"Injected fault."` |
| `?slow=<ops>` | the named ops resolve after a fixed 1200 ms |

Four params and one delay constant — this is test scaffolding, not a
feature. Without the hook, everything below P0-1 is manual-only.

### Optional: command-level tests for `src-tauri`

> **Dependency landed 2026-09-09** with P0-2: `src-tauri`'s
> `[dev-dependencies]` now carries `tauri = { workspace = true, features =
> ["test"] }`, and `logging_is_installed` builds a mock app with it. The
> harness is therefore available at no further cost to P0-3 and P1-2 — the
> command-layer tests themselves are still unwritten.

`tauri` 2.11 ships a `test` feature (`tauri::test::{mock_builder,
mock_context, noop_assets, get_ipc_response, INVOKE_KEY}`) that runs real
`#[tauri::command]`s over a mock runtime, with `app.manage(...)` supplying
a `VaultState` pointed at a temp dir. Adding `tauri = { workspace = true,
features = ["test"] }` to `src-tauri`'s `[dev-dependencies]` would let
P0-0, P0-3 and P1-2 be proved at the command layer — over the real
`invoke` wire format — rather than one layer down. Worth doing once. The
module is documented as unstable, so if it fights back, fall back to the
crate-level tests each item names and keep the manual `make run` pass.

---

## P0 — silent data loss / permanent inaccessibility

### P0-0. The Tauri bridge field names don't match — every save fails

> **Landed 2026-09-09.** Resolved in the opposite direction to the one
> proposed below: Rust is now the source of truth and the TypeScript is
> generated from it. See **Fix (landed)**. The diagnosis that follows is
> kept because it is what P0-4's rationale rests on.

`crates/entry/src/lib.rs:41`/`:49` (`#[serde(rename_all = "snake_case")]`)
against `src/types.ts:4` (`updatedAt`), with no conversion in
`src/api/entries.ts`.

`Header`/`Entry` serialize `updated_at`; the frontend's `EntryHeader`
declares `updatedAt`. Verified against the real types:

```text
frontend JSON -> Entry: Err("missing field `updated_at`", line: 1, column: 107)
Header -> JSON: {"id":"...","title":"t","updated_at":"2026-09-07T20:17:50Z"}
```

Tauri converts top-level *command argument* names between camelCase and
snake_case, but not nested struct fields, so nothing rescued this. Under
Tauri, before this item landed:

- **Every `save_entry` rejects** with `missing field updated_at` — and the
  empty `catch` described in P0-1 is exactly what hid it. Entry creation
  had never once worked outside the browser fallback.
- `list_entries`/`get_entry` return `updated_at`, so `entry.updatedAt` is
  `undefined` and `formatUpdatedAt` (`src/utils/date.ts:3`) falls through
  to returning `undefined`.

This predates the encryption work (`45806ca`), which is why a review scoped
to `fd3407c` missed it. `npm run test:e2e` cannot catch it either: the
browser fallback never crosses the bridge.

**Fix (landed).** The first attempt did what this item originally said —
flip `Header` to `camelCase` — and that works, but it points the coupling
the wrong way: it bends the Rust wire format to match hand-written
TypeScript, and leaves the two free to drift again. What shipped instead
generates the TypeScript from Rust:

- **`Header` keeps `#[serde(rename_all = "snake_case")]`.** Unchanged from
  before this plan, and now a deliberate no-op that documents the
  convention — Rust's field names already are snake_case. **Do not change
  this to `camelCase`.** The wire format is `updated_at` on both sides.
- `Entry`'s `rename_all` was deleted: its fields are `header`
  (`#[serde(flatten)]`) and `markdown`, so it never renamed anything.
- `crates/entry` derives `ts_rs::TS` on `Id`/`Header`/`Entry`, emitting
  `src/bindings/*.ts`. `src/types.ts` is now two re-exports aliasing
  `Header`→`EntryHeader` and `Entry`→`JournalEntry`, so every existing
  `from "../types"` import is untouched.
- The frontend reads `entry.updated_at` at all seven call sites. Purely
  local names stay camelCase (`formatUpdatedAt`'s own parameter).
- ts-rs sits behind `crates/entry`'s off-by-default `ts` feature, so the
  codegen crate is absent from the shipped binary (`cargo tree -p
  journal-entry` finds zero; `--features ts` finds two).
- `make bindings` regenerates. `make check-bindings` regenerates and fails
  if the result differs from what is committed — without it a stale
  binding leaves `tsc` green while the bridge is broken.

**What this changes for every later item.** `tsc` now catches bridge
field-name mismatches at every call site, which corrects the claim in
**Verification** at the bottom of this plan. Any item that renames a field
on a type crossing `invoke` must run `make bindings` in the same commit.

Note what is still coupled: `Header` has one serde impl serving both the
`invoke` wire format and the encrypted on-disk payload, so a field rename
still silently changes the disk format. This stopped being theoretical
during this very item — see **Correction: the on-disk format now has an
installed base** above. A separate on-disk DTO is the eventual answer.

**Order.** This must land first. It is the evidence for the P0-4 rewrite,
and P0-1 turns its silent failure into an error dialog on every save.

**Verify.** `make run`, unlock, write an entry, save, relaunch, and confirm
it is still listed with a real timestamp rather than `undefined`.

Partially observed: entry files were written under Tauri on 2026-09-09 for
the first time, which is the save path working. The relaunch-and-read-back
half is still outstanding, and is now entangled with the orphaned-entry
problem described in the correction above.

**Prove it (landed).** `crates/entry/src/lib.rs` gained a `#[cfg(test)] mod
tests`, with `serde_json` in its `[dev-dependencies]`. Two tests, one per
direction, both watched red before the fix. They assert the **snake_case**
wire format that shipped:

1. `frontend_json_deserializes_into_entry` — hand-write the exact object
   the frontend sends and feed it to `serde_json::from_str::<Entry>`:

   ```rust
   // Mirrors what the frontend sends over `invoke`. Keep this literal
   // hand-written: building it from the Rust type would only test serde
   // against itself. Note the `r##"…"##` — the `"# h"` in the markdown
   // body closes a plain `r#"…"#` and will not compile.
   let json = r##"{"id":"5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01","title":"t",
                   "updated_at":"2026-09-07T20:17:50Z","markdown":"# h"}"##;
   let entry: Entry = serde_json::from_str(json).unwrap();
   assert_eq!(entry.markdown, "# h");
   ```

   It was red with ``missing field `updated_at` `` back when the frontend
   still sent `updatedAt` — verbatim the rejection every `save_entry` was
   getting.
2. `header_serializes_snake_case_keys` — assert `serde_json::to_value(&header)`
   has a string under `updated_at` and **no** `updatedAt` key. That is the
   `list_entries`/`get_entry` direction.

The stronger guard is the codegen rather than either test: reverting
`Header` to `camelCase` and regenerating produces seven `tsc` errors naming
every call site, verified in both directions. `npm run test:e2e` still
cannot catch any of this — a green e2e run is not evidence here — but
`tsc` now can.

### P0-1. Save and delete failures are swallowed while the UI reports success

> **Landed 2026-09-09**, both steps, as written. Two notes for later items:
>
> - Fixing this exposed a latent bug: `loadEntry` was rebuilt on every
>   `entries` change, and both `EditorPage` and `PreviewPage` key a
>   `useEffect` on it, so the optimistic `setEntries` re-ran the editor's
>   load effect and wiped the draft. It is now a `useCallback` with an
>   empty dependency list. Keep it stable.
> - `listEntries` rejecting still has no UI surface. The context logs it
>   with `console.error` rather than swallowing it, but under Tauri a
>   failed listing still renders as "No entries yet." That is not part of
>   this item; fold it into P1-2's error surface if it is worth fixing.

`src/context/AppStateContext.tsx:103` and `:110`.

`upsertEntry` optimistically updates React state and then calls
`saveEntryBackend(entry).catch(() => {})`; `removeEntry` does the same with
`deleteEntryBackend`. The empty catch was tolerable when the only failure
mode was "not running inside Tauri", but `save_entry` can now genuinely
fail: `"vault is locked"` (`src-tauri/src/lib.rs:19`), an `age` encrypt
error, or a disk error. `EditorPage.handleSave`
(`src/pages/EditorPage.tsx:57`) navigates straight to the preview, so the
user sees their entry rendered as saved while nothing reached disk — and it
is gone on next launch. A failed delete symmetrically removes the row from
the UI while the file survives, so the entry reappears after a restart.

`loadEntry` (`src/context/AppStateContext.tsx:89`-`:92`) masks backend
errors the same way: its `.catch(() => offlineEntries.current.get(id))`
means a `"vault is locked"` or decrypt failure under Tauri silently renders
whatever is in the offline map, including the hardcoded seed entries.

**Fix, step 1: move the browser fallback into `src/api/entries.ts`.**
This is the structural change the rest of the frontend items depend on, so
do it early — right after P0-0 — in its own commit, with no behaviour
change intended.

Today `AppStateContext` is the thing that knows about both worlds: it holds
`seedEntries`, an `offlineEntries` ref that is *simultaneously* the
browser-fallback store and a read-through cache under Tauri
(`getEntry(id).then((entry) => entry ?? offlineEntries.current.get(id))`,
`src/context/AppStateContext.tsx:88`), and an `isTauri()` branch in
`login`. Every fix below would otherwise add another `isTauri()` guard next
to it.

Instead, make `src/api/entries.ts` the single place that knows whether a
backend exists:

- When `isTauri()` is false, the module serves the seeded in-memory map —
  this is the expected path for `npm run dev` and the Playwright tests, and
  errors stay swallowed there. `hasVault()` must return an explicit value
  on this path (`true`, matching today's effective behaviour); see P1-3 for
  why that value needs to be controllable.
- When `isTauri()` is true, every call goes to `invoke` and rejections
  propagate untouched.

`AppStateContext` then loses `seedEntries`, `offlineEntries`, and every
`isTauri()` branch, and initialises `entries` to `[]` unconditionally. It
treats any rejection as a real error, because by then it is one.
`LoginPage`'s `.catch(() => setVaultExists(true))` becomes dead and can go.

**Fix, step 2: propagate the failures.**

- Make `upsertEntry` and `removeEntry` return a `Promise<void>` that
  rejects. Update the context type too: `upsertEntry` is declared
  `=> void` (`src/context/AppStateContext.tsx:40`).
- `EditorPage.handleSave` becomes `async`, awaits `upsertEntry`, and only
  navigates on success; on failure it stays on the editor (content intact)
  and shows an MUI `Alert`/`Snackbar` with the backend error string.
- `PreviewPage`'s delete (`src/pages/PreviewPage.tsx:69`) does the same:
  re-add the row and surface the error rather than silently diverging from
  disk. `PreviewPage.handleDelete` (`src/pages/PreviewPage.tsx:66`) also
  sets `deleting` with no `finally`, so a now-rejecting `removeEntry`
  leaves the dialog's Cancel and Delete buttons disabled forever — the same
  defect as P2-5, so fix it here.

**Verify.** With `make run`: unlock, open the editor, then trigger a
backend failure (easiest: temporarily make `save_entry` return `Err` in
`src-tauri/src/lib.rs`) and confirm the editor keeps the draft and shows an
error instead of navigating away. Then `npm run dev` and confirm the seeded
browser path still saves, deletes and lists as before.

**Prove it.**

*Step 1 is a guard, not a proof.* It is behaviour-neutral by definition,
so nothing can go red for it. Run `npm run test:e2e` before and after:
five green, with **zero edits to `e2e/entries.spec.ts`**. Needing to touch
the suite means the move changed browser behaviour and is the signal to
stop and rework it. The fault-injection hook lands in this commit and gets
its coverage from the tests in step 2, P1-2, P1-3 and P2-4.

*Step 2* gets a new `e2e/backend-failures.spec.ts`, three tests:

1. `a failed save keeps the user in the editor` —
   `page.goto("/?fail=save&failMessage=Disk+is+full.")`, log in, **New
   entry**, fill title and body, **Save**. Assert the URL still ends
   `#/entries/new/edit`, the markdown textarea still holds what was typed,
   and `Disk is full.` is visible. Red today: the app navigates to the
   preview and renders the entry as though it had been saved.
2. `a failed delete keeps the row and re-enables the dialog` —
   `?fail=delete&failMessage=Disk+is+full.`, open *Project ideas*,
   **Delete**, confirm. Assert the message is visible, *Project ideas* is
   still in the list, and the dialog's **Cancel** button is enabled. Red
   twice over: today the row vanishes, and once `removeEntry` rejects,
   `deleting` never resets — that second assertion is P2-5's defect living
   in `PreviewPage`, which is why it is fixed here.
3. `a failed load does not render stale content` — `?fail=get`, open an
   entry from the list. Assert the injected message is shown and the entry
   body is not. Red today: `loadEntry`'s `.catch` quietly serves whatever
   the offline map holds, including seed data.

Test 3 pins a decision the fix has to make: a failed `get_entry` surfaces
the error rather than falling through to "Entry not found." Decide it
before writing the test, not by editing the test afterwards.

### P0-2. `tracing::warn!` output goes nowhere — the app is blind to skipped entries

> **Landed 2026-09-09.** The open question below resolved to
> **`tauri-plugin-log`**, so the workspace now uses `log` rather than
> `tracing` and there is no future migration when mobile matters. See
> **Fix (landed)**. One thing the plan did not anticipate: turning the
> logger on exposed a reentrancy deadlock, written up there too.

`crates/storage/src/lib.rs:59`; no subscriber anywhere
(`grep -rn tracing src-tauri crates Cargo.toml` finds only the workspace
dependency, the `journal-storage` dependency, and this one call site).

`tracing` macros short-circuit when no subscriber is installed — the
`error = %e` field is not even evaluated. So the "skip and log" behaviour
in `list_entries` is really just "skip". Corrupt or undecryptable entries
vanish with zero output in every build, and `list_entries` returning
`Ok(vec![])` is indistinguishable from an empty vault. This also blocks
diagnosing every other issue in this document.

There is now a concrete instance to diagnose: P0-0's naming change left at
least one real entry file on the dev machine that no longer parses, and
it is being skipped with no output. That entry is the ready-made fixture
for this item's **Verify** step — no need to corrupt a file by hand.

**Fix (landed).** `tauri-plugin-log`, not `tracing-subscriber` — see the
resolved open question below. What shipped:

- `log` replaces `tracing` in `[workspace.dependencies]`; `crates/storage`
  swaps its one call site to `log::warn!` and drops the `tracing` dep.
  Nothing else in the workspace used `tracing`.
- `src-tauri/src/lib.rs` grows `logging_plugin()`, registered first in the
  builder so it is installed before anything else can log. It targets
  `TargetKind::Stderr` at `LevelFilter::Info`. The plugin routes
  `Stdout`/`Stderr` to `android_logger` on Android and oslog on iOS, so all
  three target platforms get output from the one registration.
- `src-tauri` gains `tauri = { workspace = true, features = ["test"] }` as a
  dev-dependency. This is the "Optional: command-level tests" section's mock
  runtime, pulled in early: the logger is installed by the *plugin's setup
  hook*, so nothing short of building an app can prove it runs.

**Deadlock found while landing this.** `age`'s error `Display` lazily loads
its Fluent i18n bundle on first use, and logs (`"Attempting to load language
file: \"en-US/age.ftl\""`) while doing it. Formatting that error *inside*
`log::warn!`'s arguments therefore re-enters the logger from inside the
logger — and any sink that holds a lock across formatting its own record
deadlocks on the spot. This is not hypothetical: it hung the new
`journal-storage` test, and fern's file target (what `TargetKind::LogDir`
would use) holds exactly that kind of lock, so it would have been a real
hang in the app the first time an entry failed to decrypt. The call site
now renders the error with `e.to_string()` *before* the `warn!`, which keeps
the reentrant records outside it. Keep it that way, and do not move a
`{e}`-style argument back inside a logging macro anywhere that `age` errors
are involved.

**Resolved open question — `tauri-plugin-log` instead?** Yes. `fmt` writes to
stderr, which goes nowhere a developer will see on Android or iOS, so
`tracing-subscriber` would have left two of the project's three target
platforms blind and booked a migration for later. The cost named below —
that it is a `log`-crate sink — was one line, exactly as predicted, because
there was exactly one call site.

The one thing given up: `tracing-subscriber`'s `env-filter` reads `RUST_LOG`
for free, and the plugin does not. The level is fixed at `Info` in
`logging_plugin()`. If per-module control is wanted later, `Builder::level`
and `Builder::level_for` are the hooks.

**Verify.** *Still outstanding — the automated half is done, the manual
half is not.* Unlock a vault, corrupt one entry file with
`printf garbage > <entries dir>/<uuid>.json`, relaunch, and confirm a
`WARN` line naming the file appears on stderr while the rest of the list
loads. The pre-existing undecryptable entry from P0-0 works as the fixture.

**Prove it (done).** Both tests live in the tree and both were watched
fail before the fix:

- `src-tauri/src/lib.rs` — `logging_is_installed` (`cargo test -p journal`).
  Asserts `log::max_level()` is `Off` before, and not `Off` after building a
  mock app with `logging_plugin()`. `log::max_level()` is precisely the
  switch that makes `log::warn!` compile down to nothing, so this asserts the
  defect rather than the wiring. Red before the fix as a compile error —
  neither `logging_plugin` nor the `log` dependency existed.
- `crates/storage/src/lib.rs` — `list_entries_logs_the_file_it_skipped`
  (`cargo test -p journal-storage`). Installs a capturing `log::Log`, drops a
  corrupt `<uuid>.json` next to a good entry, and asserts the captured output
  names the corrupt file. The plan expected this to be a guard that could not
  go red; under `tauri-plugin-log` it was a real proof, failing with
  `no log record named the skipped file; captured: ""` — nothing was emitted
  at all, which is the defect stated exactly.

  Its `CapturingLogger` formats each record *before* taking its mutex, for
  the reentrancy reason above. Do not fold that back into one statement.

`cargo test --workspace` is green, and CI already runs it, so both are
covered without a workflow change.

### P0-3. A missing key file silently creates a brand-new vault

> **Landed 2026-09-09.** `open_or_create` is in `crates/vault`, `unlock`
> has no branch left, and `create_vault`/`unlock_vault` are private. See
> **Prove it (done)**. The error copy is worded for the ADR 0006 world
> (the key file is a canary, so a backup restores access); P0-4 must
> reword it, since under a cached X25519 key the file holds the only copy.
> Note also that `LoginPage` still shows `"Incorrect passphrase."` for
> every rejection (`src/pages/LoginPage.tsx:50`), so the new message does
> not reach the user until P1-2 lands.

`src-tauri/src/lib.rs:32`.

`unlock` branches purely on `journal_vault::has_vault`, i.e. on whether the
key file exists. If it is lost but the entry files remain — partial backup
restore, a sync conflict, a user tidying dotfiles — the next login takes
the `create_vault` branch, writes a fresh key file under whatever
passphrase was typed, and every existing entry becomes permanently
undecryptable and silently skipped. The user is shown the cheerful "Create
a passphrase" copy while their journal is being orphaned.

**Fix — collapse the branch instead of guarding it.** The bug exists
because the decision "create or open?" lives in the command layer
(`src-tauri/src/lib.rs:32`), where it is one `if` away from being wrong.
Adding a second condition to that `if` leaves the decision in the same
place. Move it into the crate instead:

```rust
// journal-vault
pub fn open_or_create(dir: &Path, passphrase: &SecretString)
    -> Result<Vault, VaultError>;
```

It creates a vault only when the directory holds neither a key file nor any
entry file, opens it when the key file is present, and otherwise returns
`VaultError::MissingKeyFile`. `unlock` then has no branch to get wrong, and
`create_vault`/`unlock_vault` stop being public. `has_vault` stays public
purely so `LoginPage` can pick between "Create a passphrase" and "Sign in"
copy — a display concern, where being wrong is cosmetic.

The entry-file test must be `is_file()`, not merely the `.json` extension:
P2-2 establishes that a *directory* named `<uuid>.json` is reachable, and
counting one would permanently block vault creation with an error message
about restoring a backup that does not exist.

Give the variant an `#[error("…")]` message the user can act on, since P1-2
shows that string verbatim.

**Interaction with P0-4.** Under ADR 0006 the key file is a canary and the
passphrase alone can regenerate access, so "restore `.vault-canary` from a
backup" is sound advice. Under P0-4 the file holds the only copy of the
X25519 secret key, so losing it is unrecoverable even with the correct
passphrase. That makes this check *more* important and the message more
honest — word it for the P0-4 world if P0-4 has landed.

**Verify.** Unit test in `crates/vault`: create a vault, write a dummy
`*.json` entry, delete the key file, assert `open_or_create` errors with
`MissingKeyFile` and that no new key file was written. Add a second case
with a *directory* named `<uuid>.json` and assert vault creation still
proceeds.

**Prove it.** In `crates/vault`, one throwaway followed by two real tests.

First, put the bug on record with today's API. This one *passes* — it is a
characterization test showing exactly what the command layer does once the
key file is gone:

```rust
create_vault(&dir, &passphrase("right")).unwrap();
let entry = encrypt(&passphrase("right"), b"{}").unwrap();
fs::write(dir.join("5c1f7c9e-6e8b-4a2f-9c1a-1f6b9b6b0a01.json"), &entry).unwrap();
fs::remove_file(dir.join(".vault-canary")).unwrap();

assert!(!has_vault(&dir));                       // src-tauri:32 takes the
create_vault(&dir, &passphrase("typo")).unwrap();// create branch, silently
assert!(decrypt(&passphrase("typo"), &entry).is_err()); // orphaning the entry
```

Then delete it and write the pair that has to go green:

1. `open_or_create_refuses_when_the_key_file_is_missing_but_entries_remain`
   — same setup; assert the error is `VaultError::MissingKeyFile` **and**
   that no key file was written (`assert!(!has_vault(&dir))` after the
   call). The second assertion is the one that matters: an error that
   still writes a key file has orphaned the entries anyway.
2. `a_directory_named_like_an_entry_does_not_block_creation` — a
   directory (not a file) named `<uuid>.json` in an otherwise empty dir;
   assert `open_or_create` creates the vault and succeeds. Red against the
   naive `extension() == "json"` test, which is the trap P2-2 documents.

Both are red by failing to compile until `open_or_create` exists; the
characterization test above is what proves the behaviour underneath them,
so run it and paste its result before deleting it. Add a third once P1-2
lands: `assert_eq!(VaultError::MissingKeyFile.to_string(), "<the copy you
chose>")` — P1-2 shows that string to the user verbatim, which makes it
behaviour rather than a comment.

**Prove it (done).** All three ran as written.

- The characterization test **passed** with the pre-fix API, which is the
  defect stated exactly: with `.vault-canary` deleted, `has_vault` is
  `false`, `create_vault` succeeds under the typo'd passphrase, and the
  surviving entry no longer decrypts. It was then deleted, as planned.
- Both real tests were red by failing to compile — `cannot find function
  open_or_create`, `no variant ... named MissingKeyFile` — and both went
  green unedited after the fix. `cargo test --workspace` is green.

**Fix (landed).** As proposed, with two notes:

- `open_or_create` returns `Result<(), VaultError>`, not `Result<Vault,
  _>`. There is no `Vault` handle under ADR 0006 — `Storage` holds the
  `SecretString` and calls `journal_vault::{encrypt,decrypt}` per file, so
  a handle would carry nothing. P0-4 is what gives it one.
- `encrypt`/`decrypt` stay public; `journal-storage` is built on them.
  Only the create/open decision moved.

The third test is still outstanding, waiting on P1-2 as the item says.

### P0-4. Per-file scrypt locks users out across devices

> **Landed 2026-09-09.** ADR 0007
> (`docs/adrs/0007-cached-x25519-vault-key.md`) came first (`a0768f8`);
> the crate change followed. `crates/vault` now exposes a `Vault` — a
> directory plus the X25519 key its entries are encrypted to, sealed into
> `.vault-key` under one pinned scrypt operation at unlock. `Vault::create`
> and `Vault::unlock` are private behind `Vault::open_or_create`, and
> `Storage::new` takes the vault alone, asking it for its directory
> instead of holding a second copy of the path. All four **Prove it** tests below ran red first and green
> unedited after — see **Proved** at the end of this item. `CLAUDE.md`'s
> `crates/vault` paragraph was rewritten in the same commit, and ADR 0007
> amended with the pinned write factor that test 4 forced.

`crates/vault/src/lib.rs:55`-`:66`; ADR 0006.

Three defects, one root cause. `encrypt`/`decrypt` construct an
`age::scrypt::Recipient`/`Identity` per call, and age's
`target_scrypt_work_factor()`
(`age-0.12.1/src/native/scrypt.rs:36`) picks `log_n` by benchmarking the
current machine to hit ~1 second, storing it in each file's header.
`Identity::new` then accepts at most `target + 4`.

1. **Cross-device permanent lockout.** A file written on a fast desktop
   carries a higher `log_n` than a phone will accept, so it fails to
   decrypt there. `unlock_vault` (`crates/vault/src/lib.rs:42`) maps *every*
   decrypt error to `WrongPassphrase`, so the user is told their passphrase
   is wrong — forever, on a device where it is correct. With sync and
   iOS/Android both on the charter, this is the reason this item is P0 and
   not a performance ticket.
2. **Attacker-controlled work on the decrypt path.** `log_n` is read from
   the file header, and age's own doc comment on `set_max_work_factor`
   warns that large values "can allow attempted decryption of a malicious
   file that takes hours and tens of GiB of RAM". `list_entries` reads
   every `*.json` in the directory, and once sync lands those files are
   semi-untrusted.
3. **Cost.** Roughly 0.7–1.5s per operation (measured in release), so
   `list_entries` over N entries costs ~1s × N. Optimising around it does
   not work: at `log_n ≈ 18`, scrypt's memory is `128·r·N` = 128·8·2^18 ≈
   256 MiB *per concurrent operation*, so eight-way parallelism is ~2 GB
   resident and an OOM kill on mobile. Any parallelism must be bounded to
   2–3, capping the win at ~3×.

**Fix: one scrypt at unlock, X25519 per entry.** ADR 0006 deferred this,
reasoning that it "needs its own spike to confirm how to construct an
`age::x25519::Identity` from raw derived bytes (likely via bech32-encoding
… not a clean, verified path in `age`'s public API today)". That premise is
wrong — no raw-bytes construction is needed. Generate the identity and
store its own serialized form:

- At vault creation: `age::x25519::Identity::generate()`, then seal
  `identity.to_string()` (a `SecretString` holding `AGE-SECRET-KEY-1…`)
  under the passphrase with the existing scrypt `encrypt`, and write it to
  the key file. This file replaces `.vault-canary` and keeps exactly the
  same wrong-passphrase signal: the wrong passphrase fails to decrypt it.
- At unlock: one scrypt `decrypt` of the key file, then
  `age::x25519::Identity::from_str` on the result. Hold the parsed identity
  (and `to_public()`) for the session in place of today's `SecretString`.
- Per entry: `age::encrypt(&recipient, …)` / `age::decrypt(&identity, …)`.

`generate`, `to_public`, `to_string` and `FromStr` are all public API
(`age-0.12.1/src/native/x25519.rs:36,69,75,88,158`). Measured end to end in
release:

```text
scrypt seal of key file:  1.487s
scrypt open of key file:  1.118s
x25519 encrypt entry:     256µs
x25519 decrypt entry:     169µs
```

That is ~5000× per entry, and it removes defects 1 and 3 outright: the only
scrypt stanza left is the key file, which is per-device-restorable from the
passphrase, and nothing else carries a work factor at all.

Correction, forced by the fourth test under **Prove it** below: "per-device-
restorable from the passphrase" is wrong under this design. The key file
holds the only copy of the key, and it inherits the same benchmarked work
factor, so a desktop-written key file can be undecryptable on a phone —
defect 1 all over again, on the one file that matters most. Pin its work
factor on write and cap it on read; both are one line.

For defect 2, still call `set_max_work_factor` explicitly on the key file's
`Identity` with a deliberate cap rather than inheriting age's generous
`target + 4`, so a tampered key file cannot burn hours and gigabytes.

**This now destroys real entries.** When this plan was written the format
had no installed base; since P0-0 it does. Every entry written under
ADR 0006 becomes undecryptable the moment this lands. Confirm the wipe
with the user first, and land P0-2 before this so the skipped files are
at least visible. See **Correction: the on-disk format now has an
installed base** at the top.

**Tradeoff, to be stated in the ADR and the login UI.** The key file
becomes the only copy of the key: losing it is unrecoverable even with the
correct passphrase, where today the passphrase alone suffices. This is the
standard bargain for cached-key designs, it is what makes P0-3 matter, and
it raises the value of backing up one small file.

**What this changes elsewhere.** P1-1 shrinks to "get the work off the main
thread": no parallelism, no per-entry progress UI, and — since entry
operations drop to sub-millisecond — no spinners anywhere except `unlock`,
which `LoginPage` already covers with `submitting`. The `~1s per call`
warnings in `journal-vault`'s doc comments (`crates/vault/src/lib.rs:51`,
`:62`) become true only of the key-file path.

**Verify.** Unit tests in `crates/vault`: seal/open round trip; wrong
passphrase fails at the key file; an entry encrypted to the recipient
decrypts with an identity re-parsed from a re-opened key file. Then
`cargo test -p journal-storage --release` should drop from ~7.6s to well
under a second, which is itself the regression test for the cost claim.

**Prove it.** Four tests in `crates/vault`, and a stopwatch. The first is
the whole item:

1. `an_entry_written_here_opens_on_a_lower_powered_device`. age caps the
   work factor it will *accept* at `target + 4`, where `target` is
   benchmarked per device (`age-0.12.1/src/native/scrypt.rs:36`, `:211`),
   and returns `DecryptError::ExcessiveWork` past the cap. That makes the
   phone simulatable exactly:

   ```rust
   let ciphertext = /* whatever the vault's entry-encrypt path produces */;
   let mut phone = age::scrypt::Identity::new(passphrase("hunter2"));
   phone.set_max_work_factor(10);            // a much slower device
   assert!(!matches!(
       age::decrypt(&phone, &ciphertext),
       Err(age::DecryptError::ExcessiveWork { .. })
   ));
   ```

   Red today: entries carry a `log_n` near 18, so this is `ExcessiveWork`
   — the permanent cross-device lockout, reported to the user as "wrong
   passphrase". It goes green after P0-4 for a different reason (the entry
   has no scrypt stanza at all), which is why the assertion is *not
   `ExcessiveWork`* rather than `is_ok`.
2. `entry_files_carry_no_scrypt_stanza` — assert the entry ciphertext does
   not contain the bytes `-> scrypt`. age headers are ASCII, so this reads
   the property straight off the file and cannot pass by accident. This is
   the one that stays meaningful after the rewrite.
3. `key_file_round_trips_and_rejects_the_wrong_passphrase` — seal, open,
   `age::x25519::Identity::from_str` the result, and assert an entry
   encrypted to `to_public()` decrypts under an identity re-parsed from a
   *re-opened* key file, not the in-memory one. A wrong passphrase must
   fail at the key file as `WrongPassphrase`.
4. `the_key_file_is_portable_across_devices` — **this test will change the
   fix.** The key file is still scrypt-sealed, so a desktop writing
   `log_n = 19` produces a key file a phone rejects, and under P0-4 that
   is unrecoverable rather than merely annoying, because the file holds
   the only copy of the key. Assert it the same way as test 1, against the
   key file. Passing it means pinning the write side with
   `age::scrypt::Recipient::set_work_factor(log_n)` to a fixed constant
   instead of letting age benchmark, and setting the read cap deliberately
   above it. One line each, both public API
   (`age-0.12.1/src/native/scrypt.rs:110`-`:133`, `:211`).

Cost (defect 3) is proved by the clock, not an assertion: record
`cargo test -p journal-storage --release` wall time before (~7.6s) and
after (expect well under a second) and put both numbers in the PR. If it
should be enforced, add one `#[ignore]`d test that saves five entries and
lists them inside a 1s budget — ignored, so a slow CI box cannot fail the
build on a timing bound.

Defect 2 (attacker-controlled work) has no cheap failing test: a file with
an absurd `log_n` means either doing the work or forging a header whose
MAC check happens *after* the scrypt cost is paid. Test 4's explicit cap
is the coverage; say so in ADR 0007 rather than pretending otherwise.

**Proved (2026-09-09).** All four ran as written; only the one-line
`entry_ciphertext` helper — the "whatever the vault's entry-encrypt path
produces" placeholder above — changed with the API. No assertion was
edited.

- Tests 1 and 2 were red against the pre-fix API for exactly the stated
  reasons: `entry ciphertext carries an scrypt stanza: … -> scrypt … 18`,
  and `entry rejected for its work factor on a lower-powered device`.
- Tests 3 and 4 were red by failing to compile — `cannot find function
  key_path`, `no method named encrypt found for unit type ()`,
  `cannot find value KEY_FILE_WORK_FACTOR` — and went green after the fix.
- Test 4 changed the fix as predicted: the key file's work factor is now
  pinned on write (`set_work_factor(KEY_FILE_WORK_FACTOR)`, 18) and capped
  on read (`set_max_work_factor(KEY_FILE_MAX_WORK_FACTOR)`, 20), with a
  `const _: () = assert!(…)` keeping the two in order.
- Cost, by the clock: `cargo test -p journal-storage --release` went from
  **7.81s** to **0.53s**, and what is left is the single shared
  `open_or_create` the suite now does once. The `#[ignore]`d budget test
  was not added — the two numbers are the evidence, and a wall-clock bound
  in the suite would only be noise on a slow box.
- One thing the plan did not anticipate: pinning the factor made the
  *debug* cost visible, because unoptimized scrypt is ~40x slower — a 0.4s
  unlock became 17s in `cargo test` and `make run`. The root `Cargo.toml`
  now raises `opt-level` for the KDF's crates in the dev profile, which
  brings it to ~0.8s. Without that, pinning a release-calibrated constant
  would have made development unusable.

**Not done here.** A key file that decrypts but does not parse now lands
on a new `VaultError::CorruptKeyFile` rather than being called a wrong
passphrase — P0-4 needed *some* error for it. The rest of P2-3 (matching
on `age::DecryptError` variants so a truncated or zero-length key file is
also distinguished) is untouched and still open.

---

## P1 — app appears broken / misleading to the user

### P1-1. Every command runs on Tauri's main thread

> **Landed 2026-09-16.** Every command is now an `async fn` whose storage
> work runs on `tauri::async_runtime::spawn_blocking`, through one
> `blocking` helper that flattens the join error and the command's own
> error into the bridge's string. `VaultState` holds the store outside the
> mutex — `has_vault` takes no lock at all — and `Mutex<Option<Arc<Vault>>>`
> inside it, reached through `lock_state`/`vault`; the `Arc` handle means
> the guard is dropped before any crypto runs. `lock_state` recovers the
> guard from poisoning, which is all of P2-1. Names differ from the sketch
> below (`vault`, not `storage`, and `dir` became the store) because the
> crate split in ADR 0009 landed in between. No frontend change, as the
> item says. The **Verify** pass below is still outstanding — it needs a
> window.

`src-tauri/src/lib.rs:21`, `:27`, `:42`, `:49` and the remaining commands.

Non-`async` `#[tauri::command]` functions execute on the main thread, so
every command freezes the entire webview for its duration — and
`LoginPage`'s `submitting` disabled-button state cannot even repaint while
Rust holds the thread, so the user sees a dead window rather than a
spinner. After P0-4 the only expensive command is `unlock` (~1s), but file
I/O still has no business on the UI thread.

**Fix.**

- Restructure `VaultState` so the lock covers only what actually changes.
  `dir` is immutable after `setup` — `has_vault` currently takes the mutex
  for no reason at all:

  ```rust
  struct VaultState {
      dir: PathBuf,
      storage: Mutex<Option<Arc<Storage>>>,
  }
  ```

- Add one helper that every CRUD command shares, doing the lock, the clone
  and the poison recovery in one place:

  ```rust
  fn storage(state: &VaultState) -> Result<Arc<Storage>, String> {
      state
          .storage
          .lock()
          .unwrap_or_else(|e| e.into_inner())
          .clone()
          .ok_or_else(|| LOCKED_ERROR.to_string())
  }
  ```

  Cloning the `Arc` and dropping the guard *before* the crypto keeps a slow
  command from blocking every other one, including `lock` (i.e. logout).
  The `unwrap_or_else(|e| e.into_inner())` is the whole of what used to be
  P2-1.
- Make `has_vault`, `unlock`, `lock`, `list_entries`, `get_entry`,
  `save_entry` and `delete_entry` `async fn`s whose bodies call
  `tauri::async_runtime::spawn_blocking` around the storage work.
  `State<'_, T>` works in async commands; the `storage` helper above means
  no `MutexGuard` is ever held across an `.await`.

  Not `#[tauri::command(async)]` on a sync fn: that gets the work off the
  main thread but onto a tokio *worker*, which it then blocks. Blocking
  work belongs on the blocking pool. (This is the one place the "async is
  fine" latitude actually changes the design; do not async-ify
  `journal-vault` or `journal-storage` themselves — the async boundary
  belongs at the command layer and the crates stay synchronous.)
- **No new spinners.** An earlier draft of this item added loading states
  to `EntryListPage`, `EditorPage` (`src/pages/EditorPage.tsx:43`) and
  `PreviewPage` (`src/pages/PreviewPage.tsx:52`), all of which `return null`
  while data loads. That was written against the ~1s-per-entry world; after
  P0-4 those operations are sub-millisecond, and a spinner that flashes for
  0.2ms is worse than the blank frame it replaces. The only slow command
  left is `unlock`, which `LoginPage`'s `submitting` already covers — and
  which only ever *looked* broken because the main thread could not repaint
  it. Leave the `return null` branches alone; revisit only if a real
  measurement says otherwise.

**Verify.** `make run` and confirm the window stays responsive during the
~1s unlock — the theme toggle still reacts, and `LoginPage`'s button
actually renders its disabled state instead of the frame freezing.

**Prove it.** The main-thread half is not provable by an automated test in
this repo — nothing here can observe Tauri's event loop — so it keeps the
manual check under **Verify** above. The lock half is testable, and it is
where the real bug (P2-1) lives.

`mutex_poisoning_does_not_wedge_the_vault`, in `src-tauri/src/lib.rs`
(`cargo test -p journal`), written against the extracted `storage` helper:

```rust
let state = VaultState { dir: temp_dir(), storage: Mutex::new(Some(storage)) };
let panicked = std::panic::catch_unwind(AssertUnwindSafe(|| {
    let _guard = state.storage.lock().unwrap();
    panic!("boom");
}));
assert!(panicked.is_err());
assert!(storage(&state).is_ok());   // every later command still works
```

Red before `unwrap_or_else(|e| e.into_inner())`: the helper returns the
`PoisonError` string and every subsequent command fails for the life of
the process. It only reproduces under unwinding — the release profile sets
`panic = "abort"` in the root `Cargo.toml`, which is exactly why this item
is scoped to `make run` and `cargo test`. `cargo test` unwinds by default;
do not add `panic = "abort"` to the test profile to "match production".

The test needs `VaultState` and `storage()` reachable from a test module,
which is the P1-1 restructure itself. With the optional `tauri::test`
harness, add a second case: `list_entries` before any `unlock` returns
exactly `"vault is locked"` across the real IPC boundary.

**Prove it (done).** Both tests landed. `mutex_poisoning_does_not_wedge_the_vault`
was red against the restructure carrying `lock().map_err(...)` — the
poisoned mutex returned the `PoisonError` string from `vault()` and every
later command with it — and green, unedited, once `lock_state` recovered
the guard. `commands_before_unlock_report_the_vault_as_locked` is the
IPC-level case, and is a guard rather than a proof. One wrinkle worth
keeping: app commands are ACL-gated like plugin ones, and a mock context
carries no ACL, so the test has to allow the command on the mock's runtime
authority (`runtime_authority_mut().__allow_command`) or the command is
rejected before it ever reaches the vault. `tauri-build` generates that
allowance for the real binary.

### P1-2. Every unlock failure is reported as "Incorrect passphrase."

> **Landed 2026-09-16.** `login` returns `Promise<void>` and lets the
> rejection through; `LoginPage` renders `String(err)` with no mapping
> table. `KeyError::WrongPassphrase` is now `"Incorrect passphrase."`, and
> `VaultError::Key` is `#[error(transparent)]` so `journal-key`'s copy
> reaches the user without a `"vault error: "` label in front of it (the
> variants live in `journal-key` since ADR 0010's rename, not
> `journal-vault` as written below). Both **Prove it** tests were red
> first: `error_messages_are_user_facing` on
> `"vault error: wrong passphrase"`, and the e2e on the missing message.
> P2-5 landed with it, its assertion observed red on the intermediate
> state.

`src/context/AppStateContext.tsx:77` (the bare `catch { return false; }`)
and `src/pages/LoginPage.tsx:51`.

`login` discards the backend's error string and returns `false`, so
`LoginPage` renders "Incorrect passphrase." for a permissions error on the
key-file write, a full disk during `create_vault`, a poisoned mutex, a
corrupt key file (P2-3), or a missing one (P0-3). The one failure this app
declares unrecoverable is exactly the one it reports for everything.

**Fix.** The backend already produces distinct messages — every
`VaultError` and `StorageError` variant carries a `thiserror`
`#[error("…")]` string, and `unlock` maps them through `e.to_string()`. The
defect is purely that the frontend throws that string away:

- **`login` stops catching.** It sets `authenticated` on success and lets
  the rejection propagate; it returns `Promise<void>`. An intermediate
  draft had it return `Promise<string | null>` — `null` for success, the
  message otherwise — but that is a sentinel return type invented for one
  caller, and `LoginPage` needs a `try`/`catch` regardless for P2-5's
  `finally`. Rejecting reuses it.
- `LoginPage`'s `catch` renders `String(err)` verbatim, with **no** mapping
  table.
- Reword the `#[error]` strings in `journal-vault` so they are the
  user-facing copy: `WrongPassphrase` becomes
  `#[error("Incorrect passphrase.")]`, and P0-3's and P2-3's new variants
  are written the same way.

This also settles a convention for the whole context, which P0-1 step 2
follows: **every `AppStateContext` method rejects on failure; none returns
a status value.** `login`, `upsertEntry` and `removeEntry` then all behave
the same way, and no caller has to remember which one signals failure how.

An earlier draft proposed machine-readable code prefixes across every
command; another proposed mapping the single string `"wrong passphrase"` to
"Incorrect passphrase." in TypeScript. Both are the same coupling — the
frontend branching on a backend string — and the TypeScript one is worse,
because reword the `#[error]` and the mapping silently stops firing with
nothing to grep. Making the Rust strings the single source of truth removes
the branch entirely.

The cost is user-facing copy living in a Rust error enum, which will need
revisiting if i18n ever appears. Accepted deliberately: there is no i18n
today, and P0-3 and P2-3 already assume a well-worded `#[error]` string.

**Prove it.** One Rust test and one e2e, because the fix has a half on
each side of the bridge.

- `crates/vault`, `error_messages_are_user_facing`:
  `assert_eq!(VaultError::WrongPassphrase.to_string(), "Incorrect
  passphrase.")`, and the same for P0-3's `MissingKeyFile` and P2-3's
  corrupt-key-file variant once they exist. Red today
  (`"wrong passphrase"`). This is what makes "the Rust string is the
  single source of truth" enforceable rather than a convention someone
  remembers.
- `e2e/backend-failures.spec.ts`, `an unlock failure shows the backend's
  own message`:
  `page.goto("/?fail=unlock&failMessage=Vault+key+file+is+missing.")`,
  submit the login form, assert `Vault key file is missing.` is visible
  **and** that `Incorrect passphrase.` is not. Red today: the context's
  bare `catch` throws the message away and `LoginPage` prints its own
  constant for every failure.

The negative assertion is load-bearing. Without it, a fix that appends the
real error underneath the old constant would pass.

### P1-3. Vault creation accepts a single unconfirmed passphrase

> **Landed 2026-09-16.** `LoginPage` renders a "Confirm passphrase" field
> on the `isNewVault` path and refuses a mismatch or anything under
> `MIN_PASSPHRASE_LENGTH` (12), stated in the passphrase field's helper
> text; the recovery warning now names the key file. All four cases in
> the new `e2e/create-vault.spec.ts` were red first. The `email` field
> this section asks about was already deleted in `d0fc03a`, so the
> `entries.spec.ts` churn it warned of is not part of this commit.

`src/pages/LoginPage.tsx:38` (only a non-empty check) and `:100` (the
"no way to recover this passphrase" warning).

On the `isNewVault` path the passphrase is taken from one field with no
confirmation and no length minimum, so a typo silently becomes the real
passphrase — and by the page's own warning, that permanently locks the user
out of everything they subsequently write. A single-character passphrase is
also accepted. P0-4 raises the stakes: the passphrase is now what protects
the only copy of the key.

**Fix.** When `isNewVault` is true, render a second "Confirm passphrase"
field and block submit unless the two match; enforce a minimum length
(pick a number and state it in the helper text). Leave the returning-user
path as a single field. Keep the existing recovery warning and extend it to
mention the key file (see P0-4's tradeoff).

**This is untestable as specified — fix that here.** After P0-1 step 1 the
browser fallback decides `hasVault()`, and if it returns `true` (matching
today's behaviour) then `isNewVault` is never true under `npm run dev`, so
the confirm field and the minimum length get zero coverage from the only
automated suite this project has. Make the fallback's vault state
controllable — a query param (`?vault=new`) read by `src/api/entries.ts` is
enough — and add an e2e case for the create-vault form: mismatched
confirmation blocks submit, too-short passphrase blocks submit, matching
pair proceeds.

**While in this file.** `LoginPage` validates an `email` field
(`src/pages/LoginPage.tsx:31`-`:37`) that nothing consumes — there is no
account system, and the value is never passed to `login`. If it is purely
decorative, deleting it removes two error paths from the same form this
item is adding a field to. Product call, not a defect; decide it here
rather than carrying it. Note the e2e helper fills it in all five tests
(`e2e/entries.spec.ts:10`), so deleting it means touching the suite.

**Prove it.** A new `e2e/create-vault.spec.ts`, every case behind
`page.goto("/?vault=new")` — which is the entire reason the hook exists,
since today `hasVault()` rejects in the browser, `LoginPage` catches that
into "returning user", and this form has zero coverage:

1. `the create-vault form asks for confirmation` — assert the **Confirm
   passphrase** field and the **Create vault** button are both visible.
   Red today: no such field exists.
2. `a mismatched confirmation blocks submit` — two different values,
   submit, assert an error is shown and the app has not navigated
   (`await expect(page.getByRole("heading", { name: "Entries" }))
   .toHaveCount(0)`). Red today by way of test 1.
3. `a too-short passphrase blocks submit` — one character below the
   minimum you pick, same assertions. State the minimum in the field's
   helper text and assert against that rendered text rather than a magic
   number duplicated in the test.
4. `a matching pair creates the vault` — same value twice, long enough,
   assert `Entries` appears. This is what stops 2 and 3 being "fixed" by
   blocking every submit.

If the `email` field goes, update `login()` in `e2e/entries.spec.ts` in
the same commit: all five existing tests fill it
(`e2e/entries.spec.ts:10`) and will go red on a locator that no longer
exists. Expected churn, not a regression — say so in the commit message.

### P1-4. Off-main-thread commands make `save_entry` non-atomic

> **Landed 2026-09-16**, not in the P1-1 commit as this section asks —
> P1-1 shipped first. The write lives in `FsStore::put` since ADR 0009
> split the crate, so that is where the temp-file-and-rename went, with
> a per-call scratch name (two threads sharing one temp path would tear
> the blob exactly as a shared `fs::write` does) and `list` skipping
> anything ending in `.tmp`. Ships **without a failing test**, as this
> section prescribes: `save_entry_leaves_no_temp_file` (`crates/vault`)
> and `put_leaves_no_temp_file_behind` (`crates/fs-store`) are guards,
> green on both sides of the fix. The third,
> `list_skips_scratch_left_by_an_interrupted_put`, covers the new
> convention rather than the old defect.

`crates/storage/src/lib.rs:87`.

P1-1 is what introduces this: today commands are serialised by the main
thread, so nothing in `journal-storage` ever runs concurrently. Once they
move to the blocking pool, two `save_entry` calls for the same id, or a
save racing `list_entries`' read of the same path, can interleave — and
`save_entry` is a bare `fs::write`, which truncates before it writes. The
window is a torn file, i.e. one lost entry. P2-2 already lists "a file
being written concurrently" as a reachable cause of read failures without
noticing where the concurrency comes from.

**Fix.** Write to `<id>.json.tmp` and `fs::rename` onto the final path.
Rename is atomic within a directory on every platform the app targets, so a
reader sees either the old file or the new one. Land it in the P1-1 commit.

**Verify.** Assert the invariant, not the race: a test that saves, then
checks no `.tmp` file is left behind and `get_entry` round-trips. Atomicity
is `rename`'s guarantee, not something to re-derive — an earlier draft
proposed racing two writer threads against a reader, which would be a slow,
flaky test that can only ever fail to disprove the property.

**Prove it (guard only — there is no honest failing test here).**
Atomicity is `rename`'s guarantee; a test that races threads against each
other can only ever fail to disprove it, and would be slow and flaky, so
write the invariant instead. In `journal-storage`,
`save_entry_leaves_no_temp_file`: save an entry, overwrite it with a
second save, then assert (a) `get_entry` returns the second version,
(b) no file in the directory ends in `.tmp`, and (c) `list_entries`
returns exactly one header. Green before and after. It exists to catch the
*fix's* own new failure mode — a crash between write and rename leaving
`<id>.json.tmp` behind, or a listing that starts counting those.

Say plainly in the PR that this item ships without a failing test, and
why.

---

## P2 — robustness and hygiene

### P2-1. Mutex poisoning recovery — folded into P1-1

`std::sync::Mutex` poisons on panic, and every command does
`state.lock().map_err(|e| e.to_string())?`, so one panic while the lock is
held would fail every subsequent command for the life of the process.

Scope correction from the review: the root `Cargo.toml` sets
`panic = "abort"` under `[profile.release]`, so a panic aborts the process
and poisoning is unreachable in shipped builds. This is a `make run` /
`cargo test` problem (debug profile, unwinding) only.

The fix is `unwrap_or_else(|e| e.into_inner())` inside the shared `storage`
helper that P1-1 introduces anyway — so it is no longer a separate item.
Nothing to do here beyond P1-1.

**Prove it.** P1-1's `mutex_poisoning_does_not_wedge_the_vault` is this
item's test; there is nothing separate to write.

### P2-2. `fs::read` failure still aborts the whole listing

> **Closed 2026-09-16. No fix was needed — the ADR 0009 split had already
> made it green**, which the tests below were written to find out and did.
> They stay as guards, not as proof: both passed before any change, so per
> "How each item is proved" the work stopped there rather than carrying on
> to a fix that had nothing left to do.

The original defect, in the crate that is now three: the `?` on `fs::read`
sat outside the `match` that swallows decrypt and parse failures, so a
single unreadable file made `list_entries` return `Err` and the entry list
fail entirely. Reachable via bad file permissions, a stray *directory*
named `something.json` (yields `IsADirectory`), or the concurrent write of
P1-4.

**Why it is already fixed.** The read is no longer in the listing loop.
`Vault::list_entries` calls `Vault::read`, whose `self.store.get(name)?`
returns a `VaultError::Store` *into* the `match` — the same arm that logs
and skips a decrypt failure — so an I/O error on one entry costs that
entry and nothing else. The directory case does not even reach the read:
`FsStore::list` filters on `is_file`, so a directory named like a blob is
never reported as one. Only directory-level errors (`read_dir` and the
`DirEntry` result) are still fatal, which is what the fix asked for.

**Guard tests** (`crates/vault`, green before and after):

- `list_entries_survives_an_unreadable_entry` — one good entry plus
  `fs::create_dir_all(dir.join("<uuid>.json"))`; `list_entries()` is `Ok`
  with exactly one header.
- `list_entries_survives_an_entry_with_no_read_permission` —
  `#[cfg(unix)]`, a real entry file chmod'd to `0o000`, same assertion.
  Guarded against running as root, which would read the file anyway and
  pass for the wrong reason: after the chmod, `if fs::read(&path).is_ok()
  { return; }`.

### P2-3. A corrupt key file is indistinguishable from a wrong passphrase

> **Partly landed 2026-09-09** with P0-4, which needed an error for the
> case it introduced: a key file that decrypts but does not parse as an
> `AGE-SECRET-KEY-1…` string is `VaultError::CorruptKeyFile`, not
> `WrongPassphrase`, covered by the third test below
> (`a_key_file_that_opens_but_does_not_parse_is_not_a_wrong_passphrase`),
> landed ahead of the first two. The
> `age::DecryptError` variant matching is **still open**, so a truncated
> or zero-length key file is still reported as a wrong passphrase.

`crates/vault/src/lib.rs`, `unlock_vault`.

`unlock_vault` maps *every* decrypt error to `WrongPassphrase`, including
`age::DecryptError::InvalidHeader` from a truncated or zero-length file
(e.g. a crash between `create_dir_all` and `fs::write` in `create_vault`,
or an interrupted sync). The user is then told their passphrase is wrong on
every attempt, with no hint that the file itself is damaged.

**Fix.** Match on the `age::DecryptError` variant: map only
`NoMatchingKeys` and `Decryption` to `WrongPassphrase`, and let structural
errors surface as `VaultError::Decrypt(_)` (or a new
`VaultError::CorruptKeyFile`) so P1-2 shows a distinct message. Under P0-4
this is also where a key file that decrypts but does not parse as an
`AGE-SECRET-KEY-1…` string must land — same class of problem, same message.
Add a unit test that truncates the key file to a few bytes and asserts the
error is *not* `WrongPassphrase`.

**Prove it.** Two tests in `crates/vault`:

1. `a_truncated_key_file_is_not_reported_as_a_wrong_passphrase` — create
   the vault, truncate the key file to a few bytes
   (`fs::write(&path, b"age")`), unlock with the **correct** passphrase,
   and assert the error is *not* `VaultError::WrongPassphrase`. Red today:
   `unlock_vault` maps every `DecryptError`, `InvalidHeader` included,
   onto `WrongPassphrase`.
2. `a_zero_length_key_file_is_not_reported_as_a_wrong_passphrase` — the
   same with `fs::write(&path, b"")`, which is the crash-between-
   `create_dir_all`-and-`write` case.

Assert the negative (`!matches!(err, VaultError::WrongPassphrase)`) rather
than a specific variant, so the test survives whichever of `Decrypt(_)` or
a new `CorruptKeyFile` you land.

After P0-4, add a third: seal a value that is *not* an
`AGE-SECRET-KEY-1…` string under the correct passphrase, write it as the
key file, and assert the same. It decrypts perfectly and still must not be
called a wrong passphrase.

### P2-4. `logout` leaves plaintext in the renderer, and now races the backend

> **First half landed 2026-09-09** with P0-1 step 1, as this item
> predicted: `entries` now initialises to `[]`, `offlineEntries` is gone
> (the seed lives in `src/api/entries.ts`), and `logout` calls
> `setEntries([])` before `lockVault()`. **Outstanding:** the
> `let cancelled = false` guard on the `list_entries` effect and both
> **Prove it** e2e cases — neither is reachable until P1-1 puts commands
> off the main thread.

`src/context/AppStateContext.tsx:81`.

`lockVault()` clears the key in Rust, but `entries` (every title) stays in
React state, so decrypted content is still live in the webview heap after
"locking". The sibling problem is that `entries` is *initialised* to the
seeded headers (`src/context/AppStateContext.tsx:48`), so under Tauri the
user reads the fake "First day back" and "Project ideas" rows until
`list_entries` returns.

Both halves are consequences of the fallback living in the context, and
P0-1 step 1 removes that: `entries` initialises to `[]`, and
`offlineEntries` — which was retaining the full markdown of every entry
opened this session — no longer exists. Add `setEntries([])` in `logout` in
the P0-1 commit.

**New, and not in the original item: P1-1 adds a race.** Once commands run
off the main thread, `lock` can complete while a `list_entries` is still in
flight. Two consequences:

- The stale promise resolves *after* logout and `setEntries(loaded)`
  repopulates decrypted titles on the login screen, undoing the
  `setEntries([])` above. Guard it with the `let cancelled = false` +
  cleanup-function pattern the codebase already uses twice
  (`src/pages/EditorPage.tsx:29`-`:41`, `src/pages/PreviewPage.tsx:40`-`:49`)
  — the `list_entries` effect already keys on `authenticated`, so its
  cleanup fires on logout and the existing idiom covers this exactly. No
  generation counter needed.
- The `Arc<Storage>` cloned by an in-flight command keeps the session key
  alive past logout, so zero-on-drop is deferred until that command drains.
  Bounded and acceptable, but worth knowing given that zeroization is the
  stated reason for using `secrecy` at all.

**Prove it.** Two more tests in `e2e/backend-failures.spec.ts`, one per
half, both using `?slow=list` to hold `list_entries` for 1200 ms:

1. `the entry list renders nothing until the backend answers` —
   `page.goto("/?slow=list")`, log in, and immediately assert
   `page.getByText("First day back")` has count 0; then assert it appears
   once the delay elapses. Red today: `entries` is seeded at construction,
   so rows are on screen before any backend call returns — under Tauri
   that is fabricated entries shown inside a real journal.
2. `a list in flight at logout cannot repopulate the next session` —
   `?slow=list`, log in, click **Log out** without waiting for rows, sit
   out the delay on the login screen, log back in, and immediately assert
   count 0 again. Red today: the stale promise resolves after logout,
   `setEntries` puts the previous session's titles back, and they render
   instantly on the second login.

Test 2 is the P1-1 race and only makes sense once commands are off the
main thread. It is deterministic because the delay is fixed; keep the
1200 ms constant well clear of Playwright's action time and resist
tightening it. The Log out control is an icon button
(`src/components/AppShell.tsx:83`-`:98`) — if
`getByRole("button", { name: "Log out" })` does not resolve, give the
`IconButton` an `aria-label`, which it should have regardless.

The remaining half of the original item — decrypted titles still sitting
in the webview heap after `lock()` — is not observable from Playwright.
Test 2 covers the code path that clears them (`setEntries([])` on logout
plus the cleanup guard); nothing further is worth writing.

### P2-5. `setSubmitting(false)` is not in a `finally`

> **Landed 2026-09-16** with P1-2, as this section prescribes. The
> third assertion on `an unlock failure shows the backend's own
> message` was watched fail on the intermediate state — `login`
> rejecting, `setSubmitting(false)` still only on the success path —
> with the button `disabled` after the failed submit, then pass once
> the `finally` was in place.

`src/pages/LoginPage.tsx:44`-`:46`.

If `login()` rejects rather than returning a value — `isTauri()` throwing,
or a refactor letting `unlockVault` errors escape (P1-2 touches exactly
this code) — the submit button stays disabled forever and the rejection is
unhandled, leaving the user stuck on the login screen with no error shown.

This also matters more after P1-1: `submitting` is what prevents a
double-click from issuing two concurrent `unlock`s, which would both take
the `create_vault` branch.

**Fix.** Wrap the `await login(password)` in `try`/`catch`/`finally`, with
`setSubmitting(false)` in the `finally` and a generic error message in the
`catch`. Small enough to land inside the P1-3 commit, which rewrites this
handler anyway.

**Prove it — and mind the window.** This defect is unreachable today:
`login` swallows every rejection, so `setSubmitting(false)` is always
reached. It becomes reachable the moment P1-2 makes `login` reject, which
is why the test belongs in the P1-2 commit.

Extend `an unlock failure shows the backend's own message` (P1-2) with a
third assertion: after the failed submit the button is enabled again —
`await expect(page.getByRole("button", { name: "Sign in" }))
.toBeEnabled()`. Land P1-2 without the `finally` and it is red: the button
stays disabled forever and the user is stranded on the login screen. With
the `finally`, green.

If P1-2 and P2-5 land together, note in the PR that the red was observed
on the intermediate state; otherwise the assertion reads as one that could
never have failed.

---

## Suggested commit sequence

Each numbered step lands its **Prove it** test in the same commit as the
fix, with the failing run from before the fix quoted in the PR body. The
two guard-only items (P0-1 step 1 and P1-4) say instead that no failing
test exists and why.

1. ~~**P0-0** (bridge field names)~~ — **done 2026-09-09.** Landed as
   Rust-as-source-of-truth plus ts-rs codegen, not the camelCase flip
   originally written here. Note it also falsified this plan's
   "installed base of zero" premise — see the correction above before
   starting step 3.
2. ~~**P0-1 step 1** (move the browser fallback into `src/api/entries.ts`)~~
   — **done 2026-09-09**, along with step 7. Behaviour-neutral, covered by
   the existing e2e suite, and the structural precondition for every later
   frontend item. Absorbs P2-4's first half.
3. **P0-4** (cached X25519 key) + ~~ADR 0007 superseding 0006~~. **ADR
   done 2026-09-09**; the crate change is still open and is now the
   longest-standing item. Do it before the error-taxonomy and threading
   work so those are written once, against the design that ships.
4. ~~**P0-2 + P2-2**~~ (logging, then `list_entries` skipping per-file
   I/O errors) — **P0-2 done 2026-09-09**, on `log`/`tauri-plugin-log`
   rather than `tracing-subscriber`, with ADR 0008 recording it. **P2-2
   closed 2026-09-16** with no fix: the ADR 0009 split had already moved
   the read behind `BlobStore::get`, inside the arm that logs and skips.
   Guard tests only.
5. **P1-1 + P1-4** (`Mutex<Option<Arc<Storage>>>`, the shared `storage`
   helper, `async fn` + `spawn_blocking`, atomic entry writes). Absorbs
   P2-1. No UI changes.
6. **P0-3 + P2-3 + P1-2** (vault error surface: `open_or_create`,
   corrupt-vs-wrong-passphrase, user-facing `#[error]` strings, and `login`
   rejecting instead of returning `false`) — these three share one surface
   and are awkward to split.
7. ~~**P0-1 step 2** (propagation through
   `upsertEntry`/`removeEntry`/`EditorPage`/`PreviewPage`)~~ — **done
   2026-09-09**, out of sequence, since splitting it from step 1 would have
   left the fallback unable to fail. P2-4's logout-race guard is **not**
   done: it needs P1-1's off-main-thread commands to be reachable.
8. **P1-3** (confirm field, minimum length, controllable fallback vault
   state and its e2e case, and the `email` decision), absorbing P2-5's
   `finally`.

## Verification

- The per-item **Prove it** blocks are the primary gate; the manual passes
  below are the second one, not a substitute. Dependencies they imply:
  `serde_json` for `crates/entry` (landed with P0-0, along with `ts-rs`
  behind that crate's optional `ts` feature), a log sink for
  `crates/storage` (landed with P0-2 — `log` plus a test-local capturing
  `log::Log`, *not* `tracing-subscriber`), and `tauri` with the `test`
  feature for `src-tauri` (landed with P0-2 as well). All three are in the
  tree; no item below needs a new dependency.
- `cargo test --workspace` after each Rust change. Before P0-4,
  `journal-storage` and `journal-vault` tests are slow by design (~1s per
  `age::scrypt` op) — prefer `--release` when iterating. After P0-4 the
  suite should be fast again; if it is not, the key is not actually being
  cached.
- `npm run build` (`tsc`) after each frontend change; `npm run test:e2e`
  still exercises only the browser fallback, so a green run is **not**
  evidence any of the vault behaviour works. It *is* the regression test
  for P0-1 step 1, which must not change browser behaviour, and P1-3 adds
  the first e2e coverage of the create-vault form. The original note here
  — that `tsc` cannot catch P0-0 — **no longer holds**: since P0-0 the
  wire types are generated from Rust, so a field-name mismatch is a
  compile error at every call site. `invoke<T>()` still asserts its return
  type without validating it at runtime, so `make run` remains the only
  proof that a command actually round-trips.
- `make check-bindings` after any change to a type that crosses `invoke`.
  It regenerates `src/bindings/` and fails if the committed output is
  stale; a stale binding silently disables the `tsc` guard above.
- Manual `make run` passes as listed under each item above; the
  browser-fallback path (`npm run dev`) must keep working unchanged
  throughout.
- ~~`CLAUDE.md` needs updating with P0-4~~ — **done 2026-09-09** in the
  P0-4 commit: the `crates/vault` paragraph now describes the `Vault`
  handle, `.vault-key` and the pinned work factor, and the no-migration
  note covers ADR 0006's format as well as pre-encryption plaintext. It
  keeps the "confirm before dropping local data" wording rather than the
  "no installed base" line originally proposed here — the correction at
  the top is why.
