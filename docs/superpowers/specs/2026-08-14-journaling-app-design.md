# Journal — Architecture Design

Date: 2026-08-14
Status: Accepted (high-level); crypto scheme and sync conflict model get their
own follow-up specs before implementation.

## 1. Product summary

A free, open source (MIT), end-to-end encrypted journaling application.
Entries are Markdown with embedded images. The user's data is encrypted on
their device with a key derived from a passphrase; cloud storage (Dropbox
first) is used only as a dumb blob store that never sees plaintext.

Platforms, in order: desktop (Linux/macOS/Windows) → iOS → Android.

## 2. Requirements

Functional:

- F1 — Desktop support (Linux, macOS, Windows)
- F2 — Mobile support, iOS first, Android second
- F3 — Markdown authoring and rendering
- F4 — End-to-end encryption with Argon2 key derivation
- F5 — Sync to Dropbox, extensible to other cloud providers
- F6 — Images embedded in entries

Technical:

- T1 — Maximize code shared across platforms
- T2 — Security is a first-class constraint, not a later hardening pass
- T3 — Backend/core logic in Rust

## 3. Threat model

What we defend against:

- **Cloud provider compromise or subpoena.** Dropbox (or any provider) only
  ever holds AEAD ciphertext plus a small amount of unavoidable metadata
  (object count, sizes, modification times). No filenames, titles, tags, or
  timestamps in cleartext.
- **Network attacker.** TLS for transport, but correctness does not depend on
  it — payloads are already encrypted before they leave the device.
- **Device theft while locked.** Vault at rest is ciphertext; the data key
  exists only in process memory after unlock, and is zeroized on lock/exit.

What we explicitly do *not* defend against (documented, not hidden):

- A compromised OS or a device with malware while the vault is unlocked.
- An attacker who knows the passphrase. There is no server-side second factor
  — this is a local-first, zero-knowledge design by choice.
- Traffic analysis of sync volume and timing.

Consequence of the model: **losing the passphrase means losing the data.**
A printable recovery code is offered at vault creation as the only escape
hatch, and it is a second wrapping of the same data key — not an escrow.

## 4. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ UI (TypeScript, React) — same code on desktop and mobile      │
│  editor (CodeMirror 6) · entry list · settings · sync status   │
└──────────────────────┬───────────────────────────────────────┘
                       │ Tauri IPC (typed commands, allowlisted)
┌──────────────────────┴───────────────────────────────────────┐
│ Tauri 2 shell — desktop + iOS + Android from one project      │
│  custom protocol handler for decrypted images                 │
└──────────────────────┬───────────────────────────────────────┘
                       │ plain Rust calls
┌──────────────────────┴───────────────────────────────────────┐
│ Rust core (platform-independent, the bulk of the codebase)    │
│                                                               │
│  journal-crypto   Argon2id KDF, AEAD, key hierarchy, zeroize  │
│  journal-store    encrypted blob vault + derived SQLite index  │
│  journal-model    entries, attachments, vault manifest         │
│  journal-sync     provider trait, change detection, conflicts  │
│  journal-dropbox  Dropbox provider (OAuth PKCE)                │
└──────────────────────────────────────────────────────────────┘
```

The rule that keeps T1 and T2 honest: **keys and plaintext-at-rest never
cross the IPC boundary.** The UI receives decrypted entry text (it must, to
render it) but never the passphrase-derived keys, never raw vault files, and
never image bytes as base64 — images are served through a custom protocol
handler so they stream from Rust to the WebView without transiting JS.

### 4.1 Why Tauri 2

Chosen over Dioxus, Flutter, and per-platform native UI because it ships
desktop + iOS + Android from a single UI codebase while keeping every
security-relevant line in Rust, and because the web ecosystem gives us a
mature Markdown editor and image handling that we would otherwise have to
write ourselves. The accepted costs: a JavaScript layer that must be treated
as a partially trusted rendering surface (locked-down CSP, no remote content,
no `eval`, devtools disabled in release), and per-platform WebView quirks.

## 5. Cryptography

Scheme (to be ratified in its own spec before any crypto code lands):

```
passphrase ──Argon2id(salt, m/t/p)──► KEK (32 B)
                                       │
recovery code ──Argon2id(salt2)──► RKEK│
                                       ▼
                            wrapped DEK  ──unwrap──► DEK (vault data key)
                                       │
                          HKDF(DEK, "entry"|id) ──► per-entry key
```

- **KDF**: Argon2id, parameters calibrated per device class at vault creation
  and stored in the (public) vault header, with a documented floor so a weak
  device cannot silently pick weak parameters.
- **AEAD**: XChaCha20-Poly1305. Random 192-bit nonces make nonce reuse a
  non-issue given the number of objects a journal will ever hold.
- **Per-object keys** derived via HKDF from the DEK, so a single object's key
  never unlocks the vault and re-keying is possible without re-encrypting
  everything.
- **Associated data** binds each ciphertext to its object ID and version, so
  the sync layer cannot be tricked into swapping or rolling back objects.
- **Hygiene**: `zeroize` on all key material, `secrecy` wrappers to prevent
  accidental logging/serialization, no key material in `Debug` output.
- Crates: `argon2`, `chacha20poly1305`, `hkdf`, `zeroize`, `secrecy`,
  `getrandom`. No hand-rolled primitives, ever.

## 6. Storage

Two layers, with a deliberate split:

- **The vault** — the source of truth. Content-addressed encrypted blobs plus
  an encrypted manifest, on the local filesystem. Sync-friendly: an object is
  immutable once written, so syncing is "copy the objects the other side is
  missing".
- **The index** — a local SQLite database holding decrypted titles, dates,
  tags, and full-text search. It is **derived and disposable**: it can be
  rebuilt from the vault at any time, it is never synced, and it is deleted
  when the vault is locked on shared/mobile devices. This keeps search fast
  without inventing a second thing to encrypt correctly.

Images are ordinary blobs. Markdown references them by content hash
(`journal://blob/<hash>`), which deduplicates identical images automatically
and means an entry's text carries no filesystem paths.

## 7. Sync

`journal-sync` owns the algorithm; providers are dumb:

```rust
trait SyncProvider {
    async fn list(&self, prefix: &str) -> Result<Vec<RemoteObject>>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn put(&self, key: &str, body: &[u8]) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}
```

Dropbox is the first implementation (OAuth 2.0 with PKCE — no client secret
shipped in a binary). Because the trait is this small, adding Google Drive,
WebDAV, S3, or a plain synced folder is a contained piece of work.

Conflict handling is the hardest part of the project and gets its own spec.
The starting position: immutable object versions plus a manifest make most
"conflicts" merges of disjoint object sets; genuine same-entry divergence
produces a conflict copy rather than silent data loss. CRDT-based merging is
a possible later refinement, not a milestone-1 commitment.

## 8. Roadmap

| Milestone | Scope |
|---|---|
| **M1** | Rust core + encrypted local vault + Markdown editing + images, desktop only. No sync, no mobile. |
| **M2** | Sync engine + Dropbox provider, conflict handling, desktop only. |
| **M3** | iOS build: Keychain-wrapped key, biometric unlock, mobile editor UX. |
| **M4** | Android build (Keystore), plus a second sync provider to prove the trait. |
| **M5** | Hardening pass: external crypto review, reproducible builds, signed releases. |

M1 is deliberately narrow: the crypto and storage layers are the parts that
are expensive to change later, so they get proven before sync and mobile
toolchains are added on top.

## 9. Open questions

- Exact Argon2id parameter floors per device class (desktop vs. phone).
- Whether the derived index needs encryption-at-rest on mobile, or whether
  "delete on lock" is sufficient.
- Conflict-resolution model — deferred to the M2 spec.
- Whether to support plaintext export/import at M1 (useful for trust and for
  escaping the app; also the single easiest way for a user to leak their own
  journal).
