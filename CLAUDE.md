# journal

A free, open source, end-to-end encrypted journaling app (Markdown entries
with embedded images, Argon2id-derived keys, Dropbox as a dumb ciphertext
blob store). Desktop first (Tauri 2 + React), then iOS, then Android. Full
product/threat model/crypto design:
[`docs/superpowers/specs/2026-08-14-journaling-app-design.md`](docs/superpowers/specs/2026-08-14-journaling-app-design.md).
The byte-level cryptographic scheme (key hierarchy, slots, Argon2id bounds,
AEAD envelope, header layout, object naming, recovery code) is ratified
separately in
[`docs/superpowers/specs/2026-08-14-crypto-design.md`](docs/superpowers/specs/2026-08-14-crypto-design.md)
— it is normative, so implement against it rather than re-deriving a format.

Current status: early scaffolding (workspace, CI, and crate skeletons only —
see [`docs/superpowers/plans/INDEX.md`](docs/superpowers/plans/INDEX.md) for
what's actually implemented vs. planned). Don't assume crypto/storage/UI
functionality exists until the relevant plan task is checked off.

## Workspace layout

```
ui/                    React + TypeScript frontend (Tauri webview)
apps/desktop/           Tauri 2 shell — desktop entry point, IPC commands
crates/journal-model/   Shared domain types: entries, attachments, manifest
crates/journal-crypto/  Argon2id KDF, AEAD envelope, key hierarchy, zeroize
crates/journal-store/   Encrypted on-disk vault + in-memory search index
```

`crates/journal-sync` and `crates/journal-dropbox` are planned (M2) but do
not exist yet.

## Security-critical conventions

This is an E2E-encrypted app — treat these as hard rules, not style
preferences:

- Keys and plaintext-at-rest never cross the Tauri IPC boundary. No command
  returns key material, slot bytes, or raw vault file paths.
- Key material types must never implement `Serialize` or `Debug`, and must
  be wrapped in `zeroize`/`secrecy`. No hand-rolled crypto primitives.
- No key material, entry bodies, titles, or passphrases in `tracing`
  output, at any level.
- Serialization of on-disk types is explicit and versioned — no
  `#[serde(flatten)]`, no silent acceptance of unknown fields. A client that
  meets an unknown major format version refuses to open the vault and never
  writes to it.
- On-disk and in-memory crypto formats follow the crypto spec byte for byte
  — envelope framing, associated-data construction, header layout, Argon2id
  floor/ceiling, and object naming are specified there, not chosen at the
  call site. Changing any of them is a format-version change, not a refactor.
- See the design doc's §3 for what this app explicitly does *not* defend
  against — don't silently expand or narrow that list without updating it.

## Build, test, and lint

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets      # CI runs with RUSTFLAGS=-D warnings
cargo deny check                             # advisories, licenses, bans
npm --prefix ui ci && npm --prefix ui run build
npm --prefix ui run lint                     # oxlint
cargo tauri dev                              # run the desktop app
```

Lint policy: `[workspace.lints.clippy]` in the root `Cargo.toml` sets levels
(`all`, `pedantic`, `unwrap_used`, `expect_used` all warn) and CI denies
warnings via `RUSTFLAGS`; `clippy.toml` only configures lint *thresholds*,
it cannot deny warnings itself. `Cargo.lock` is committed — this workspace
ships binaries.

## Development workflow

This repo plans and executes work using the `superpowers` Claude Code plugin
(already enabled in `.claude/settings.json`). Before starting non-trivial
work, read [`docs/superpowers/README.md`](docs/superpowers/README.md) for
the full brainstorm → plan → execute → review lifecycle and where each
artifact lives (`docs/superpowers/specs/`, `docs/superpowers/plans/`,
`docs/superpowers/plans/INDEX.md`).

Commit messages follow Conventional Commits — see
`.claude/skills/conventional-commit-guide/SKILL.md`.
