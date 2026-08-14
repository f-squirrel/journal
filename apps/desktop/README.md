# desktop

The Tauri 2 desktop shell — the entry point that wires the `ui/` frontend
to the Rust core (`journal-model`, `journal-crypto`, `journal-store`) over
Tauri IPC. Builds for Linux, macOS, and Windows from one codebase.

**Status:** minimal skeleton. `run()` currently just boots a Tauri window;
no IPC commands or blob protocol handler exist yet. See Tasks 11–12 of
[`../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md`](../../docs/superpowers/plans/2026-08-14-m1-encrypted-local-vault.md).

## Layout

- `src/main.rs` — binary entry point; calls into the library.
- `src/lib.rs` — `run()`, the Tauri app builder (`#[tauri::mobile_entry_point]`
  so the same code path also serves future iOS/Android builds).
- `tauri.conf.json` — app identity, window config, dev server URL
  (`http://localhost:1420`), bundle targets. `beforeDevCommand`/
  `beforeBuildCommand` drive `npm --prefix ../../ui`.
- `build.rs` — `tauri-build` codegen.
- `icons/` — app icons for bundling.

## Planned contents

- `commands.rs` — the IPC command surface: `vault_create`, `vault_unlock`,
  `vault_lock`, `vault_change_passphrase`, `recovery_code_show`,
  `entry_list`/`entry_get`/`entry_save`/`entry_delete`, `attachment_add`,
  `export_vault`. The passphrase enters Rust and never returns; no command
  returns key material, slot bytes, or vault file paths.
- `protocol.rs` — the `journal://blob/<name>` custom protocol that streams
  decrypted images to the WebView (`Cache-Control: no-store`, so the
  WebView never caches plaintext to disk).

## Security conventions specific to this crate

- Strict CSP, no remote origins, no `eval`, devtools disabled in release,
  filesystem/shell APIs left disabled in `tauri.conf.json`.
- Entry bodies, titles, passphrases, and key material must never reach
  `tracing`, at any level.
- Auto-lock reloads the WebView (rather than trying to scrub it) to discard
  the plaintext heap — see §4 of
  [`../../docs/superpowers/specs/2026-08-14-journaling-app-design.md`](../../docs/superpowers/specs/2026-08-14-journaling-app-design.md).

## Running

From the repo root: `cargo tauri dev` (dev) or `cargo tauri build`
(release bundle). See the root [`README.md`](../../README.md) for
prerequisites.
