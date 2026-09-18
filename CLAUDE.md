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
  `src/api/entries.ts` below, which also carries the URL fault-injection
  hook the suite uses to make that fallback fail on demand.
- `make check-no-fallback` — build the frontend and fail if the browser
  fallback (seed entries or the fault hook) reached the production bundle.
  Runs in CI next to `npm run build`; see `src/api/fallback.ts` below.
- `make rust-test` — `cargo test` over the whole workspace.
- `make test` — the whole suite: `rust-test` then `test-e2e`, in that order.
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
    renders the empty pane and `/entries/:id` the entry. There is no
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
  - `src/api/entries.ts` is the only module that knows whether a Rust
    backend exists: inside Tauri every call goes over `invoke` and
    rejections propagate untouched; outside it (`npm run dev`, Playwright)
    a **development** build serves the seeded in-memory vault in
    `src/api/fallback.ts`. That module also carries the fault-injection
    hook it parses once at load: `?fail=<ops>`, `?failMessage=<text>`,
    `?slow=<ops>` and `?vault=new`. The app uses `HashRouter`, so the query
    string goes *before* the hash (`/?fail=save`, not `/#/?fail=save`).
    `entries.ts` reaches it through a dynamic `import()` that must stay
    textually inside an `import.meta.env.DEV` branch — the constant is
    replaced with `false` in a production build, so the branch is dead code
    and the whole chunk is dropped. A production build with no Tauri
    backend throws instead of fabricating a journal it would then lose.
    Keep every module-scope constant (seed data, the `Map`, the parsed
    faults) inside `fallback.ts`: a `new Map(...)` in `entries.ts` is not
    provably side-effect-free and drags the seed data back into the bundle.
    `make check-no-fallback` is what holds all of this.
  - `theme.ts` provides `getTheme(mode)`, switched between light/dark based
    on `prefers-color-scheme` in `App.tsx`.
  - The UI is one screen, in the shape of Day One's: a sidebar, the entry
    list, and the entry pane. `components/` holds the shell
    (`JournalLayout`, `Sidebar`, `EntryList`) and `pages/` the route
    elements (`LoginPage`, `EntryPane`). The sidebar is an icon rail one
    button wide (`SIDEBAR_WIDTH` in `components/chrome.ts`): each control
    is an `IconButton` whose label lives in a `Tooltip` on hover and in
    `aria-label`, which is what keeps it addressable by name (the
    Playwright suite clicks "New entry" and "Log out"). Below the `md`
    breakpoint the three columns become one — the rail moves into a drawer
    and the list gives way to the pane — so check a change at phone width
    too.
    List rows carry a title and a time but no excerpt on purpose:
    `list_entries` returns headers without `markdown`, and filling
    excerpts would decrypt the whole journal to draw the list. Rows sort
    and group by `updated_at`, the only timestamp an entry has, so an
    edited entry moves to the top as it is typed; an entry date of its own
    is the fix, and would be a model change.
  - **Editing is inline and autosaved** (`hooks/useAutosave.ts`): there is
    no Save button and no separate editor page. The hook keeps the live draft in a ref so its unmount cleanup can
    flush what was typed — which is also what a switch to another entry is,
    since the route element keys `EntryPane` on the id. It never writes an
    untouched draft (so "New entry" costs nothing if abandoned), never
    reschedules after a failed save (that would retry a broken backend
    forever — the next keystroke or Retry does it), and `cancel()` before a
    delete keeps a pending save from resurrecting the entry.
    `hooks/useNewEntry.ts` generates the entry's id before navigating, so
    the draft's URL never changes under the keyed pane.
  - `src/bindings/` is **generated** — do not edit by hand. ts-rs derives
    it from `crates/entry` (behind that crate's off-by-default `ts`
    feature, so the codegen crate stays out of the shipped binary).
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
  `id`/`title`/`updated_at` `Header` (flattened when serialized) plus
  `markdown` — used by `journal-vault` and the Tauri commands.
  Rust names the wire format and the generated TypeScript follows, so
  these types stay `snake_case` on both sides (`updated_at`, not
  `updatedAt`) — do not add camelCase renames to make the frontend read
  more idiomatically. One serde impl serves both the `invoke` wire format
  and the encrypted on-disk payload, so renaming a field changes the
  on-disk format too.
  `crates/store` (`journal-store`) defines the `BlobStore` trait — named
  opaque byte blobs (`get`/`put`/`delete`/`list`/`contains`) — and
  `BlobName`, the validated newtype its methods take, so a name cannot
  address anything outside the store. It holds no implementation:
  `crates/fs-store` (`journal-fs-store`) is the local one, one file per
  blob in a directory. Every layer above depends on the trait crate; only
  `src-tauri` names a concrete store, and the crates between reach for
  `journal-fs-store` as a dev-dependency for their tests. See ADR 0009
  for the three-layer split and the local-first sync plan.
  `crates/vault` (`journal-vault`) is the layer that knows what an
  entry *is*, and is what the rest of the app means by "the vault":
  `{id}.json` blob naming, serde, and the header sort. One encrypted
  blob per entry (see ADR 0003), exposed via the `list_entries`
  (returns headers only)/`get_entry`/`save_entry`/`delete_entry`
  commands. `Vault::open(store, passphrase)` owns the create-or-open
  decision — creating a vault only when the store holds neither a key
  blob nor an entry blob, and refusing with `MissingKeyFile` when the key
  is gone but entries remain, so a lost key cannot silently orphan a
  journal. That decision lives here, not in `journal-key`, because only
  this layer knows what an entry blob is named; keep it that way rather
  than branching on `Keyring::exists` in a command. Sync is not wired up
  yet.
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
  - `tauri.conf.json` — app identifier, window config, build hooks
    (`beforeDevCommand`/`beforeBuildCommand` run the npm scripts above),
    and the webview's Content-Security-Policy (ADR 0011, which also
    turned `withGlobalTauri` off). Tauri applies that policy only when it
    serves the frontend itself, so `make run` and the Playwright suite —
    which load Vite's `devUrl` — do not exercise it: check a CSP change
    against a packaged build, or a dev build with `devUrl` unset.
  - `capabilities/default.json` — Tauri's permission/capability grants for
    the webview; extend this when adding new Tauri plugins or commands.
  - Logging goes through `tauri-plugin-log`, registered as
    `logging_plugin()` in `src/lib.rs`. Backend crates log with `log`
    (not `tracing`); the plugin routes `Stderr` to logcat on Android and
    oslog on iOS, so one registration covers all three targets. Level is
    fixed at `Info` — it does not read `RUST_LOG`.
- Frontend and backend communicate via Tauri's `invoke()` bridge
  (`@tauri-apps/api`), wrapped in `src/api/entries.ts` for entry CRUD.

## Architecture Decision Records

`docs/adrs/` contains ADRs for non-obvious architectural choices (e.g. why
React+MUI v6 was chosen over the latest MUI). Check `docs/adrs/README.md`
for the index before assuming a technology choice is undocumented, and use
the `adr` skill to record new architectural decisions (framework, library,
storage/protocol, or structural choices with real alternatives) — always
confirm with the user before creating a new ADR file.
