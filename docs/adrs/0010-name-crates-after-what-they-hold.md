# 0010. Name the storage crates after what they hold

## Status

Accepted — 2026-09-14

Amends [0009](0009-blob-store-seam-and-local-first-sync.md): the layers it
split are unchanged, and so is the direction they depend in. Only the names
move.

## Decision

The word "vault" names the layer that holds a user's entries, not the layer
that holds a key:

| Was | Is |
| --- | --- |
| `journal-storage` / `Storage` | `journal-vault` / `Vault` |
| `journal-vault` / `Vault` | `journal-key` / `Keyring` |
| `journal-vault` / `VaultError` | `journal-key` / `KeyError` |
| `journal-storage` / `StorageError` | `journal-vault` / `VaultError` |
| `crates/vault/src/vault.rs` | `crates/key/src/keyring.rs` |
| `src-tauri`'s `VaultState.storage` | `VaultState.vault` |

Nothing else changes: no dependency edge is reversed, no method moves between
crates, no on-disk format is touched, and the Tauri command names
(`has_vault`/`unlock`/`lock`) and every user-facing `#[error]` string are
exactly as they were.

## Reasons

**The type called `Vault` was not a vault.** After 0009 it held one field, an
X25519 keypair, and its own doc comment opened by saying it owns no directory
and knows nothing about what an entry looks like. What it did was read and
write one sealed blob and implement `Cipher`. That is a keyring.

**The type that *is* a vault was called `Storage`.** `Storage` owned the
entries, the `{id}.json` naming, and — the decision 0009 deliberately moved
there — whether a store is a journal to unlock or an empty one to create,
including the `MissingKeyFile` refusal that keeps a lost key from orphaning a
journal. Unlocking, creating, refusing: vault verbs, on a type named after a
filing cabinet.

**The mismatch was already costing comment space.** `Storage::open` carried a
paragraph explaining why the vault decision was *not* in the vault crate, and
`Vault::create`/`Vault::unlock` each carried a "call this only through
`Storage::open`" warning. A type whose every constructor says "don't call me"
is a type whose name is on the wrong layer.

**It leaked into the app.** `src-tauri` named its state `VaultState`, but the
field actually locked and unlocked was `storage: Option<Storage>`, and
`"vault is locked"` meant `storage.is_none()`. Meanwhile
`VaultError::MissingKeyFile`'s user-facing copy — "this folder holds journal
entries but its key file is missing" — describes a condition the `Vault` type
could not detect; only `Storage` could.

**`Keyring` says more than `Cipher` would.** The type is not purely a cipher:
it also knows that its key lives sealed in a `.vault-key` blob and how to get
it in and out. Naming it for the algorithm (`AgeCipher`) would leak the choice
the `Cipher` trait exists to hide; naming it `Keyring` names the thing.

## Alternatives considered

- **Rename only the key half** (`journal-vault` → `journal-key`, leave
  `journal-storage`). Fixes the false name but leaves the real vault called
  `Storage`, so "vault" survives only in command names and user copy with no
  type behind it. Rejected: half the confusion is the app's state field and
  the `LOCKED_ERROR` string, and neither improves.
- **Leave it.** Rejected: the naming is load-bearing for the safety rule 0009
  established. The convention protecting every entry — that `Vault::open` is
  the only caller of the key constructors — is enforced by reading, and
  reading is what a wrong name breaks.
- **Make `Vault` the outermost *type* by moving entry code into the crypto
  crate.** This is 0009's rejected alternative and it stays rejected: it would
  make the key crate depend on `journal-entry`, serde, and the on-disk layout,
  when that is the crate that most benefits from staying small enough to audit
  in a sitting. The present change is the opposite trade — it gives the
  outermost crate the domain word *without* moving a line of code into the
  crypto one, which is why 0009's reason for rejecting it does not apply here.
  0009 proposed serving the domain language with command names alone; that
  turned out to be what left `VaultState.storage` behind.
- **`KeyStore`, `MasterKey`, `VaultKey` or `AgeCipher` for the lower type.**
  `KeyStore` collides with `BlobStore` in a codebase where "store" already
  means "where bytes live"; `MasterKey`/`VaultKey` read as bare key material
  and say nothing about the sealed blob; `AgeCipher` names the algorithm the
  `Cipher` trait deliberately hides.

## Consequences

Every layer's name now answers "what does this hold?" in one word: blobs, a
key, entries. `src-tauri`'s state reads `VaultState { store, vault }`, and
`"vault is locked"` refers to the field that is actually `None`.

`Keyring::create`/`Keyring::unlock` are still `pub` with `Vault::open` as
their only legitimate caller — 0009's positional-rather-than-compiler
protection, unchanged. The convention is easier to state now: the vault owns
the keyring, not the other way round.

`journal-vault` is the crate name a reader will reach for first, and it is
now the one that holds the entry semantics. Old references to
`journal-storage` in `docs/plans/` and in ADRs 0003, 0005, 0006 and 0009 are
left as written: they are records of decisions made under the old names, and
this ADR is the pointer that translates them.

No data migration, and no change to what is on disk: same blob names, same
`.vault-key` blob, same ciphertext. A vault created before this rename opens
after it.
