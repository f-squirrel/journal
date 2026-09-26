# CLAUDE.md

Journal: a cross-platform (desktop, iOS, Android) encrypted journaling app —
Tauri v2, React 19 + MUI v6 frontend in `src/`, thin Tauri glue in
`src-tauri/`, backend logic in workspace crates under `crates/`. User-facing
overview, prerequisites and Dropbox setup: `README.md`. Decisions and their
rationale: `docs/adrs/` (index in `docs/adrs/README.md`; record new ones with
the `adr` skill, confirming first). Open problems: `docs/known-issues.md`.

**Alpha software — nothing is final.** No released versions or external users.
Prefer changing a design over compatibility shims. No data migrations: if a
change drops local data, say so and confirm with the user.

## Commands

- `make run` — full Tauri app (regenerates `src/bindings/` first). Primary
  way to see real behavior; `npm run dev` is the browser-only frontend.
- `make build` — native installers; fails if `src/bindings/` is stale.
- `npm run build` — `tsc` + frontend build (the only TS static check; no linter).
- `make bindings` / `make check-bindings` — regenerate / verify the ts-rs wire
  types. Run after changing any type that crosses `invoke`.
- `make test` — `make rust-test` (`cargo test`) then `make test-e2e`
  (Playwright, `e2e/`, against the browser fallback — no frontend unit tests).
- `make check-no-fallback` — fails if the dev fallback reached the prod bundle.
- `make coverage` — Rust + web line coverage, quiet summary (full log in
  `coverage/run.log`, `V=1` streams it). Never run two at once (port 5183).
  Use the `coverage-check` skill. Codecov gates the total at ≥85% and no drop.
- `make format` — `cargo fmt` + Prettier.

See the `Makefile` for the rest.

## Rules that are easy to break

Frontend:

- `HashRouter`, never `BrowserRouter` (Tauri static serving). Query strings
  go before the hash: `/?fail=save`.
- `src/bindings/` is generated — never edit it. Wire fields are `snake_case`
  on both sides; don't add camelCase renames. `src/types.ts` re-exports them.
- `src/api/backend.ts` is the only module that knows whether Tauri exists.
  Its `import()` of `src/api/fallback.ts` must stay textually inside an
  `import.meta.env.DEV` branch, and every module-scope constant stays inside
  `fallback.ts`, or the seed data ships. Fault/sync-simulation URL params
  (`?fail=`, `?slow=`, `?sync=`, `?vault=`, …)
  are parsed in `fallback.ts` (`URLSearchParams`).
- State lives in `context/AppStateContext.tsx` (`useAppState()`); its
  mutations are optimistic and roll back on rejection. `loadEntry` must keep
  a stable identity.
- Autosave (`hooks/useAutosave.ts`): every save is conditional on the
  `updated_at` the pane last saw; a stale save becomes a "(conflicted copy)"
  (ADR 0012 amendment). Idle lock (`hooks/useIdleLock.ts`) flushes drafts first.
- Every UI size is `rem` via `components/chrome.ts` — never bare numbers/`px`
  in `sx`, never `vw`/`vh`. `e2e/text-size.spec.ts` enforces it. Check
  changes at phone width (below `md` the layout collapses to one column).
- Entry list rows never show excerpts (would decrypt the whole journal);
  rows sort by `entry_date`, not `updated_at`.
- Keyboard shortcuts: defaults only in `src/shortcuts/actions.ts`; components
  use `useShortcut(id, …)`, never key strings. Bindings are `$mod`+`event.code`
  (layout-independent) and always carry a modifier; none fire while a dialog
  is open (ADR 0015).
- Markdown links open via `openUrl`, never in the webview.
- Date pickers need `reduceAnimations` (ADR 0013).
- E2E specs import `test`/`expect`/`Page` from `./coverage`, not
  `@playwright/test`, or their coverage is silently lost.

Backend:

- New backend code goes in a `crates/<name>/` workspace member with
  `[lints] workspace = true`; `src-tauri` stays thin glue. A type the vault
  stores belongs in `journal-entry`; one that only crosses `invoke`, in
  `journal-api`. Layering and sync design: ADRs 0009, 0010, 0012.
- Every Tauri command is `async` and does storage work through the
  `blocking` helper; crates below `src-tauri` stay synchronous.
- `Vault::open` is the only caller of `Keyring::create`/`unlock` — a fresh
  key over existing entries orphans them.
- The scrypt work factor is pinned (`crates/key/src/identity.rs`); never let
  `age` choose it (ADR 0007).
- No legacy-format read paths. A `#[serde(default)]` field is fine.
- No `expect` on thread spawns in `src-tauri`.
- `export.rs` is the only plaintext write: empty folder outside app data,
  `create_new`, `0600`, clean up on failure, log counts only.
- Search decrypts on every call, no index (ADR 0014) — don't add one without
  measurements.
- Keep the `dropbox_sdk::oauth2` log target at `info`+ (it logs tokens).
  Dropbox needs `DROPBOX_APP_KEY` at build time and redirect
  `http://127.0.0.1:53682/`.
- Tests: `#[cfg(test)]` modules carry
  `#[cfg_attr(coverage_nightly, coverage(off))]`; crate roots with one start
  with `#![cfg_attr(all(test, coverage_nightly), feature(coverage_attribute))]`.
- CSP (ADR 0011) only applies to packaged builds, not `make run` or e2e.
- New Tauri plugins/commands need grants in
  `src-tauri/capabilities/default.json`.
