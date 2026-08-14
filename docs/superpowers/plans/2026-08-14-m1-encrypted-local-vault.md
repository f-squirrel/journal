# M1 — Encrypted local vault (desktop)

Design: [`../specs/2026-08-14-journaling-app-design.md`](../specs/2026-08-14-journaling-app-design.md)

Goal: a desktop app that creates a passphrase-protected vault, writes and
renders Markdown entries with embedded images, and stores everything as
ciphertext on disk. No sync, no mobile.

Done when: a user can create a vault, write an entry with an image, quit,
reopen, unlock with the passphrase, and see the entry — and every byte the
app wrote to disk is unreadable without that passphrase.

## Task 1 — Workspace skeleton

- [ ] Cargo workspace at repo root with member crates `journal-model`,
      `journal-crypto`, `journal-store`, and `apps/desktop` (Tauri).
- [ ] `rust-toolchain.toml` pinning a stable version; `rustfmt.toml`,
      `clippy.toml` with `-D warnings` in CI.
- [ ] `ui/` — Vite + React + TypeScript, wired as the Tauri frontend.
- [ ] CI (GitHub Actions): fmt, clippy, `cargo test`, `cargo deny check`,
      build on Linux/macOS/Windows.
- [ ] Verify: `cargo test` and a `tauri dev` window that renders "hello".

## Task 2 — Crypto spec ratified

- [ ] Write `specs/2026-08-14-crypto-design.md`: exact key hierarchy, Argon2id
      parameter floors, AEAD framing, associated-data binding, versioned
      header format, recovery-code encoding.
- [ ] Review it before writing crypto code. This task is a document, on
      purpose — the scheme is the expensive thing to get wrong.

## Task 3 — `journal-crypto`

- [ ] Argon2id KDF with parameters from the vault header; calibration routine
      that picks parameters for the current device against a documented floor.
- [ ] XChaCha20-Poly1305 seal/open over a versioned envelope, with object ID
      and version bound as associated data.
- [ ] Key hierarchy: KEK wraps DEK; HKDF derives per-object keys.
- [ ] Recovery code: generate, encode, and use as a second DEK wrapping.
- [ ] `zeroize`/`secrecy` on all key types; no key material in `Debug`.
- [ ] Verify: round-trip tests, tampered-ciphertext rejection, wrong-passphrase
      rejection, cross-object substitution rejection, known-answer tests for
      the KDF, and a test asserting keys are zeroized on drop.

## Task 4 — `journal-store`

- [ ] Vault layout on disk: header, encrypted manifest, content-addressed
      blob directory.
- [ ] Create / unlock / lock a vault; atomic writes (temp + rename, fsync).
- [ ] Entry CRUD and attachment put/get, all through `journal-crypto`.
- [ ] Derived SQLite index with FTS over decrypted titles and bodies, plus a
      rebuild-from-vault path.
- [ ] Verify: property test that any entry written is byte-identical on read
      back; a test that greps the entire vault directory for known plaintext
      and fails if it finds any; index-rebuild equivalence test.

## Task 5 — Tauri commands and IPC surface

- [ ] Typed commands: `vault_create`, `vault_unlock`, `vault_lock`,
      `entry_list`, `entry_get`, `entry_save`, `attachment_add`.
- [ ] Passphrase enters Rust and never returns; no command exposes key
      material or vault file paths.
- [ ] Custom `journal://blob/<hash>` protocol handler streaming decrypted
      images to the WebView.
- [ ] Lock the shell down: strict CSP, no remote origins, devtools off in
      release, filesystem/shell APIs not enabled.
- [ ] Verify: an integration test per command, plus a review pass confirming
      no command returns secrets.

## Task 6 — Editor UI

- [ ] Entry list, editor, and preview. CodeMirror 6 with Markdown mode.
- [ ] Unlock screen; auto-lock after inactivity, clearing in-memory state.
- [ ] Image insert via paste and drag-drop → `attachment_add` → hash reference
      written into the Markdown.
- [ ] Verify: run the app, create a vault, write an entry with a pasted image,
      restart, unlock, confirm it renders. Confirm a wrong passphrase fails.

## Task 7 — Ship-readiness

- [ ] `README.md`: what it is, security model in plain language, build steps.
- [ ] `SECURITY.md` with the threat model's "we do not defend against" list
      stated honestly, and a disclosure contact.
- [ ] Release builds for the three desktop platforms in CI.
- [ ] Verify: a clean checkout builds and runs from the README alone.

## Not in this milestone

Sync, Dropbox, mobile, sharing, multi-vault, plaintext export. Each has a
later milestone or its own spec.
