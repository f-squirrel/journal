# M1 — Encrypted local vault (desktop)

Design: [`../specs/2026-08-14-journaling-app-design.md`](../specs/2026-08-14-journaling-app-design.md)

Goal: a desktop app that creates a passphrase-protected vault, writes and
renders Markdown entries with embedded images, and stores everything as
ciphertext on disk. No sync, no mobile.

Done when: a user can create a vault, write an entry with an image, quit,
reopen, unlock with the passphrase, and see the entry — and every byte the app
wrote **anywhere on disk** is unreadable without that passphrase or the
recovery code. "Anywhere" means the whole application data directory, not just
the vault folder: the search index, temp files, and WebView caches all count,
and Task 16 is what proves it.

Conventions for every task: write the failing test first, then the
implementation, then tick the box. Paths are repo-relative.

## Task 1 — Workspace skeleton

- [x] Root `Cargo.toml` workspace with members `crates/journal-model`,
      `crates/journal-crypto`, `crates/journal-store`, `apps/desktop` (Tauri).
- [x] `rust-toolchain.toml` pinning a stable version and the `rustfmt` and
      `clippy` components; `rustfmt.toml`.
- [x] Lints go in `[workspace.lints.clippy]` in the root `Cargo.toml`, inherited
      by each crate; CI adds `-D warnings` through `RUSTFLAGS`. (`clippy.toml`
      configures lint *thresholds*, it cannot deny warnings.)
- [x] `deny.toml` for `cargo deny check` — advisories, licenses, bans. Commit
      `Cargo.lock`: this workspace ships binaries.
- [x] `ui/` — Vite + React + TypeScript, wired as the Tauri frontend.
- [x] `.github/workflows/ci.yml`: fmt, clippy, `cargo test`, `cargo deny check`,
      build on Linux/macOS/Windows.
- [x] Verify: `cargo test` passes and `cargo tauri dev` opens a window
      rendering "hello". `cargo fmt --check`, `cargo clippy --workspace
      --all-targets` (`-D warnings`), and `cargo deny check` are all clean too.
      The frontend renders `<h1>hello</h1>` (confirmed in the built bundle);
      this sandbox has no display server tooling to screenshot a real
      `tauri dev` window, so the release binary was instead run headlessly
      under `xvfb-run` and stayed up (event loop running, no panic/error)
      until killed — GUI-visual confirmation is left for a machine with a
      real display.

## Task 2 — Crypto spec ratified

- [ ] Write `docs/superpowers/specs/<date>-crypto-design.md`, dated the day it
      is written. Contents: key hierarchy and the slot list, Argon2id floor and
      weakest-device ceiling with the calibration procedure, AEAD envelope
      framing byte for byte, associated-data construction, header layout
      including the reserved rollback-counter field, the object-naming HMAC,
      and the recovery-code alphabet and entropy.
- [ ] Review it before writing crypto code. This task is a document, on
      purpose — the scheme is the expensive thing to get wrong, and Task 16's
      independent decryptor is written from this document alone.

## Task 3 — `journal-model`

- [ ] `crates/journal-model/src/{entry.rs,attachment.rs,manifest.rs,ids.rs}`:
      entry, attachment, manifest, tombstone, and object-ID types.
- [ ] Serialization is explicit and versioned — no `#[serde(flatten)]`, no
      silent acceptance of unknown fields.
- [ ] Verify (test first): round-trip tests, plus a golden-file test so an
      accidental format change fails loudly instead of migrating silently.

## Task 4 — `journal-crypto`: KDF and calibration

- [ ] `crates/journal-crypto/src/kdf.rs`: Argon2id over parameters read from
      the vault header.
- [ ] Calibration routine choosing parameters for the current device, clamped
      to both the documented floor and the weakest-supported-device ceiling
      (design §5).
- [ ] Verify (tests first): known-answer tests against RFC 9106 vectors;
      calibration on the CI runner lands inside the documented bounds; opening
      a vault whose header parameters sit below the floor is rejected.

## Task 5 — `journal-crypto`: envelope and key hierarchy

- [ ] `crates/journal-crypto/src/aead.rs`: XChaCha20-Poly1305 seal/open over
      the versioned envelope, with object ID and format version bound as
      associated data.
- [ ] `crates/journal-crypto/src/derive.rs`: HKDF for per-object keys and for
      `K_name` (object naming, design §6).
- [ ] `crates/journal-crypto/src/key.rs`: key types wrapped in
      `zeroize`/`secrecy`, no key material in `Debug`.
- [ ] `fuzz/fuzz_targets/envelope_parse.rs`: a `cargo-fuzz` target over the
      envelope decoder. It will be parsing attacker-influenced bytes the moment
      sync exists; the target costs an hour now.
- [ ] Verify (tests first): round trip; tampered ciphertext rejected;
      truncated envelope rejected; cross-object substitution rejected; version
      downgrade rejected; a `trybuild` compile-fail test asserting key types
      implement neither `Serialize` nor `Debug`; zeroization-on-drop test.

## Task 6 — `journal-crypto`: key slots and recovery code

- [ ] `crates/journal-crypto/src/slots.rs`: the slot list
      `{type, kdf_params, salt, wrapped_DEK}`; unwrap tries slots by type.
- [ ] Recovery code: generate ≥128 bits, encode in a transcription-friendly
      alphabet (no `0`/`O`, no `1`/`l`), parse case- and separator-insensitively.
- [ ] Passphrase change rewraps the passphrase slot only — no object is
      re-encrypted.
- [ ] Verify (tests first): unlock through the passphrase slot and through the
      recovery slot; wrong passphrase rejected; after a passphrase change every
      object is byte-identical and the recovery code still works; malformed
      recovery codes are rejected without panicking.

## Task 7 — `journal-store`: vault format and atomic IO

- [ ] `crates/journal-store/src/layout.rs`: header, encrypted manifest, blob
      directory, with the exact paths under the vault root.
- [ ] `crates/journal-store/src/atomic.rs`: temp file + fsync + rename, plus an
      fsync of the parent directory.
- [ ] Create / unlock / lock a vault. An unknown major format version refuses
      to open and never writes (design §5).
- [ ] Typed errors for the ugly cases: corrupt header, truncated object, disk
      full, vault already open.
- [ ] Verify (tests first): a future format version is refused; a
      killed-mid-write test (write the temp file, abort before rename) leaves
      the vault openable and unchanged; disk-full surfaces as an error rather
      than a panic or a half-written object.

## Task 8 — `journal-store`: manifest, entries, tombstones

- [ ] `crates/journal-store/src/entries.rs`: create / read / update / delete,
      each going through `journal-crypto`.
- [ ] Delete writes a manifest tombstone rather than silently dropping the
      object (design §6).
- [ ] Manifest writes are serialized through a single owner — two writers must
      not interleave.
- [ ] Verify (tests first): property test that any entry written reads back
      byte-identical; a deleted entry leaves listings and leaves a tombstone;
      concurrent writes from two handles do not corrupt the manifest.

## Task 9 — `journal-store`: attachments and object naming

- [ ] `crates/journal-store/src/blobs.rs`: put/get named
      `HMAC(K_name, plaintext_hash)` (design §6).
- [ ] Garbage-collection pass reclaiming blobs that no entry references.
- [ ] Verify (tests first): the same image added twice stores one blob; a
      blob's on-disk name equals no hash of its plaintext — this is the
      confirmation-oracle regression test and it should read like one; GC drops
      unreferenced blobs and keeps referenced ones.

## Task 10 — `journal-store`: in-memory index

- [ ] `crates/journal-store/src/index.rs`: SQLite `:memory:` with FTS5 over
      decrypted titles and bodies, built on unlock and dropped on lock. No part
      of it touches disk (design §6).
- [ ] Rebuild-from-vault is the *only* path — there is no incremental on-disk
      state that can drift out of sync with the vault.
- [ ] Verify (tests first): search results match a brute-force scan of the
      vault; the index is empty after lock; the rebuild time for a 1,000-entry
      vault is printed by the test, so the threshold that would force an
      encrypted on-disk index is a measurement rather than a guess.

## Task 11 — Tauri commands and IPC surface

- [ ] `apps/desktop/src/commands.rs`: `vault_create`, `vault_unlock`,
      `vault_lock`, `vault_change_passphrase`, `recovery_code_show` (creation
      only), `entry_list`, `entry_get`, `entry_save`, `entry_delete`,
      `attachment_add`, `export_vault`.
- [ ] The passphrase enters Rust and never returns. No command returns key
      material, slot bytes, or vault file paths.
- [ ] Logging policy: entry bodies, titles, passphrases, and key material never
      reach `tracing`, at any level. State it in the module docs; Task 16
      checks it.
- [ ] Verify (tests first): an integration test per command, plus a test
      asserting every command's return type is on an explicit allow-list of
      types known to carry no secrets.

## Task 12 — Blob protocol and shell lockdown

- [ ] `apps/desktop/src/protocol.rs`: `journal://blob/<name>` streaming
      decrypted images to the WebView, served with `Cache-Control: no-store` —
      WKWebView and WebView2 will otherwise cache these responses to disk in
      plaintext, which would defeat the milestone's done-criterion through the
      one path built to protect it.
- [ ] Strict CSP, no remote origins, devtools off in release, filesystem and
      shell APIs not enabled, `tauri.conf.json` allowlist reviewed line by line.
- [ ] Verify: an unknown blob name 404s without revealing whether it exists;
      the CSP is asserted by a test rather than eyeballed; the on-disk cache
      behaviour is covered by Task 16's sweep.

## Task 13 — Editor UI

- [ ] `ui/src/`: entry list, editor, and preview. CodeMirror 6 with Markdown
      mode.
- [ ] Image insert by paste and drag-drop → `attachment_add` → a
      `journal://blob/<name>` reference written into the Markdown.
- [ ] Failure states are visible rather than silent: save failed, vault
      corrupt, disk full.
- [ ] Verify: run the app, create a vault, write an entry with a pasted image,
      restart, unlock, confirm it renders. Confirm a wrong passphrase fails.

## Task 14 — Unlock, auto-lock, teardown

- [ ] Unlock screen. Vault creation shows the recovery code once, requires
      confirmation that it was written down, and shows calibration progress —
      Argon2id calibration is slow enough to look like a hang.
- [ ] Auto-lock after inactivity (default 15 minutes, configurable): Rust
      zeroizes keys and drops the index, and the shell **reloads the WebView**
      to discard the plaintext heap (design §3, §4).
- [ ] Verify: after auto-lock the reloaded WebView shows the unlock screen with
      no entry content surviving in the page, the index is gone, and a
      subsequent unlock works.

## Task 15 — Export and import

- [ ] `export_vault` writes a plaintext tree of Markdown files plus images,
      behind an explicit confirmation naming what is about to be written and
      where.
- [ ] Import of that same layout, so the round trip is real rather than a
      one-way exit.
- [ ] Verify: export → import into a fresh vault reproduces the same entries
      and attachments; the confirmation cannot be bypassed at the command layer.

## Task 16 — Format oracle and leak audit

- [ ] `tools/decrypt.py`: an independent decryptor written from the crypto spec
      alone, not from the Rust source. If it cannot open a vault this app
      wrote, the spec is wrong or incomplete — which is the point. It doubles
      as a user's escape hatch.
- [ ] Leak sweep over the **whole app data directory** — vault, index, temp
      files, WebView cache, crash dumps. Write an entry containing a
      high-entropy canary, exercise the app, then fail if the canary or a known
      image byte-run appears anywhere on disk.
- [ ] No-network assertion: M1 makes zero outbound connections. Assert it in CI.
- [ ] Log audit: run the app at maximum verbosity and grep the output for the
      canary.
- [ ] Verify: all four run in CI, and the leak sweep runs *after* Task 12's
      protocol handler has actually served images.

## Task 17 — Ship-readiness

- [ ] `README.md`: what it is, the security model in plain language, build
      steps.
- [ ] `SECURITY.md`: the design's "we do not defend against" list stated
      honestly — including shape metadata, swap, the WebView heap, and
      whole-vault rollback — plus a disclosure contact.
- [ ] Release builds for the three desktop platforms in CI, **unsigned**:
      notarization and Authenticode land in M5, so the release notes must say
      that Gatekeeper and SmartScreen will warn, and give checksums to verify
      against instead.
- [ ] Verify: a clean checkout builds and runs from the README alone, on a
      machine that has never built this project.

## Not in this milestone

Sync, Dropbox, mobile, sharing, multi-vault, padding of object sizes, and
whole-vault rollback protection (Task 2 reserves the header field; the
mechanism is M2), signed releases. Each has a later milestone or its own spec.

## Revision notes

**2026-08-14** — revised alongside the design doc after a critical review,
before any execution. The previous version had seven tasks; tasks 3–6 there
were multi-day epics rather than checklist items, carried no file paths, and
put verification after implementation. Beyond the restructuring, this version
adds work that was missing outright: the recovery code was implemented but
never surfaced through a command or the UI, there were no `entry_delete` or
`vault_change_passphrase` commands, and nothing covered export, corrupt-vault
handling, fuzzing, logging policy, or the WebView cache. See the design doc's
§10 for the format-level changes that drove the rest.
