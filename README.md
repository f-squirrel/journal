# journal

A free, open source, end-to-end encrypted journaling application. Entries
are Markdown with embedded images. Data is encrypted on your device with a
key derived from a passphrase; cloud sync (Dropbox first) is used only as a
dumb blob store that never sees plaintext.

Platforms, in order: desktop (Linux/macOS/Windows) → iOS → Android.

**Status:** early scaffolding. The workspace, CI, and crate skeletons exist;
the crypto, storage, and UI layers described below are not implemented yet.
See [Roadmap](#roadmap).

## Security model

- End-to-end encryption: the app encrypts on-device; a sync provider only ever
  holds ciphertext. The first milestone seals the vault with
  [age](https://github.com/C2SP/C2SP/blob/main/age.md) (scrypt-derived keys);
  Argon2id-derived keys arrive with the native backend in M2, behind a
  swappable seam designed for exactly that change.
- Losing your passphrase means losing your data — a one-time recovery code
  is offered as the sole escape hatch (a second wrapping of the same key,
  not an escrow).
- What this does *not* defend against — a compromised/malware-infected OS
  while unlocked, traffic-shape analysis, swap/hibernation capturing keys,
  and whole-vault rollback — is documented, not hidden.

Full threat model and architecture:
[`docs/superpowers/specs/2026-08-14-journaling-app-design.md`](docs/superpowers/specs/2026-08-14-journaling-app-design.md).
The cryptographic scheme is specified to the byte — key hierarchy, key
slots, Argon2id parameters, envelope format, object naming, recovery code —
in [`docs/superpowers/specs/2026-08-14-crypto-design.md`](docs/superpowers/specs/2026-08-14-crypto-design.md),
so the format can be re-implemented (and audited) without reading this
codebase.

## Architecture

```
ui/                    React + TypeScript frontend (Tauri webview)
apps/desktop/           Tauri 2 shell — desktop entry point, IPC commands
crates/journal-model/   Shared domain types: entries, attachments, manifest
crates/journal-crypto/  Swappable sealing backend (age at M1), keys, naming
crates/journal-store/   Encrypted on-disk vault + in-memory search index
```

The same Rust core and UI code are meant to be shared across desktop and
(later) mobile, with all security-relevant logic living in Rust — the UI
never sees passphrase-derived keys, only decrypted content it needs to
render.

## Prerequisites

- Rust, pinned via [`rust-toolchain.toml`](rust-toolchain.toml) (currently
  1.97.1) with the `rustfmt` and `clippy` components — `rustup` installs the
  right version automatically.
- Node.js 22+ and npm, for the `ui/` frontend.
- [Tauri system dependencies](https://tauri.app/start/prerequisites/) for
  your OS (on Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
  `libayatana-appindicator3-dev`, `librsvg2-dev`,
  `libjavascriptcoregtk-4.1-dev`, `libsoup-3.0-dev`).
- `cargo install tauri-cli` (or use `cargo tauri` via `cargo-binstall`) for
  `cargo tauri dev` / `cargo tauri build`.
- `cargo install cargo-deny` if you want to run the license/advisory check
  locally.

## Building and running

```bash
# install UI dependencies
npm --prefix ui ci

# run the desktop app in dev mode (starts the Vite dev server too)
cargo tauri dev

# build a release bundle
cargo tauri build
```

To work on the Rust workspace alone (no Tauri shell):

```bash
cargo build --workspace
cargo test --workspace
```

To work on the UI alone:

```bash
cd ui
npm run dev      # Vite dev server on http://localhost:1420
npm run build    # tsc -b && vite build
npm run lint      # oxlint
```

## Checks CI runs

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets    # RUSTFLAGS=-D warnings
cargo test --workspace
cargo deny check
npm --prefix ui run build
cargo build --workspace --release
```

## Architecture decisions

[`docs/adrs/`](docs/adrs/README.md) records why this project is built the way
it is — one Architecture Decision Record per decision, in the
[adr.github.io](https://adr.github.io/) format (Status, Context, Decision,
Consequences). Start there if you're wondering why M1 uses `age` instead of
Argon2id, why object names are keyed rather than plain content hashes, why the
search index is never written to disk, or what the project deliberately does
*not* defend against. New architectural decisions get a new numbered record;
reversals supersede rather than rewrite.

The specs remain the normative source for byte-level detail — where an ADR and
a spec disagree, the spec wins.

## Project docs and workflow

This repo plans and executes work using the `superpowers` Claude Code
plugin. See [`docs/superpowers/README.md`](docs/superpowers/README.md) for
the brainstorm → plan → execute → review lifecycle, and
[`docs/superpowers/plans/INDEX.md`](docs/superpowers/plans/INDEX.md) for
current project status.

## Roadmap

| Milestone | Scope |
|---|---|
| **M1** | Rust core + encrypted local vault (age backend) + Markdown editing + images + plaintext export, desktop only. No sync, no mobile. |
| **M2** | Sync engine + Dropbox provider, conflict handling, desktop only. Plus the native Argon2id backend, which migrates M1 vaults. |
| **M3** | iOS build: Keychain-wrapped key, biometric unlock, mobile editor UX. |
| **M4** | Android build (Keystore), plus a second sync provider to prove the trait. |
| **M5** | Hardening pass: external crypto review, reproducible builds, signed releases. |

## License

MIT — see [`LICENSE`](LICENSE).
