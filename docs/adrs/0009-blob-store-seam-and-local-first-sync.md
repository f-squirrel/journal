# 0009. Layer entry storage over a blob store, and sync local-first

## Status

Accepted — 2026-09-09

Amends [0007](0007-cached-x25519-vault-key.md): the sealed key file becomes a
blob in the store rather than a path at the vault root. The cached-key scheme
0007 decided is unchanged.

Amended by [0010](0010-name-crates-after-what-they-hold.md): the two upper
crates here are now named `journal-vault` (what this ADR calls
`journal-storage`) and `journal-key` (what it calls `journal-vault`), with
`Storage` renamed `Vault` and `Vault` renamed `Keyring`. The layering and its
dependency direction are unchanged.

## Decision

Split persistence into three layers, each owning exactly one fact, and add two
crates for the bottom one — the abstraction and its local implementation:

```text
journal-storage      entry semantics: id <-> blob name, serde, header sort, skip-corrupt
  |-- journal-vault       the key; encrypt/decrypt bytes (implements `Cipher`)
  `-- journal-store       `trait BlobStore`: named opaque blobs. No implementation.
        `-- journal-fs-store   one file per blob in a directory
```

- **`journal-store`** defines `BlobStore` — `get`/`put`/`delete`/`list`/
  `contains` over named byte blobs — and nothing else. It knows nothing about
  entries, keys, or JSON. It is the only layer that knows *where bytes live*,
  and the only one a future Dropbox/Drive backend has to implement.
- **`journal-fs-store`** is that local implementation, in its own crate rather
  than beside the trait. Every layer above depends on the trait crate alone;
  only `src-tauri`, which wires the app together, names a concrete store, and
  the crates in between reach for `journal-fs-store` as a *dev*-dependency for
  their tests. A cloud backend then arrives as a sibling crate instead of as a
  variant inside the local one, and cannot be reached by accident from code that
  should not care which store it has.
- **`journal-vault`** keeps the key and gains a `Cipher` trait
  (`encrypt`/`decrypt`) that `Vault` implements. It loses `dir()` and
  `holds_entry_files`: it no longer owns a directory and no longer knows
  what an entry file looks like. Its sealed key is a blob named
  `.vault-key` in the store it is given.
- **`journal-storage`** owns the directory-level decisions it was already
  making halfway: the `{id}.json` blob naming, and — new — the
  create-or-open decision, now `Storage::open(store, passphrase)`.
  `Vault::create`/`Vault::unlock` become public, and `Storage::open` is
  their only caller.

Encryption sits **above** the store, not inside it: `Storage` serializes,
asks the `Cipher` to encrypt, and hands the store ciphertext. A store never
sees plaintext, and a consumer that only moves blobs needs no key at all.

Sync, when it is built, is **local-first**. The local `FsStore` stays the
source of truth for every read and write; a separate engine replicates
encrypted blobs between it and a remote `BlobStore`. Reads and writes never
wait on a network. This ADR does not design the remote trait — see
Consequences for what it will need beyond the local one.

## Reasons

**The layout invariant was split across two crates.** `Storage` named
entries `{id}.json` and filtered on the extension; so did
`holds_entry_files` in `journal-vault`, the other half of 0007's
`MissingKeyFile` orphan guard. Renaming entries in `Storage` would have
silently disabled that guard, with no test able to see it, because the two
halves lived in different crates. Moving the decision to `Storage::open`
puts both facts — "is there a key blob" and "are there entry blobs" — in
the one place that legitimately knows both.

**Three copies of the directory path.** `Vault.dir`, `Storage` reading it
back out via `vault.dir()`, and a third in `src-tauri`'s `VaultState`,
which needed its own copy because `Storage` does not exist until unlock.
Under the split, `FsStore` owns the directory and nothing else does; the
Tauri state holds one `Arc<dyn BlobStore>` and hands it to `Storage::open`.

**The axis of variation for sync is *where bytes live*, not how they are
encrypted.** Dropbox and Google Drive change the bottom layer only. With
the seam at `BlobStore`, a backend is one trait impl that touches neither
the entry format nor the key handling — and with the trait in a crate of its
own, adding one does not touch the local store either.

**The key must travel with the entries.** 0007 put the sealed key at a
filesystem path. If it stays local while entries sync, the other devices
receive ciphertext they cannot open, and the passphrase is not enough to
recover — 0007's own Consequences section says the key file is the only
copy of the key. Making it a blob in the same store is what lets a second
device bootstrap from the passphrase alone.

**Local-first, because the entry list reads every blob.** `list_entries`
decrypts every entry to build its headers. Against a directory that is
fine. Against a remote store it is one network round trip per entry to draw
the first screen, and nothing works offline. Replicating in the background
keeps latency, retries, pagination, auth refresh, and conflicts out of the
path of every keystroke.

**`Cipher` as a trait also buys test speed.** `journal-storage`'s tests
each created a real vault, paying one scrypt (~0.4s) for coverage that has
nothing to do with encryption. They now run against a trivial in-test
cipher, with one test still using a real `Vault` to prove entries are
encrypted at rest.

## Alternatives considered

- **`Vault` as the outermost type, with `Storage` inside it** (the mirror
  of this decision). It reads well against the domain — `has_vault`,
  `unlock` and `lock` are vault-level verbs — and it would collapse
  `src-tauri`'s split `dir`/`Option<Storage>` state into one handle.
  Rejected: it makes the crypto crate depend on `journal-entry`, serde, and
  the on-disk layout, when that is the one crate that most benefits from
  staying small enough to audit in a sitting; and with sync it would put
  the key handling structurally *above* a network backend. The domain
  language is served instead by keeping the Tauri commands named
  `unlock`/`lock` over a `Storage` underneath.
- **A remote `BlobStore` as a drop-in replacement for `FsStore`, with no
  local mirror.** The tempting reading of this diagram. Rejected for the
  round-trip and offline reasons above; the abstraction is there so two
  stores can coexist and be replicated between, not so the remote one can
  be swapped into the entry path.
- **One async `BlobStore` trait covering local and remote now.** Rejected:
  local and remote have genuinely different contracts — etags, cursors,
  OAuth refresh, provider-side conflict copies — and unifying them today
  produces a lowest-common-denominator interface that leaks, while forcing
  `async` through every entry read for a backend that does not exist yet.
  The remote trait gets designed when the sync engine is.
- **An index blob, or SQLite, holding the headers** so the entry list is
  one read instead of N. A real option, and probably the eventual answer if
  entry counts grow — but it adds a second source of truth to keep
  consistent, and local-first sync removes the pressure that made it
  urgent. Deferred, not rejected.
- **Leaving the layering as it was.** Rejected: no place to put a second
  backend, and the duplicated layout knowledge above is a live defect
  regardless of sync.

## Consequences

`Storage::open` is now the only place the create-or-open decision can be
made, which is the property 0007 achieved by keeping `Vault::create` and
`Vault::unlock` private. That protection is now positional rather than
enforced by the compiler: the two constructors are `pub`, and the rule that
`Storage::open` is their only caller is a convention. In exchange the
decision sits with the code that can actually check both halves of it.

Blob names become path components — or, for a cloud backend, URL components —
so they are a validated newtype rather than strings: `journal_store::BlobName`
is constructed only through `TryFrom`, which rejects empty names, `.`/`..`, and
any name containing `/`, `\`, `:` or NUL. The trait's methods take `&BlobName`,
which puts the check in the type rather than in each backend — a store cannot
forget it, and `../escape` is unrepresentable rather than merely refused.
Nothing today builds a name that isn't a UUID or the key blob, but the trait
invites it.

`BlobName` is hand-written rather than generated with `nutype`. The validator
and its error type have to be hand-written either way to keep the message
specific about *which* character was rejected, so the crate would buy the
`TryFrom`/`Deref`/`Display` impls only — about 25 lines — in exchange for six
transitive dependencies (including a second major version of `syn`) in the one
crate every other layer depends on. The obvious second candidate, `Id` in
`journal-entry`, rules itself out: it carries `#[serde(transparent)]` and a
feature-gated `ts_rs::TS` derive for the generated TypeScript, and `nutype`
supports neither arbitrary third-party derives nor a `cfg_attr` on the
generated type.

The `.json` extension on entry blobs is now a misnomer: the bytes are `age`
ciphertext wrapping JSON. Kept anyway, because changing it would make
existing local entries unreachable for no benefit. ADR 0003's
`<id>.json` naming still describes what is on disk.

A remote store will need at least three things the local one does not, and
none are designed here: pagination over `list`, a version or etag per blob,
and an auth/refresh lifecycle. Two further gaps belong to the sync engine
rather than to any store:

- **Deletions need tombstones.** `delete_entry` removes the blob. With two
  replicas and no record of the deletion, a delete on the laptop is undone
  by the phone re-uploading the entry it still holds.
- **Conflict resolution cannot read `updated_at`.** That field lives inside
  the ciphertext, so a replicator that holds no key cannot use it. It must
  either resolve on store metadata (etag/mtime, last writer wins) or keep
  both copies and let the app decide after decrypting. Choosing between
  those is a prerequisite for writing the engine, not for this split.

Whatever ends up in the cloud, the provider learns the entry count, blob
sizes, modification times, and entry ids. Titles, dates, and bodies stay
inside the ciphertext. That is the accepted trade of encrypt-then-store,
recorded here so it is not rediscovered as a surprise.

No on-disk format change: same blob names, same ciphertext, same key file
contents in the same location. Nothing to migrate, and no local data is
dropped.
