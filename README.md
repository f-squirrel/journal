# Journal

A journaling app built as a [Tauri v2](https://v2.tauri.app/) application: a
React/TypeScript frontend in a native shell, with a Rust backend. Targets
desktop today; iOS and Android are planned but not yet wired up.

> **Alpha software.** Nothing here is final: features, storage formats, APIs,
> and the UI can all change without notice, and breaking changes may be made
> without a migration path. Don't rely on it for data you can't afford to lose.

## Current state

- **UI**: a three-pane layout (sidebar, entry list, entry pane), in the shape
  of Day One's. Entries are markdown, edited inline with a CodeMirror-based
  editor and rendered with `react-markdown` in read mode. Editing autosaves —
  there's no Save button.
- **Storage**: entries are stored locally, one encrypted JSON blob per entry,
  behind a generic blob-store trait (`crates/store`) so alternate/synced
  backends can be added later. `crates/fs-store` is the only implementation
  today, writing one file per blob to a local directory.
- **Encryption**: every entry is encrypted at rest with
  [`age`](https://github.com/FiloSottile/age) under an X25519 keypair. The
  keypair itself is sealed with a passphrase (scrypt) and stored alongside the
  entries, so a synced store carries its key with it. See the ADRs below for
  the design history.
- **Not yet built**: sync to Dropbox/Google Drive/iCloud, and iOS/Android
  builds.

## Development

- `npm run dev` — Vite dev server only (frontend, in a browser, with seeded
  in-memory data — no Rust backend).
- `make run` — the full Tauri app (opens the native window; needs a Rust
  toolchain). This is the primary way to see real app behavior.
- `make build` — build the native installers.
- `make test` — the full test suite (`cargo test` + Playwright UI tests).

See [`CLAUDE.md`](CLAUDE.md) for the full architecture rundown and command
reference, and [`docs/adrs/`](docs/adrs/README.md) for the reasoning behind
major design decisions.
