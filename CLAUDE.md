# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Journal is a cross-platform (desktop, iOS, Android) journaling app built as a
Tauri v2 application: a React/TypeScript frontend rendered inside a native
Tauri shell, with a minimal Rust backend. Planned features (not yet built):
markdown support, sync to Dropbox/Google Drive/iCloud, end-to-end encryption.

**This is alpha software — nothing is final.** There are no released versions,
no external users, and no stability guarantees. Storage formats, crate
boundaries, command signatures, types, and UI are all open to change when a
better design shows up; prefer changing the design over layering compatibility
shims on top of it. Backward compatibility and data migrations are not required
by default (see the deliberate no-migration decision under `crates/key`
below) — if a change would drop existing local data, say so and confirm with
the user rather than assuming a migration must be written.

## Commands

- `npm run dev` — run the Vite dev server only (frontend, browser).
- `make run` — run the full Tauri app (opens the native window, requires a
  Rust toolchain). This is the primary way to see real app behavior, since
  the app is designed to run inside Tauri, not a browser. It regenerates
  `src/bindings/` first; plain `npm run tauri dev` is the same launch
  without that step.
- `make build` — build the native installers. Fails if `src/bindings/` is
  stale rather than regenerating it, so a release can't ship types that
  have drifted from the Rust ones. Plain `npm run tauri build` skips the
  check.
- `npm run build` — type-check (`tsc`) then build the frontend for production.
- `npm run preview` — preview the production frontend build.
- `npm run tauri <args>` — passthrough to the Tauri CLI (e.g. `npm run tauri build` for native installers).
- `make bindings` — regenerate `src/bindings/*.ts` (the TypeScript wire
  types) from the Rust definitions in `crates/entry` via ts-rs.
  `make check-bindings` regenerates and fails if the result differs from
  what is committed; run it after changing any type that crosses the
  `invoke` bridge. `make run` and `make build` already do this for you, as
  does CI — neither `npm run build` nor the raw `npm run tauri *` commands
  do.
- `npm run test:e2e` / `make test-e2e` — run the Playwright UI tests
  (`e2e/`, see ADR 0004) against the Vite dev server, which
  `playwright.config.ts` starts automatically. These run against the
  browser fallback (seeded in-memory data), not the Rust backend — see
  `src/api/backend.ts` below, and `src/api/fallback.ts`, which carries the
  URL fault-injection hook the suite uses to make that fallback fail on
  demand (and to simulate sync).
- `make check-no-fallback` — build the frontend and fail if the browser
  fallback (seed entries or the fault hook) reached the production bundle.
  Runs in CI next to `npm run build`; see `src/api/fallback.ts` below.
- `make rust-test` — `cargo test` over the whole workspace.
- `make test` — the whole suite: `rust-test` then `test-e2e`, in that order.
- `make coverage` — line coverage of both languages, written as lcov (what
  `.github/workflows/coverage.yml` uploads to Codecov, whose badge in the
  README is the combined total) and as HTML. **Quiet:** it prints only the
  Rust, web and total percentages (with the change since the last local run),
  the patch coverage, and the changed lines no test executed
  (`scripts/coverage-summary.sh`; working tree against `COV_BASE`, default
  `HEAD`). The full output is in `coverage/run.log`; `V=1` streams it. The
  `coverage-check` skill runs it once after a feature — never two runs at
  once, since the web half holds port 5183. Codecov gates the combined total
  (`codecov.yml`): it must stay at or above 85% and not fall below the base
  commit's. Those are commit statuses, so they block a merge only once
  `codecov/project/floor` and `codecov/project/no-drop` are required in
  branch protection.
  `make coverage-open` opens the two HTML reports;
  `coverage-rust` and `coverage-web` run one half. Output lands in
  `coverage/` (gitignored). Test code is left out of every report:
  - **Rust** uses `cargo llvm-cov` on **nightly** (needs `cargo binstall
    cargo-llvm-cov` and the `llvm-tools-preview` component), because the
    inline test modules opt out with `#[coverage(off)]`, which is unstable.
    Every `#[cfg(test)]` module (or test-only item) therefore carries
    `#[cfg_attr(coverage_nightly, coverage(off))]` under it, and a crate
    root that has one starts with `#![cfg_attr(all(test, coverage_nightly),
    feature(coverage_attribute))]`. A new crate gets `[lints] workspace =
    true` in its manifest, which is what declares `coverage_nightly` to
    stable builds. `tests/`, `examples/`, `build.rs`, `MemoryRemote` and
    `main.rs` are dropped by the `COV_IGNORE` regex in the `Makefile`
    instead. Plain `cargo test` stays on stable and never sees any of it.
  - **Web** has no unit tests, so its number is what the Playwright suite
    reaches. `COVERAGE=1` makes the dev server instrument `src/`
    (`vite-plugin-istanbul` in `vite.config.ts`, excluding
    `src/api/fallback.ts` and `src/bindings/`), and `e2e/coverage.ts`
    saves each page's counters to `.nyc_output/` for `nyc report`
    (`.nycrc.json`). **Specs import `test`, `expect` and `Page` from
    `./coverage`, not `@playwright/test`** — without it their coverage is
    silently not collected. Counters die with the document, so the fixture
    saves them just before `page.goto`/`page.reload`; a navigation the page
    starts itself would be missed (the app starts none).
- `make format` — `rust-format` (`cargo fmt --all`, whole workspace) then
  `ts-format` (`npm run format`, Prettier over every `.ts`/`.tsx` file).
  `src/bindings/` is excluded via `.prettierignore` since it's ts-rs's own
  generated output — formatting it would fight `make bindings`.

There is no linter configured yet (Prettier above only formats, it doesn't
lint), and no unit tests for the frontend (Rust
crates have `cargo test` unit tests, e.g. `crates/vault`; `src-tauri`'s
run over `tauri::test`'s mock runtime, from its `tauri = { features =
["test"] }` dev-dependency). `tsc` (via
`npm run build`) is the current form of static checking, alongside the
Playwright UI tests above. Because the frontend's wire types are generated
from Rust (see `src/bindings/` below), `tsc` also catches `invoke`
field-name mismatches — a class of bug neither the Playwright suite nor
`cargo test` can see, since the browser fallback never crosses the bridge.

## Architecture

- **Frontend** (`src/`): React 19 + TypeScript, MUI v6 for components, routed
  with `react-router-dom` using `HashRouter` (required for compatibility with
  Tauri's static asset serving — do not switch to `BrowserRouter`).
  - `App.tsx` defines routes and gates authenticated-only routes via a
    `RequireAuth` wrapper that reads from `AppStateContext`. The
    authenticated routes sit under one layout route
    (`components/JournalLayout.tsx`), so the sidebar and entry list mount
    once and keep their scroll position as entries are opened; `/entries`
    renders the empty pane, `/entries/:id` the entry and `/calendar` the
    calendar (`JournalLayout` counts either of the last two as "pane open",
    which is what makes them replace the list below `md`). There is no
    editor route — `/entries/:id/edit` only redirects (see the UI section
    below).
  - `context/AppStateContext.tsx` is the single source of app state right
    now: auth flag and the list of entry headers (`list_entries` returns
    `Header`, not the full entry). `loadEntry(id)` fetches a full entry via
    `get_entry` on demand (used when opening or editing an entry); saves
    and deletes go through `src/api/entries.ts`. The context knows nothing
    about whether a backend exists, so every rejection it sees is a real
    failure: `upsertEntry`/`removeEntry` update state optimistically, then
    roll that update back and reject if the backend refuses, letting the
    page keep the draft and show the error rather than reporting a save
    that never reached disk. `loadEntry`'s identity must stay stable
    (`useCallback`, no deps) — `EntryPane` keys a `useEffect` on it, so
    rebuilding it on every `entries` change re-runs that effect and wipes
    the draft. All pages read/write through `useAppState()` rather than
    local state or prop drilling.
    It also holds the **sync status** (`sync`, plus `pendingIds` and
    `conflictedIds` as `Set`s for rows to look themselves up in, and
    `providers` from `list_providers`), seeded from `sync_status` and kept
    current by `sync://status`. Sync is not gated on the vault — it runs while
    locked, and connecting is how a second device gets its key — so the
    subscription is live from the first render, on the login screen too.
    `sync://changed` bumps a version that re-runs the list effect, so the
    list reloads; the entry open in the pane is left alone, since autosave
    owns its draft. A reload does not simply replace the list: every local
    save and delete is kept (the `mutations` ref) until no listing that
    could have missed it is open, and laid over each listing's result —
    otherwise a save landing mid-reload put the row's older title back. `connectSync`/`disconnectSync`/`syncNow` re-read the
    status when they finish rather than trusting the throttled event.
    **The journal locks itself after 10 s without input**
    (`hooks/useIdleLock.ts`, `IDLE_LOCK_MS`; a constant until it becomes a
    setting): the timer is only a trigger for the same lock a manual "Log
    out" performs. Both write before locking — `useAutosave` registers a
    flush via `registerFlush`, and the lock awaits every one, because
    locking first unmounts the pane and its unmount flush then hits a
    locked vault and loses the draft. A failed flush **declines the idle
    lock** (the draft is the only copy; the timer retries after another
    quiet period) but not a manual log out. The fallback vault refuses
    entry commands while locked, like `VaultState`, so the Playwright
    suite (`e2e/auto-lock.spec.ts`, on `page.clock`) can see that.
  - `src/api/backend.ts` is the only module that knows whether a Rust
    backend exists, for both `entries.ts` (entry CRUD) and `sync.ts` (the
    sync commands and the two `sync://` events): inside Tauri every call
    goes over `invoke` and rejections propagate untouched; outside it
    (`npm run dev`, Playwright) a **development** build serves the seeded
    in-memory vault in `src/api/fallback.ts`. That module also carries the
    fault-injection hook it parses once at load: `?fail=<ops>` (the ops
    include `export`, `connect`, `sync` and `resolve`), `?failMessage=<text>`, `?slow=<ops>`,
    `?vault=new` (`?vault=arrives`: the same, but the first unlock reports the
    journal's key arriving from the cloud) and
    `?sync=on|conflict|busy|reauth|key_mismatch` (`busy`: every pass fires `sync://changed`; the last two start halted, with the
    message the backend sends). Its listing reads the vault when called, not when it
    answers, as the real one does, so `?slow=list` exposes races with it.
    It simulates sync too, in the
    sequence the backend produces — a save marks its entry pending at once,
    a debounced pass (`syncing` while it runs) clears it, and the pass that
    unlocking starts settles a `?sync=conflict` into a "(conflicted copy)"
    the way `resolver.rs` does — and connecting from it plays a second
    device joining: the vault "arrives" and an entry from another device
    reaches the list through `sync://changed`. The app uses `HashRouter`, so
    the query string goes *before* the hash (`/?fail=save`, not
    `/#/?fail=save`). `backend.ts` reaches the fallback through a dynamic
    `import()` that must stay textually inside an `import.meta.env.DEV`
    branch — the constant is
    replaced with `false` in a production build, so the branch is dead code
    and the whole chunk is dropped. A production build with no Tauri
    backend throws instead of fabricating a journal it would then lose.
    Keep every module-scope constant (seed data, the `Map`, the parsed
    faults) inside `fallback.ts`: a `new Map(...)` in `backend.ts` is not
    provably side-effect-free and drags the seed data back into the bundle.
    `make check-no-fallback` is what holds all of this.
  - `theme.ts` provides `getTheme(mode)`. The mode is a manual light/dark
    toggle held by `context/ColorModeContext.tsx`, saved to `localStorage`
    and defaulting to dark; it does not follow `prefers-color-scheme`.
  - The UI is one screen, in the shape of Day One's: a sidebar, the entry
    list, and the entry pane. `components/` holds the shell
    (`JournalLayout`, `Sidebar`, `EntryList`, and `EntryRow`, the one row
    the list, its search results and the calendar all draw) and `pages/`
    the route elements (`LoginPage`, `EntryPane`, `CalendarPage`). The
    sidebar is an icon rail one button wide (`SIDEBAR_WIDTH` in
    `components/chrome.ts`): each control
    is an `IconButton` whose label lives in a `Tooltip` on hover and in
    `aria-label`, which is what keeps it addressable by name (the
    Playwright suite clicks "New entry" and "Log out"). The sync button's
    label *is* the status ("2 changes not uploaded yet", `utils/sync.ts`)
    and it opens `components/SyncDialog.tsx`, where the account is
    connected, retried and disconnected (with a warning naming what has not
    been uploaded). The rail's "Export journal" button opens
    `components/ExportDialog.tsx`: pick a folder with the native picker,
    then a confirmation that names it and says the files are unencrypted,
    and only then the write. List rows carry a mark only when sync is connected and
    the row is the exception — "Not uploaded yet", or a conflict — never on
    a synced row, and it must not make a row taller. `LoginPage` offers
    "Connect <provider>" to someone with no vault, which is how a second
    device joins. While a modal is open MUI hides the rail from the
    accessibility tree, so `e2e/sync.spec.ts` reads its `aria-label`.
    Below the `md`
    breakpoint the three columns become one — the rail moves into a drawer
    and the list gives way to the pane — so check a change at phone width
    too.
    **Every size in the UI is `rem`, never `px`** — the shell's dimensions
    and the type scale both live in `components/chrome.ts` (`rem()`,
    `SIDEBAR_WIDTH`, `LIST_WIDTH`, `HEADER_HEIGHT`, `READING_WIDTH`,
    `BODY_MIN_HEIGHT`, `TYPE`, `TRACKING`, `sectionHeadingSx`), and
    `theme.ts` states `spacing` in `rem` too, so every `px:`/`gap:` follows.
    A bare number in `sx` is `px`, which ignores the reader's text-size
    setting; `e2e/text-size.spec.ts` sets the root font to 24px and fails
    if the rail, the headers, the list column or the type did not scale
    with it. Tracking goes in `em` for the same reason. The exceptions are
    deliberate and commented: hairline borders, corner radii, and
    `CalendarPage`'s `CALENDAR_WIDTH` and day dot, which are pinned to MUI
    X's hard-coded 320px grid. Nothing is sized in `vw`/`vh` — the list is
    a column, not a share of the screen, and the `md` breakpoint is what
    handles a small one.
    List rows carry a title and a time but no excerpt on purpose:
    `list_entries` returns headers without `markdown`, and filling
    excerpts would decrypt the whole journal to draw the list. A **search
    result** row does carry one line of the entry's text, which is a
    different trade rather than a reversal of that one: it decrypts on
    demand, for someone who asked a question, not on every render. The
    search field sits under the list column's header and is the whole of
    the search UI — `Ctrl`/`Cmd`-F focuses it from anywhere (the browser's
    own find bar would only search the rendered DOM), Escape clears it,
    and a non-empty query replaces the month sections with a flat result
    list. The query and its results live in `AppStateContext`, debounced
    ~200 ms, so they survive opening a result and are cleared by the lock
    along with the decrypted titles. A *failed* search leaves
    `searchResults` null on purpose: the journal stays on screen under the
    error, because an empty pane would read as "nothing matched", which is
    the one thing a failed search does not know. Rows sort
    and group by `entry_date` — the day the entry is *about* — and not by
    `updated_at`, so fixing a typo in a three-month-old entry leaves it
    where it belongs instead of dragging it to the top as it is typed.
    `updated_at` is still the last write, which is what sync and the
    resolver compare; it is no longer shown in the pane beyond a "last
    edited" tooltip on the save status. The pane's date heading *is* the
    date control: clicking it opens a `DateCalendar`
    (`@mui/x-date-pickers`, with `date-fns` as its adapter, wired up as a
    `LocalizationProvider` in `App.tsx`) and picking a day changes the day
    while keeping the entry's time of day. A change there goes through
    `autosave.noteChange` like a title edit, so the draft shape
    (`hooks/useAutosave.ts` `Draft`) carries `entryDate` alongside
    `title` and `markdown`.
    The rail's **Calendar** button opens `pages/CalendarPage.tsx` in the
    pane: a `DateCalendar` whose custom day (`slots.day`) dots every day
    with entries, the selected day's entries below it, and an **On this
    day** section for earlier years. It is all derived from `entries` (the
    helpers are `dayKey`/`entriesByDay`/`onThisDay` in `utils/date.ts`), so
    it decrypts nothing and needs no clearing on lock. The button's label
    carries the count of today's earlier-year entries ("Calendar — 1 entry
    on this day") and a dot marks the icon, the same label-is-the-status
    trick as the sync button. The grid needs `reduceAnimations`, as the
    entry pane's picker does (ADR 0013): the slide transition keeps the
    outgoing month mounted beside the one on screen. Marked days reach the
    day component through a context rather than a prop, because the slot
    is a component type and one defined inside the page would remount
    every day on each render. `e2e/calendar.spec.ts` pins the zone
    (`timezoneId`) and "now" (`page.clock.setFixedTime`, which fixes
    `Date` and leaves timers running).
  - **Editing is inline and autosaved** (`hooks/useAutosave.ts`): there is
    no Save button and no separate editor page. The hook keeps the live draft in a ref so its unmount cleanup can
    flush what was typed — which is also what a switch to another entry is,
    since the route element keys `EntryPane` on the id. It never writes an
    untouched *new* draft (so "New entry" costs nothing if abandoned — but
    emptying an entry already on disk is an edit and is written; `isNew` and
    the `written` ref tell the two apart), never
    reschedules after a failed save (that would retry a broken backend
    forever — the next keystroke or Retry does it), and `cancel()` before a
    delete keeps a pending save from resurrecting the entry — with
    `whenSettled()` after it, so a write already in flight lands before the
    delete rather than after.
    `hooks/useNewEntry.ts` generates the entry's id before navigating, so
    the draft's URL never changes under the keyed pane.
  - `src/bindings/` is **generated** — do not edit by hand. ts-rs derives
    it from `crates/entry` (behind that crate's off-by-default `ts`
    feature, so the codegen crate stays out of the shipped binary). That
    includes `SyncStatusPayload`/`SyncState`, the sync status the backend
    sends over `sync_status` and `sync://status`.
    `src/types.ts` re-exports it under the names the app uses
    (`EntryHeader`, `JournalEntry`), so imports stay pointed at
    `../types`. Rename a field in Rust without running `make bindings`
    and the check above fails; run it and `tsc` reports every call site.
    Wire fields therefore read `snake_case` in TypeScript, which is
    deliberate — purely local names (e.g. `formatUpdatedAt`'s parameter)
    stay camelCase.
  - Entries are markdown (`types.ts` `JournalEntry.markdown`) and the pane
    has **two explicit modes**, switched from the `⋯` menu and nowhere
    else — no focus-driven swapping, which is what the earlier designs got
    wrong. Read mode renders with `react-markdown` + `remark-gfm` into
    `.markdown-body` (`index.css`; the two rules that need the palette,
    link colour and image sizing, are `sx` on the container instead).
    Links are overridden (`components.a`) to open in the user's browser
    through `openUrl`, never in the webview: the window has no address bar,
    so a plain `<a>` would navigate the app off the journal. There is
    deliberately no `on_navigation` handler as well — a `tauri://` scheme
    test is wrong on Windows and Android, which serve `http://tauri.localhost`.
    Edit mode is `components/MarkdownEditor.tsx`, a CodeMirror 6 surface
    holding the raw markdown with its syntax styled as it is typed —
    headings large, quotes italic, marks muted — so no serializer ever
    rewrites what is on disk. `markdownHighlight` holds that styling, and
    everything mode-dependent sits in a CodeMirror `Compartment` so a
    light/dark switch reconfigures the editor rather than rebuilding it and
    dropping the undo history. An entry opens in read mode; a new one opens
    in edit mode, since there is nothing to render. Leaving edit mode saves
    first (`saveNow`), so the rendered view is never ahead of the vault.
    The mode is component state, not a route — reloading lands in read
    mode. The editor is a contenteditable, not a textarea: Playwright
    reaches the body as `getByTestId("entry-body")` in either mode, and the
    editor as `.cm-content` beneath it.
- **Backend** (`src-tauri/`): Rust, Tauri v2, part of a Cargo workspace
  rooted at the repo root (top-level `Cargo.toml`/`Cargo.lock`,
  `[workspace.dependencies]` for shared versions). New backend crates go
  under `crates/<name>/` as workspace members; `src-tauri` stays thin
  Tauri glue that wires up commands and manages app state (`src/lib.rs`).
  `crates/entry` (`journal-entry`) defines the `Entry` type — an
  `id`/`title`/`updated_at`/`entry_date` `Header` (flattened when
  serialized; `entry_date` takes a `#[serde(default)]` of "now", so an
  entry written before the field existed parses into the one current
  struct rather than meeting a migration) plus
  `markdown` — used by `journal-vault` and the Tauri commands.
  Rust names the wire format and the generated TypeScript follows, so
  these types stay `snake_case` on both sides (`updated_at`, not
  `updatedAt`) — do not add camelCase renames to make the frontend read
  more idiomatically. One serde impl serves both the `invoke` wire format
  and the encrypted on-disk payload, so renaming a field changes the
  on-disk format too.
  `crates/store` (`journal-store`) defines the `BlobStore` trait — named
  opaque byte blobs (`get`/`put`/`put_new`/`delete`/`list`/`contains`) — and
  `BlobName`, the validated newtype its methods take, so a name cannot
  address anything outside the store. `put_new` is the create-only write
  (`StoreError::AlreadyExists` if the name is taken): `contains` + `put`
  cannot tell a caller it was the one that created a blob, because the sync
  engine writes the same directory from its own thread, so `Keyring::create`
  uses it for the key. The trait's default body *is* that racy pair, so a
  store that can create exclusively must override it — `FsStore` does (a
  `hard_link` of a synced temp file, falling back to renaming that file into
  place unless the name has appeared where links are unsupported or denied:
  never a partial blob under the real name, but the check and the rename are
  two steps, so a writer in the gap between them is overwritten) and so does
  `NotifyingStore`, which forwards it and notifies. It holds no
  implementation:
  `crates/fs-store` (`journal-fs-store`) is the local one, one file per
  blob in a directory. Every layer above depends on the trait crate; only
  `src-tauri` names a concrete store, and the crates between reach for
  `journal-fs-store` as a dev-dependency for their tests. See ADR 0009
  for the three-layer split and the local-first sync plan.
  `crates/remote` (`journal-remote`) is the cloud-side counterpart:
  `RemoteStore`, a *separate* trait from `BlobStore` because a shared remote
  needs a change feed (deletions included) and conditional writes that report
  a lost race as `RemoteError::Conflict` rather than overwriting. It is
  synchronous and holds no implementation; its `testing` feature provides
  `MemoryRemote` for the sync engine's tests.
  `crates/sync` (`journal-sync`) is the engine over the two traits: local-first,
  key-less, one blocking `sync_once` pass at a time. It compares each blob with
  a per-device *base* (never synced) rather than clocks, so an edit always
  outranks a delete and a conflict is parked for a `ConflictResolver` the app
  supplies (it needs plaintext). `.vault-key` is never merged — a mismatch
  stops sync (`KeyMismatch`) — and has its own table in `Pass::sync_key`: never
  pushed once it differs from its base, never deleted (a remote deletion is
  undone by pushing it back, a local one by creating it from the remote copy,
  create-only so a key made meanwhile is not replaced). Its name is a constant
  here because `journal-key` keeps its own private, and a test checks they
  agree. It also has
  `NotifyingStore`, the pending/conflicted `SyncStatus` behind the UI's
  per-entry marks, and `erase_state` for disconnecting. `src-tauri/src/sync/`
  wires it up — see below and `.claude/plans/cloud-sync.md`.
  `crates/dropbox` (`journal-dropbox`) is the first `RemoteStore`: the
  official `dropbox-sdk` on its blocking `ureq` client (no async runtime in its
  tree), one file per blob in an "App folder". Besides `DropboxRemote` it holds
  the OAuth 2 PKCE connect flow (`authorize_url`/`finish`, which open no
  browser and listen on no socket), and `RefreshingClient`, the SDK client with
  token refresh written here so it can be tested. It takes a `RefreshToken` but
  does not keep one: that is `crates/credential`. Its tests run against a scripted
  `HttpClient`, not a network; `live_round_trip` is `#[ignore]`d and needs
  `DROPBOX_TOKEN`. The SDK logs the token response at `debug`, so keep its
  `dropbox_sdk::oauth2` target at `info` or above. `src-tauri/src/sync/` wires
  it up.
  `crates/credential` (`journal-credential`) is the provider-neutral login:
  `RefreshToken`, `Credential` (the token plus the id of the provider that
  issued it) and `CredentialStore`, with a 0600 `FileCredentialStore` that keeps
  the one saved credential. A new provider's adapter takes a `RefreshToken` and
  needs nothing else from it.
  `crates/vault` (`journal-vault`) is the layer that knows what an
  entry *is*, and is what the rest of the app means by "the vault":
  `{id}.json` blob naming, serde, and the header sort. One encrypted
  blob per entry (see ADR 0003), exposed via the `list_entries`
  (returns headers only)/`get_entry`/`save_entry`/`delete_entry`/
  `search_entries` commands. `Vault::search` (ADR 0014, with `search.rs`
  holding the matching itself: case-insensitive substring over title and
  markdown, and the snippet around the first hit) decrypts the journal on
  **every call**
  and keeps no index. That is measured, not assumed: over 1,000 entries a
  search costs ~100 ms in a release build — the same ~100 ms `list_entries`
  costs, since it decrypts every blob too, and the app already runs that on
  every unlock and after every sync pass. An index would save a cost already
  being paid, in exchange for invalidating it on every save, delete, lock
  and entry sync pulls in *underneath* this layer. Do not add one without
  evidence. Both commands share one enumerate-and-skip path
  (`Vault::read_each`), so a blob that will not decrypt is skipped and
  logged rather than failing either call.
  `Vault::open(store, passphrase)` owns the create-or-open
  decision — creating a vault only when the store holds neither a key
  blob nor an entry blob, and refusing with `MissingKeyFile` when the key
  is gone but entries remain, so a lost key cannot silently orphan a
  journal. That decision lives here, not in `journal-key`, because only
  this layer knows what an entry blob is named; keep it that way rather
  than branching on `Keyring::exists` in a command.
  `crates/key` (`journal-key`) encrypts every entry blob at rest with
  `age`, under an X25519 keypair (see ADR 0007, which supersedes 0006,
  amended by 0009, which moved the key into the store, and 0010, which
  renamed these two crates). It holds the key and nothing else — the
  word "vault" belongs one crate up, on the type that actually holds a
  user's entries. Four modules: `keyring.rs` (`Keyring` — the key and
  the sealed blob it lives in; it owns no directory), `cipher.rs`
  (`Cipher` — the two-method trait `Vault` depends on, so it never sees
  key handling and its tests can stand in a fake), `identity.rs`
  (`Identity` — the keypair, and everything about how it is sealed to and
  opened from bytes), `error.rs` (`KeyError`, whose `#[error]` strings
  are user-facing copy).
  The passphrase protects **only** the sealed key — a `.vault-key` blob
  in the same store as the entries, so a store synced to another device
  carries the key with it — and is used for exactly one scrypt operation,
  at unlock; entries are encrypted to the keypair and cost
  sub-milliseconds. The passphrase itself is never written to disk nor
  kept on the `Keyring`. The key blob also *is* the canary: a wrong
  passphrase is a failure to open it, which is how the app tells "wrong
  passphrase" from "first run".
  `src-tauri` exposes `has_vault`/`unlock`/`lock`, and the entry CRUD
  commands fail with `"vault is locked"` until `unlock` succeeds. The
  `unlock` command calls `Vault::open`.
  Every command is an `async fn` that runs its storage work through the
  `blocking` helper (`tauri::async_runtime::spawn_blocking`): a sync
  `#[tauri::command]` runs on the main thread and freezes the webview,
  and `#[tauri::command(async)]` on a sync fn just blocks a tokio worker
  instead. The async boundary stays at this layer — `journal-vault` and
  the crates below it are synchronous on purpose. `VaultState` keeps the
  store outside its mutex and `Mutex<Option<Arc<Vault>>>` inside it, so a
  command clones the `Arc` and drops the guard before doing any I/O, and
  a panic that poisons the mutex does not wedge the session.
  `Keyring::create`/`Keyring::unlock` are public but `Vault::open` is
  their only caller — keep it that way, since a fresh key written over
  existing entries orphans all of them. `Keyring::exists` is public only
  so `has_vault` (and thus `LoginPage`) can pick its copy.
  `src-tauri/src/export.rs` backs `export_entries(directory)`: one plaintext
  `YYYY-MM-DD-slug.md` per entry (the *local* day of `entry_date`, the id
  spliced in when two entries would share a name) with a YAML front matter
  block of `id`, `title`, `entry_date` and `updated_at`, so a later import
  can round-trip. It is the one place the app writes the journal
  unencrypted, so: it takes only an existing, **empty** folder outside the
  app's data directory, never overwrites (`create_new`), writes each file
  `0600` on Unix (owner only, whatever the umask), removes what it
  wrote if it fails part-way (or the empty-folder rule would turn one
  failed run into a permanent refusal), and its command logs a count and
  nothing else — never a path or a title. It reads through `list_entries` +
  `get_entry`, one entry's plaintext in memory at a time. A module, not a
  crate: glue over the vault with no reusable core. The folder picker is
  `tauri-plugin-dialog`, granted `dialog:allow-open` alone.
  `src-tauri/src/sync/` is the sync wiring, in six files: `mod.rs`
  (`SyncControl`, the connect/disconnect flow, the `list_providers`,
  `connect_remote(provider)`, `disconnect_remote`, `sync_now` and
  `sync_status` commands, and the
  `Provider` trait — an `id`, a display `name`, `begin` and `remote` — that a
  build lists in `SyncConfig::providers` and that lets tests stand in a fake),
  `dropbox.rs` (the one `Provider` there is; adding Google Drive is another
  file like it plus a line in `lib.rs`, and nothing else names a provider),
  `worker.rs` (the
  thread that owns the `SyncEngine` and its pure `Schedule`: a pass at
  startup, every 60 s, ~2 s after a save, on demand; exponential backoff after
  a transient failure, a halt after `Unauthorized` (which a
  `MissingPermission` counts as)/`KeyMismatch` until
  `sync_now`), `resolver.rs` (D1 — the newer entry keeps its id, the older
  becomes a "(conflicted copy)"; `Deferred` while the vault is locked, and
  `unlock` kicks a pass to settle those), `status.rs` (`derive_state`, the
  payload, and the 250 ms coalescer behind `sync://status`) and `oauth.rs`
  (the loopback listener the browser is redirected to, on the port
  `Provider::redirect_port` asks for or any free one; Dropbox asks for
  53682, so register `http://127.0.0.1:53682/` with the Dropbox app). `sync://changed`
  fires when a pass pulled, deleted or merged entries. Three things to keep:
  the vault is given a `NotifyingStore` over the `FsStore` while the engine
  gets the bare `FsStore` (its own pulls are not edits); the `Slot` that
  connects the two exists before the vault's store because connecting can
  happen long after; and a worker is stopped only after it is taken out of the
  `Slot`, since a pass's resolver saves through the store whose callback locks
  it. Sync keeps running while the vault is locked. The app key is a build-time
  env var: `DROPBOX_APP_KEY=… make run` (without it, connecting reports how to
  set it). The saved login (`remote-login`: provider id, token and account
  id) lives in the app config dir and the engine state in `sync-state.json` in
  the data dir, both outside the synced `entries/`. The engine state is one
  account's cursor and revisions, so `install` keeps it only while provider
  *and* account (`Provider::account`, asked once per connect) match the saved
  login; anything else erases it. Kept across accounts it is data loss — the
  new folder's listing reads as every entry deleted — and a test holds that.
  A cursor the remote forgets (Dropbox's `reset`) is `RemoteError::CursorExpired`;
  the engine then lists from scratch, and a blob missing from that listing is a
  deletion.
  - The key file's scrypt work factor is **pinned** (`WORK_FACTOR` in
    `identity.rs`) rather than benchmarked, and capped on read
    (`MAX_WORK_FACTOR`). `age` otherwise picks the factor by
    timing the writing machine and refuses on read anything much above
    what the reading machine benchmarks, so a desktop-written file is
    undecryptable on a phone — and the key file is the only copy of the
    key. Do not let `age` choose either number.
  - Because unoptimized scrypt is ~40x slower, the root `Cargo.toml`
    raises `opt-level` for the KDF's crates in the **dev** profile. Without
    it a `cargo test` or `make run` unlock takes ~17s instead of ~0.8s.
  - **Migration between on-disk formats is deliberately not supported.**
    Not from pre-encryption plaintext (installs predating ADR 0006), and
    not from ADR 0006's scrypt-per-file entries either. Such files fail to
    decrypt and are skipped by `list_entries` with a `warn!`. Do not add
    legacy-format detection or a conversion path — delete the entries
    directory when the format changes under you in development, and per
    the alpha note above, confirm with the user first when it would drop
    their local data.
    The rule is about the *read* path: what is banned is a second shape
    alive at once, with every reader branching on which one it got. A
    field that gains a `#[serde(default)]` (as `entry_date` did) is not
    that, and neither is a separate one-shot tool that rewrites blobs into
    the current shape and is then deleted.
  - `tauri.conf.json` — app identifier, window config, build hooks
    (`beforeDevCommand`/`beforeBuildCommand` run the npm scripts above),
    and the webview's Content-Security-Policy (ADR 0011, which also
    turned `withGlobalTauri` off). Tauri applies that policy only when it
    serves the frontend itself, so `make run` and the Playwright suite —
    which load Vite's `devUrl` — do not exercise it: check a CSP change
    against a packaged build, or a dev build with `devUrl` unset.
  - `capabilities/default.json` — Tauri's permission/capability grants for
    the webview; extend this when adding new Tauri plugins or commands. It
    grants `core:default`, `opener:default` and `dialog:allow-open` (the
    export folder picker — not the plugin's save, message or confirm
    dialogs).
  - Logging goes through `tauri-plugin-log`, registered as
    `logging_plugin()` in `src/lib.rs`. Backend crates log with `log`
    (not `tracing`); the plugin routes `Stderr` to logcat on Android and
    oslog on iOS, so one registration covers all three targets. It also
    writes a capped log file to the app's log directory. Its targets
    *replace* the plugin's defaults (`Stdout` + file); adding to them
    printed every line twice. Level is fixed at `Info` — it does not read
    `RUST_LOG`.
- Frontend and backend communicate via Tauri's `invoke()` bridge
  (`@tauri-apps/api`), wrapped in `src/api/entries.ts` for entry CRUD and
  `src/api/sync.ts` for sync.

## Architecture Decision Records

`docs/adrs/` contains ADRs for non-obvious architectural choices (e.g. why
React+MUI v6 was chosen over the latest MUI). Check `docs/adrs/README.md`
for the index before assuming a technology choice is undocumented, and use
the `adr` skill to record new architectural decisions (framework, library,
storage/protocol, or structural choices with real alternatives) — always
confirm with the user before creating a new ADR file.
