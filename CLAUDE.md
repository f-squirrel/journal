# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Journal is a cross-platform (desktop, iOS, Android) journaling app built as a
Tauri v2 application: a React/TypeScript frontend rendered inside a native
Tauri shell, with a minimal Rust backend. Planned features (not yet built):
markdown support, sync to Dropbox/Google Drive/iCloud, end-to-end encryption.

## Commands

- `npm run dev` — run the Vite dev server only (frontend, browser).
- `make run` / `npm run tauri dev` — run the full Tauri app (opens the native
  window, requires a Rust toolchain). This is the primary way to see real
  app behavior, since the app is designed to run inside Tauri, not a browser.
- `npm run build` — type-check (`tsc`) then build the frontend for production.
- `npm run preview` — preview the production frontend build.
- `npm run tauri <args>` — passthrough to the Tauri CLI (e.g. `npm run tauri build` for native installers).

There is no test suite and no linter configured yet. `tsc` (via `npm run build`)
is the only current form of static checking.

## Architecture

- **Frontend** (`src/`): React 19 + TypeScript, MUI v6 for components, routed
  with `react-router-dom` using `HashRouter` (required for compatibility with
  Tauri's static asset serving — do not switch to `BrowserRouter`).
  - `App.tsx` defines routes and gates authenticated-only routes via a
    `RequireAuth` wrapper that reads from `AppStateContext`.
  - `context/AppStateContext.tsx` is the single source of app state right
    now: auth flag and the in-memory list of journal entries (seeded with
    fake data, no persistence yet). All pages read/write through
    `useAppState()` rather than local state or prop drilling.
  - `theme.ts` provides `getTheme(mode)`, switched between light/dark based
    on `prefers-color-scheme` in `App.tsx`.
  - `pages/` holds one component per route (Login, EntryList, Preview,
    Editor); `components/AppShell.tsx` is the shared page chrome.
  - Entries are markdown (`types.ts` `JournalEntry.markdown`), rendered with
    `react-markdown` + `remark-gfm`.
- **Backend** (`src-tauri/`): Rust, Tauri v2. Currently just the generated
  scaffold (`src/lib.rs` exposes a single `greet` command via
  `invoke_handler`). No real native functionality (storage, sync, crypto) is
  wired up yet — this is where that will live as it's built.
  - `tauri.conf.json` — app identifier, window config, build hooks
    (`beforeDevCommand`/`beforeBuildCommand` run the npm scripts above).
  - `capabilities/default.json` — Tauri's permission/capability grants for
    the webview; extend this when adding new Tauri plugins or commands.
- Frontend and backend communicate via Tauri's `invoke()` bridge
  (`@tauri-apps/api`); no commands are called from the frontend yet.

## Architecture Decision Records

`docs/adrs/` contains ADRs for non-obvious architectural choices (e.g. why
React+MUI v6 was chosen over the latest MUI). Check `docs/adrs/README.md`
for the index before assuming a technology choice is undocumented, and use
the `adr` skill to record new architectural decisions (framework, library,
storage/protocol, or structural choices with real alternatives) — always
confirm with the user before creating a new ADR file.
