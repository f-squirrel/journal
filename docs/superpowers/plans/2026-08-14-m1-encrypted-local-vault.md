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
- [x] Review docs (use `/docs-review-guide`) to ensure this change is reflected
      in CLAUDE.md, workspace layout docs, and CI/build documentation.

## Task 2 — Crypto spec ratified

- [x] Write `docs/superpowers/specs/<date>-crypto-design.md`, dated the day it
      is written. Contents: key hierarchy and the slot list, Argon2id floor and
      weakest-device ceiling with the calibration procedure, AEAD envelope
      framing byte for byte, associated-data construction, header layout
      including the reserved rollback-counter field, the object-naming HMAC,
      and the recovery-code alphabet and entropy.
      Written as [`../specs/2026-08-14-crypto-design.md`](../specs/2026-08-14-crypto-design.md).
- [x] Review it before writing crypto code. This task is a document, on
      purpose — the scheme is the expensive thing to get wrong, and Task 16's
      independent decryptor is written from this document alone.
      Review outcome recorded in the spec's §10 revision notes, including the
      one decision that diverges from this plan's literal wording: the
      recovery-code alphabet is Crockford Base32 (§4.5), confirmed with the
      user.
- [x] Review docs (use `/docs-review-guide`) to ensure this spec is reflected
      in CLAUDE.md security-critical conventions and linked from INDEX.md.

## Task 3 — `journal-model`

- [x] `crates/journal-model/src/{entry.rs,attachment.rs,manifest.rs,ids.rs}`:
      entry, attachment, manifest, tombstone, and object-ID types.
- [x] Serialization is explicit and versioned — no `#[serde(flatten)]`, no
      silent acceptance of unknown fields.
- [x] Verify (test first): round-trip tests, plus a golden-file test so an
      accidental format change fails loudly instead of migrating silently.
- [x] Review docs (use `/docs-review-guide`) to ensure workspace layout and
      crate structure in CLAUDE.md match the implementation.

## Task 4 — `journal-crypto`: the backend seam and naming

M1 seals objects with `age` rather than a hand-written envelope (crypto spec
§5), so Argon2id and its calibration move to M2 with the native backend. What
stays in M1 is the seam that makes that swap possible, plus everything above
it.

- [x] `crates/journal-crypto/src/backend.rs`: the `KeyStore` / `ObjectSealer`
      traits, `Binding`, `ObjectKind`, `NameKey`, `RecoveryCode`,
      `UnlockSecret`, `CryptoError` — crypto spec §3, normative signatures.
      `Binding` is a required argument of both `seal` and `open` so a caller
      cannot open an object without stating which object they believe it is.
- [x] `crates/journal-crypto/src/naming.rs`: `K_name` derivation and
      `HMAC(K_name, kind || SHA-256(content))` truncated to 160 bits, encoded
      with the `base32` crate's Crockford alphabet (spec §4.4). The hash covers
      the object's **content**, never the §5.4 preamble — the preamble carries
      the name tag, so hashing it would be circular. `K_name` is derived
      **per backend** (spec §4.4's table): full HKDF over the age identity
      string, Expand-only over the native DEK. They are not interchangeable.
      Names are computed for a `ContentKind` (entry or attachment) rather than
      an `ObjectKind`, so the manifest — which is not content-addressed — has
      no name to invent.
- [x] Verify (tests first): the golden naming vectors of spec §9 item 2 —
      **both** the age and native `K_name` rows, since the native one is
      computable today and is the likeliest thing to be implemented as if the
      two constructions were one; a blob's on-disk name equals no hash of its
      plaintext; the same content under two different vault secrets yields two
      different names.
      Vectors live in `crates/journal-crypto/tests/vectors/naming.json`,
      computed from the spec text by an independent implementation rather than
      from this crate's output.
- [x] Review docs (use `/docs-review-guide`) to ensure the seam and naming
      match the spec.

## Task 5 — `journal-crypto`: the age backend

- [ ] `crates/journal-crypto/src/age_backend/slots.rs`: `slots/passphrase.age`
      and `slots/recovery.age`, each an age file whose plaintext is the vault
      identity string (spec §5.2, §5.3). Work factors are **pinned** —
      `set_work_factor(17)` on write, `set_max_work_factor(18)` on read —
      because age otherwise calibrates to the encrypting device and *rejects*
      files whose factor exceeds the decrypting device's target by more than 4.
- [ ] `crates/journal-crypto/src/age_backend/sealer.rs`: seal/open as an age
      file over `preamble || content`, with the 52-byte binding preamble of
      spec §5.4 verified with a constant-time comparison **before** any
      plaintext is returned.
- [ ] `crates/journal-crypto/src/key.rs`: key types wrapped in
      `zeroize`/`secrecy`, no key material in `Debug`. The X25519 **recipient**
      is key material too (spec §8): age authenticates no sender, so holding
      the recipient is enough to forge any object under any `Binding`. It never
      reaches disk, logs, IPC, or an export path.
- [ ] Passphrase change rewrites `slots/passphrase.age` only — no object is
      re-sealed and the recovery code still works.
- [ ] All slot writes are crash-atomic — temp file in the same directory,
      fsync, rename, fsync the directory — and vault creation writes
      `header.jvh` **last** (spec §5.2). A torn `passphrase.age` costs the user
      their passphrase.
- [ ] Verify (tests first): round trip; tampered ciphertext rejected;
      truncated file rejected; **cross-object substitution rejected** (seal
      under binding A, open under binding B → `BindingMismatch`); a vault
      written at work factor 17 opens on a client whose own target is lower;
      a slot file at `log_n = 19` is refused rather than allocating 512 MiB;
      a `trybuild` compile-fail test asserting key types implement neither
      `Serialize` nor `Debug`; zeroization-on-drop test.
- [ ] Review docs (use `/docs-review-guide`) to ensure CLAUDE.md security
      conventions about key material, serialization, and Debug implementations
      are still accurate.

## Task 6 — `journal-crypto`: recovery code and unlock

- [ ] Recovery code: 160 bits in Crockford Base32, parsed case-, separator-,
      and confusable-insensitively — crypto spec §4.5, which supersedes this
      line's original "no `0`/`O`, no `1`/`l`" wording (the ambiguous *letters*
      are absent from the alphabet and decode to their digits instead).
- [ ] `KeyStore::unlock` tries only the slot matching the attempt type — a
      passphrase is never run against the recovery slot — and returns a single
      generic failure that reveals nothing about which slot was tried.
- [ ] Nothing is written to disk on a failed unlock: no counter, no timestamp.
- [ ] Verify (tests first): unlock through the passphrase slot and through the
      recovery slot; wrong passphrase rejected; after a passphrase change every
      object is byte-identical and the recovery code still works; malformed
      recovery codes are rejected without panicking; the sloppy-transcription
      vectors of spec §9 item 1 all decode to the same bytes.
- [ ] Review docs (use `/docs-review-guide`) to ensure recovery code spec and
      slot structure documentation in the crypto spec align with implementation.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure vault format versioning
      and error handling are documented accurately.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure tombstone and manifest
      design conventions are reflected in security-critical documentation.

## Task 9 — `journal-store`: attachments and object naming

- [ ] `crates/journal-store/src/blobs.rs`: put/get named
      `HMAC(K_name, plaintext_hash)` (design §6).
- [ ] Garbage-collection pass reclaiming blobs that no entry references.
- [ ] Verify (tests first): the same image added twice stores one blob; a
      blob's on-disk name equals no hash of its plaintext — this is the
      confirmation-oracle regression test and it should read like one; GC drops
      unreferenced blobs and keeps referenced ones.
- [ ] Review docs (use `/docs-review-guide`) to ensure object naming and
      blob storage design in the spec match the implementation.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure search index design and
      memory-only constraints in the spec match the code.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure CLAUDE.md security
      conventions about IPC boundaries, key material, and logging policy
      are upheld.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure security constraints
      and protocol design in CLAUDE.md align with implementation.

## Task 13 — Editor UI

- [ ] `ui/src/`: entry list, editor, and preview. CodeMirror 6 with Markdown
      mode.
- [ ] Image insert by paste and drag-drop → `attachment_add` → a
      `journal://blob/<name>` reference written into the Markdown.
- [ ] Failure states are visible rather than silent: save failed, vault
      corrupt, disk full.
- [ ] Verify: run the app, create a vault, write an entry with a pasted image,
      restart, unlock, confirm it renders. Confirm a wrong passphrase fails.
- [ ] Review docs (use `/docs-review-guide`) to ensure UI/frontend documentation
      is updated if needed.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure auto-lock behavior and
      security model are documented accurately.

## Task 15 — Export and import

- [ ] `export_vault` writes a plaintext tree of Markdown files plus images,
      behind an explicit confirmation naming what is about to be written and
      where.
- [ ] Import of that same layout, so the round trip is real rather than a
      one-way exit.
- [ ] Verify: export → import into a fresh vault reproduces the same entries
      and attachments; the confirmation cannot be bypassed at the command layer.
- [ ] Review docs (use `/docs-review-guide`) to ensure export/import behavior
      and security considerations are documented.

## Task 16 — Format oracle and leak audit

- [ ] `tools/decrypt.py`: an independent decryptor written from the crypto spec
      alone, not from the Rust source. If it cannot open a vault this app
      wrote, the spec is wrong or incomplete — which is the point. It doubles
      as a user's escape hatch. Under the age backend it may use `pyrage` for
      the container, but must build the §5.4 binding preamble, verify it, and
      strip it from its own reading of the spec.
- [ ] Leak sweep over the **whole app data directory** — vault, index, temp
      files, WebView cache, crash dumps. Write an entry containing a
      high-entropy canary, exercise the app, then fail if the canary or a known
      image byte-run appears anywhere on disk.
- [ ] No-network assertion: M1 makes zero outbound connections. Assert it in CI.
- [ ] Log audit: run the app at maximum verbosity and grep the output for the
      canary.
- [ ] Verify: all four run in CI, and the leak sweep runs *after* Task 12's
      protocol handler has actually served images.
- [ ] Review docs (use `/docs-review-guide`) to ensure testing, validation, and
      security audit procedures are accurately documented.

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
- [ ] Review docs (use `/docs-review-guide`) to ensure all documentation is
      complete, accurate, and ready for shipping (README, SECURITY, build docs).

## Not in this milestone

Sync, Dropbox, mobile, sharing, multi-vault, padding of object sizes, and
whole-vault rollback protection (Task 2 reserves the header field; the
mechanism is M2), signed releases. Each has a later milestone or its own spec.

## Deferred to M2 by the age decision

Choosing `age` for M1's sealing backend (crypto spec §5) bought implementation
speed by postponing work that is **specified and ready to build**, not work
that was dropped. Every item below has a byte-level home in the crypto spec
already; none of it needs re-designing, and none of it should be re-litigated.

These become the crypto half of M2's plan, alongside sync:

- [ ] **Argon2id and requirement F4.** The native backend's KDF, floor
      (64 MiB / t=3 / p=4, RFC 9106 §4), weakest-device ceiling (256 MiB /
      t=10), and the memory-first calibration procedure — crypto spec §6.4.
      F4 is unmet until this ships; the design doc's §2 says so.
- [ ] **The key-slot record and `SlotAD`.** The 116-byte slot record, the
      72-byte associated data that authenticates the header without a header
      MAC, slot types, and the additive-unknown-slot rule — spec §6.2.
- [ ] **The AEAD envelope and `ObjectAD`.** The 48-byte-overhead envelope and
      the 60-byte associated data that binds each ciphertext to its object —
      spec §6.5. This is the fail-closed version of what M1 enforces in code
      via the §5.4 binding preamble.
- [ ] **Per-object key derivation.** `K_obj` and `K_name` from the DEK via the
      30-byte HKDF `info` — spec §6.3.
- [ ] **`fuzz/fuzz_targets/envelope_parse.rs`.** A `cargo-fuzz` target over
      the envelope decoder, dropped from M1 because M1 has no envelope of its
      own to fuzz. It will be parsing attacker-influenced bytes the moment
      sync exists.
- [ ] **The age → native migration.** Re-seal and rename every object,
      rewrite the manifest, flip `backend_id` last and atomically — spec §7.
      Resumable by construction; a vault interrupted mid-migration still opens
      under the age backend.
- [ ] **Whole-vault rollback.** The reserved header counter (spec §4.2) and
      the `purpose = 3` MAC key gain their mechanism here.

Two further hardening items belong to **M5**, not M2: external review of the
native scheme against this spec, and Task 16's `tools/decrypt.py` gaining a
native-backend path so the independent decryptor covers both formats.

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
