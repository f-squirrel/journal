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
by default (see the deliberate no-migration decision under `crates/vault`
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

There is no linter configured yet, and no unit tests for the frontend (Rust
crates have `cargo test` unit tests, e.g. `crates/storage`; `src-tauri`'s
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
    `RequireAuth` wrapper that reads from `AppStateContext`.
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
    (`useCallback`, no deps) — both `EditorPage` and `PreviewPage` key a
    `useEffect` on it, so rebuilding it on every `entries` change re-runs
    those effects and wipes the editor's draft. All pages read/write
    through `useAppState()` rather than local state or prop drilling.
  - `src/api/entries.ts` is the only module that knows whether a Rust
    backend exists: inside Tauri every call goes over `invoke` and
    rejections propagate untouched; outside it (`npm run dev`, Playwright)
    it serves a seeded in-memory vault. On that fallback path only — so a
    shipped build cannot be steered by a URL — it parses a fault-injection
    hook once at module load: `?fail=<ops>`, `?failMessage=<text>`,
    `?slow=<ops>` and `?vault=new`. The app uses `HashRouter`, so the query
    string goes *before* the hash (`/?fail=save`, not `/#/?fail=save`).
  - `theme.ts` provides `getTheme(mode)`, switched between light/dark based
    on `prefers-color-scheme` in `App.tsx`.
  - `pages/` holds one component per route (Login, EntryList, Preview,
    Editor); `components/AppShell.tsx` is the shared page chrome.
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
  - Entries are markdown (`types.ts` `JournalEntry.markdown`), rendered with
    `react-markdown` + `remark-gfm`.
- **Backend** (`src-tauri/`): Rust, Tauri v2, part of a Cargo workspace
  rooted at the repo root (top-level `Cargo.toml`/`Cargo.lock`,
  `[workspace.dependencies]` for shared versions). New backend crates go
  under `crates/<name>/` as workspace members; `src-tauri` stays thin
  Tauri glue that wires up commands and manages app state (`src/lib.rs`).
  `crates/entry` (`journal-entry`) defines the `Entry` type — an
  `id`/`title`/`updated_at` `Header` (flattened when serialized) plus
  `markdown` — used by `journal-storage` and the Tauri commands.
  Rust names the wire format and the generated TypeScript follows, so
  these types stay `snake_case` on both sides (`updated_at`, not
  `updatedAt`) — do not add camelCase renames to make the frontend read
  more idiomatically. One serde impl serves both the `invoke` wire format
  and the encrypted on-disk payload, so renaming a field changes the
  on-disk format too.
  `crates/storage` (`journal-storage`) persists entries as one JSON file
  per entry under the Tauri app data dir (see ADR 0003) and is exposed via
  the `list_entries` (returns headers only)/`get_entry`/`save_entry`/
  `delete_entry` commands. Sync is not wired up yet.
  `crates/vault` (`journal-vault`) encrypts every entry file at rest with
  `age`'s scrypt passphrase recipient (see ADR 0006). The passphrase is
  entered at login, held in memory only as a `secrecy::SecretString`, and
  never written to disk; a `.vault-canary` file in the entries dir is how
  the app tells "wrong passphrase" from "first run". `src-tauri` exposes
  `has_vault`/`unlock`/`lock`, and the entry CRUD commands fail with
  `"vault is locked"` until `unlock` succeeds.
  - **Migration from pre-encryption plaintext entries is deliberately not
    supported.** Installs that predate ADR 0006 have plaintext
    `<uuid>.json` entry files and no canary; on upgrade those files fail
    to decrypt and are skipped by `list_entries`. This is accepted — do
    not add a legacy-plaintext detection or conversion path.
  - `tauri.conf.json` — app identifier, window config, build hooks
    (`beforeDevCommand`/`beforeBuildCommand` run the npm scripts above).
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
