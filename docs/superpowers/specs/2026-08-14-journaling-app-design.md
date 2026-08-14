# Journal — Architecture Design

Date: 2026-08-14
Status: Accepted (high-level); crypto scheme and sync conflict model get their
own follow-up specs before implementation.
Last revised: 2026-08-14 — see §10.

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
- F7 — Plaintext export and import, so the data is retrievable without this app

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
- **Shape metadata.** Object count, ciphertext sizes, and file modification
  times are visible to anyone holding the vault directory. For a journal that
  means entry lengths and *writing cadence* — when you wrote, and how often —
  stay inferable even though no title, tag, or entry timestamp is ever in
  cleartext. We do not pad and we do not generate cover traffic (see §9).
- **Process memory reaching disk.** Keys can be paged to swap or captured in a
  hibernation image by the OS. We do not `mlock`; on the target platforms it is
  either unreliable or needs privileges we decline to ask for.
- **Plaintext in the WebView heap while unlocked.** The UI must hold decrypted
  entry text in order to render it, and JavaScript memory cannot be zeroized.
  Auto-lock therefore tears the WebView down (§4) instead of pretending to
  scrub it.
- **Rollback of the whole vault** by an attacker with write access to the
  store, who serves an older but internally consistent snapshot. The
  per-object binding in §5 stops object-level swapping and rollback, not this.
  The mitigation is a client-remembered monotonic version; the header reserves
  the field at M1 and the mechanism ships with sync in M2.

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

Because that decrypted text then lives in a heap we cannot scrub, **locking is
a teardown, not a cleanup**: Rust zeroizes and drops the keys and the derived
index, and the shell reloads the WebView to a blank unlock screen so the old
heap is discarded wholesale rather than nulled out variable by variable.

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
passphrase ─────Argon2id(salt_p, m/t/p)──► KEK_pass ──┐
recovery code ──Argon2id(salt_r, m/t/p)──► KEK_rec  ──┤ each unwraps its own
device key ─────(Keychain/Keystore, M3+)──► KEK_dev ──┘ copy of the same DEK
                                                        │
                                                        ▼
                                              DEK (vault data key)
                                                        │
                             HKDF(DEK, "entry"|id) ──► per-entry key
                             HKDF(DEK, "name")     ──► K_name (naming, §6)
```

- **Key slots.** The header carries a *list* of slots, LUKS-style, each
  `{type, kdf_params, salt, wrapped_DEK}`. Passphrase and recovery slots exist
  from M1; the biometric/device wrapping in M3 is then a new row rather than a
  format migration. Retrofitting a third wrapping into a single-slot header is
  precisely the expensive mistake this avoids.
- **KDF**: Argon2id, parameters calibrated at vault creation and stored in the
  (public) vault header. The binding constraint is a *ceiling*, not a floor: a
  vault must stay openable on the weakest device its owner will ever unlock it
  on, so calibration targets that device class — subject to a documented
  minimum, so a weak device cannot silently pick weak parameters.
- **AEAD**: XChaCha20-Poly1305. Random 192-bit nonces make nonce reuse a
  non-issue given the number of objects a journal will ever hold.
- **Per-object keys** derived via HKDF from the DEK, so compromise of one
  object's key never unlocks the vault. The two rotations this enables are not
  equally cheap, and the difference matters: **changing the passphrase**
  rewraps the DEK in one slot and touches no object, while **rotating the DEK**
  re-derives every per-object key and therefore re-encrypts the entire vault.
  The first is routine, the second is a recovery operation.
- **Recovery code**: generated by the app with ≥128 bits of entropy, never
  user-chosen, shown once at vault creation. The Argon2id pass over it is
  defence in depth — at that entropy the KDF is not load-bearing. It is a
  second wrapping of the same DEK, not an escrow; no one else holds a copy.
- **Associated data** binds each ciphertext to its object ID and format
  version, so the sync layer cannot be tricked into swapping objects or
  rolling an individual object back. Whole-vault rollback is a separate
  problem and is not solved by this (§3).
- **Versioning**: a client that meets a major format version it does not
  understand **refuses to open the vault and never writes to it** — no partial
  upgrades, no half-migrated vaults.
- **Hygiene**: `zeroize` on all key material, `secrecy` wrappers to prevent
  accidental logging/serialization, no key material in `Debug` output, and a
  compile-fail test asserting key types cannot be serialized at all.
- Crates: `argon2`, `chacha20poly1305`, `hkdf`, `zeroize`, `secrecy`,
  `getrandom`. No hand-rolled primitives, ever.

## 6. Storage

Two layers, with a deliberate split:

- **The vault** — the source of truth. Content-addressed encrypted blobs plus
  an encrypted manifest, on the local filesystem. Sync-friendly: an object is
  immutable once written, so syncing is "copy the objects the other side is
  missing", plus tombstones (below).
- **The index** — a SQLite database holding decrypted titles, dates, tags, and
  full-text search. It is **derived and disposable**: it can be rebuilt from
  the vault at any time, it is never synced, and it is never authoritative.
  This keeps search fast without inventing a second thing to encrypt correctly.

**Object names are keyed, not raw hashes.** An object stored under the hash of
its own plaintext is a confirmation oracle: anyone holding the vault directory
can hash a file they suspect you have and check whether it is there. Objects
are therefore named `HMAC(K_name, plaintext_hash)`, with `K_name` derived from
the DEK (§5). Deduplication still works inside a vault — identical input yields
an identical name — while the name means nothing to anyone without the DEK.

**The index is not written to disk at M1.** A plaintext FTS table over entry
bodies would leave the whole journal in cleartext beside a vault we went to
considerable trouble to encrypt, which would quietly negate the milestone's
own guarantee. So the index is built in memory on unlock and dropped on lock,
at the cost of rebuild time at unlock. If a full rebuild of a realistic vault
stops being fast enough to hide behind the unlock screen, the replacement is
an *encrypted* on-disk index (SQLCipher or an encrypted-page VFS) — never a
plaintext one.

**Deletion needs tombstones.** "Immutable objects, copy whatever the other
side is missing" on its own resurrects deleted entries at the next sync, so a
delete is a tombstone record in the manifest rather than the absence of an
object. Blobs left unreferenced — an image edited out of an entry — are
reclaimed by a garbage-collection pass, which is safe to run locally at M1 and
needs replica awareness once sync exists (§7).

Images are ordinary blobs, referenced from Markdown by object name
(`journal://blob/<name>`), which deduplicates identical images automatically
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
produces a conflict copy rather than silent data loss. Deletions propagate as
manifest tombstones (§6), which also sets the condition for collecting
unreferenced blobs — a blob is only safe to drop once every known replica has
seen the tombstone that orphaned it. CRDT-based merging is a possible later
refinement, not a milestone-1 commitment.

## 8. Roadmap

| Milestone | Scope |
|---|---|
| **M1** | Rust core + encrypted local vault + Markdown editing + images + plaintext export, desktop only. No sync, no mobile. |
| **M2** | Sync engine + Dropbox provider, conflict handling, desktop only. |
| **M3** | iOS build: Keychain-wrapped key, biometric unlock, mobile editor UX. |
| **M4** | Android build (Keystore), plus a second sync provider to prove the trait. |
| **M5** | Hardening pass: external crypto review, reproducible builds, signed releases. |

M1 is deliberately narrow: the crypto and storage layers are the parts that
are expensive to change later, so they get proven before sync and mobile
toolchains are added on top.

## 9. Open questions

- Padding policy for object sizes. Entry length and image size are visible
  today (§3); bucketed padding is cheap to implement but changes the on-disk
  format, so it wants deciding before M2 ships bytes to a provider.
- Garbage-collection safety once replicas exist: how a client establishes that
  every replica has seen a tombstone before dropping the blob it orphaned.
- Conflict-resolution model — deferred to the M2 spec.
- The exact Argon2id ceiling for the weakest supported device class, measured
  on real hardware rather than guessed, once a phone build exists.

Resolved since the first draft:

- *Does the derived index need encryption at rest?* — At M1 it is not written
  to disk at all (§6), which settles the desktop case and gives mobile the same
  default. An encrypted on-disk index is the fallback if rebuild cost demands
  one; a plaintext one is not on the table.
- *Plaintext export/import at M1?* — Yes (F7). For a zero-knowledge app with no
  server, "can I get my data out if this app breaks?" is the trust question,
  and the answer cannot be "trust us". Withholding export also hides a
  data-loss risk behind a leak risk, and the leak risk is answerable with an
  explicit confirmation at the point of export.

## 10. Revision notes

**2026-08-14** — revised after a critical review of the M1 plan. Every
substantive change below touches the on-disk format or the milestone's
security claim, which is why they were worth making here rather than after
implementation:

- Object names are keyed (`HMAC(K_name, …)`) instead of raw plaintext hashes,
  closing a confirmation-of-file oracle (§6).
- The DEK is unwrapped through a list of key slots, so M3's device key becomes
  a new row rather than a format migration (§5).
- The derived index is no longer written to disk in plaintext (§6) — it was the
  one element directly contradicting M1's "everything on disk is ciphertext".
- Deletion has a model: manifest tombstones plus blob GC (§6, §7). Immutable
  objects and entry deletion were previously incompatible.
- Corrected the claim that per-object keys make re-keying possible without
  re-encryption. That is true of a passphrase change, not of DEK rotation (§5).
- Argon2id calibration is bounded by the weakest device that must open the
  vault, not only by a minimum floor (§5) — the original framing would have
  produced desktop-created vaults that a phone cannot open.
- Shape metadata, swap/hibernation, the WebView heap, and whole-vault rollback
  are now stated in the non-defenses list instead of being left implicit (§3).
- Plaintext export/import moved into M1 as F7 (§2, §8, §9).
