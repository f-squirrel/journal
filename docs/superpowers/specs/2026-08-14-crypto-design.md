# Journal — Cryptographic Design

Date: 2026-08-14
Status: Ratified — normative for M1 implementation.
Last revised: 2026-08-14 (third revision) — review corrections: `K_name` is
now specified per backend, object naming hashes content rather than the
sealed plaintext, age's lack of sender authentication is stated and the
recipient is declared key material, and the scrypt read ceiling drops to
`log_n = 18`. See §10.
Parent design: [`2026-08-14-journaling-app-design.md`](2026-08-14-journaling-app-design.md)
(§3 threat model, §5 cryptography, §6 storage).

This document specifies the vault's cryptography at the level of detail
required to implement it twice, independently. That is not a figure of
speech: M1 Task 16 writes `tools/decrypt.py` from **this document alone**,
never from the Rust source.

Everything below is normative. "MUST", "MUST NOT", "SHOULD", and "MAY" carry
their RFC 2119 meanings. Byte offsets are zero-based; all multi-byte integers
are **little-endian**; `X[a..b]` is the half-open byte range.

## 1. Shape of this document

The vault is split into a **backend-independent core** and a **pluggable
sealing backend**:

```
        ┌─────────────────────────────────────────────┐
        │ backend-independent core            §3, §4  │
        │  vault layout · header · manifest           │
        │  object naming (HMAC) · recovery code        │
        │  the Binding contract                        │
        └───────────────────┬─────────────────────────┘
                            │  KeyStore + ObjectSealer  (§3)
              ┌─────────────┴─────────────┐
              ▼                           ▼
      ┌───────────────┐          ┌──────────────────┐
      │ age backend   │  M1      │ native backend   │  M2
      │ §5            │  SHIPS   │ §6               │  PLANNED
      │ scrypt + age  │          │ Argon2id + AEAD  │
      └───────────────┘          └──────────────────┘
```

**M1 ships the age backend** (§5). It is chosen to save implementation time:
it removes the envelope framing, the AEAD plumbing, and the key-slot record
format from M1's scope, replacing them with a reviewed, widely-implemented
container. The native backend (§6) remains fully specified because it is the
M2 target and because writing it down is what made the seam in §3 the right
shape.

### 1.1 What M1 gives up, stated plainly

Adopting age for M1 is a deliberate trade, not a free win:

- **No Argon2id.** age's passphrase recipient is scrypt-only, so M1 does not
  satisfy the parent design's requirement **F4** ("End-to-end encryption with
  Argon2 key derivation"). F4 is *deferred to M2*, when the native backend
  lands. scrypt is a respected memory-hard KDF and this is a modest
  quantitative step down, not a break — but it is a requirement not met, and
  the parent design's §2 now records that.
- **Binding is enforced by our code, not by the format.** age fixes
  `aad = ""`, so the object-identity binding that the native backend gets from
  AEAD associated data is instead a preamble inside the plaintext (§5.4). It
  fails *open* if a caller forgets to verify, where AAD fails closed. §3.2
  exists to make forgetting structurally impossible.
- **No sender authentication, so the recipient key is secret material.** age
  encrypts to a public key and authenticates nobody: anyone holding the
  vault's X25519 *recipient* can mint a well-formed object with a valid
  preamble under any `Binding`. The native backend has no such property — its
  DEK is symmetric, so forging requires the same secret that reads. At M1 the
  recipient is never written to disk or exported, which is what keeps the
  parent design's §3 malicious-provider defence intact; that is an
  **invariant this spec imposes** (§8), not a property of age. Treat the
  recipient as key material, not as a public key.
- **Interop is partial.** An age file from this vault is decryptable by the
  stock `age` CLI, but the plaintext begins with the 52-byte binding preamble
  (§5.4), so a user gets their Markdown with a binary prefix rather than a
  clean file. `tools/decrypt.py` strips it.

### 1.2 Not specified here

Whole-vault rollback (the header reserves the field, §4.2; the mechanism is
M2), device/biometric slots (M3), object size padding, DEK rotation, and
sync/conflict handling.

## 2. Primitives

Backend-independent:

| Role | Primitive | Notes |
|---|---|---|
| Object naming | HMAC-SHA-256 | truncated to 160 bits, §4.4 |
| Content hashing | SHA-256 | full 256-bit output |
| Naming-key derivation | HKDF-SHA-256 | Extract **and** Expand, §4.4 |
| Recovery-code encoding | Crockford Base32 | `base32` crate, §4.5 |
| Checksums (non-security) | SHA-256, truncated | §4.2 |
| Randomness | OS CSPRNG | `getrandom`, §2.1 |

Per backend: see §5.1 (age) and §6.1 (native).

Rust crates: `age`, `base32`, `hkdf`, `hmac`, `sha2`, `zeroize`, `secrecy`,
`getrandom`, `subtle`; plus `argon2` and `chacha20poly1305` when the native
backend lands. **No hand-rolled primitives, ever** — and that extends to the
encoding and the container: the `base32` crate's `Alphabet::Crockford` is
exactly the alphabet of §4.5 and already performs the case folding and
confusable mapping it requires, and the `age` crate implements the container
of §5.

### 2.1 Randomness

The vault identity key, the recovery code, the vault ID, and every salt and
nonce MUST come from the OS CSPRNG (`getrandom`, or `age`'s internal use of
it), obtained in a single call per value. Implementations MUST NOT seed a
userspace PRNG from it, MUST NOT reuse a buffer across two values, and MUST
treat a CSPRNG failure as fatal.

## 3. The seam

Two traits separate the core from the backend. They exist so that the M2
migration is a backend swap rather than a rewrite, and — more importantly —
so that the security properties the backend must provide are stated as a
*contract* rather than left implicit in whichever backend happens to be
installed.

These signatures live in `crates/journal-crypto/src/backend.rs`. They are
normative: a backend that cannot implement them as written is not a valid
backend for this vault.

```rust
/// Everything that identifies one object. Every backend MUST bind every
/// sealed object to this value, by whatever mechanism it has.
pub struct Binding {
    pub format_major: u16,
    pub format_minor: u16,
    pub vault_id: [u8; 16],
    pub kind: ObjectKind,
    pub object_id: [u8; 20],
}

/// Seals and opens individual vault objects.
pub trait ObjectSealer {
    /// Seal `plaintext` as the object described by `binding`.
    fn seal(&self, binding: &Binding, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError>;

    /// Open `sealed`, which MUST have been sealed under exactly `binding`.
    ///
    /// Implementations MUST verify the binding and MUST return
    /// `CryptoError::BindingMismatch` — never plaintext — when it does not
    /// match, whether the mechanism is AEAD associated data or an
    /// in-plaintext preamble.
    fn open(&self, binding: &Binding, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError>;
}

/// Holds the unlocked vault secret and manages the on-disk key slots.
pub trait KeyStore: Sized {
    type Sealer: ObjectSealer;

    /// Create a new vault's key material. Returns the recovery code, which
    /// the caller MUST show once and never persist.
    fn create(root: &Path, passphrase: &SecretString)
        -> Result<(Self, RecoveryCode), CryptoError>;

    /// Unlock an existing vault with a passphrase or a recovery code.
    fn unlock(root: &Path, secret: &UnlockSecret) -> Result<Self, CryptoError>;

    /// Rewrap the passphrase slot. MUST NOT touch any object, and MUST NOT
    /// invalidate the recovery slot. Takes no root: the vault it rewraps is
    /// the one it was opened on, so rewrapping into a *different* vault's
    /// slot directory is not expressible.
    fn change_passphrase(&self, new: &SecretString) -> Result<(), CryptoError>;

    fn sealer(&self) -> &Self::Sealer;

    /// The HMAC key for object naming (§4.4).
    fn name_key(&self) -> &NameKey;
}

pub enum UnlockSecret {
    Passphrase(SecretString),
    Recovery(RecoveryCode),
}
```

### 3.1 What the seam does and does not move

**Above the seam** — the same *construction* under every backend: the
directory layout (§4.1), the header (§4.2), object kinds and IDs (§4.3), the
object-naming algorithm (§4.4), the recovery code's entropy and encoding
(§4.5), the manifest, and tombstones.

**Below the seam** — backend-specific: how the vault secret is wrapped by a
user secret, how a sealed object is framed, and which KDF and AEAD are used.

**"Same construction" is not "same bytes", and object names are where that
bites.** §4.4's algorithm is backend-independent, but its key `K_name` is
derived from the vault secret, which lives below the seam — so the two
backends produce *different names for the same content*. The M2 migration
therefore re-seals and renames every object (§7). Nothing above the seam has
to be redesigned, which is the real benefit; but the migration is a rewrite
of the object directory, not a slot swap, and §7 says so plainly rather than
promising otherwise here.

What the seam does buy is stated in §3.2: the security contract a backend
must meet is written down once, as a type, instead of living implicitly in
whichever backend happens to be installed.

### 3.2 Why `Binding` is a parameter and not a convenience

`seal` and `open` both take `&Binding` and neither offers an overload without
it. That is the whole point: age has no associated data, so under the age
backend the binding check is something *our* code performs after decrypting
(§5.4), and a check that a caller can skip is a check that will eventually be
skipped. Making `Binding` a required argument of the only available entry
point means a caller cannot open an object without stating which object they
believe it is, and the backend cannot return plaintext without having checked
that belief.

This is the single most important line in this document, because it is what
keeps M1's "we used age to save time" from also meaning "we quietly dropped
the defence against a malicious storage provider."

## 4. Backend-independent core

### 4.1 Directory layout

```
<vault>/
├── header.jvh                      §4.2, plaintext, public
├── slots/                          backend-specific (§5.2, §6.2)
├── manifest.jobj                   kind = 1, object_id = 20 zero bytes
└── objects/
    └── <name[0..2]>/<name>.jobj    kind = 2 or 3, object_id = name tag
```

`.jobj` is the sealed-object extension under **either** backend; the header's
`backend_id` says how to open it. `<name>` is the 32-character lowercase
encoding of §4.4. The two-character shard directory keeps any one directory to
roughly 1/1024 of the objects.

The layout carries **no plaintext metadata**: no titles, no dates, no
extension that distinguishes an entry from an image. A directory listing
reveals the object count and each object's size — the shape metadata the
parent design's §3 already concedes.

### 4.2 Header — `<vault>/header.jvh` (44 bytes, fixed)

```
offset  size  field
0       8     magic            b"JRNLVLT\x00"
8       2     format_major     u16, = 1
10      2     format_minor     u16, = 0
12      1     backend_id       u8, 1 = age (§5), 2 = native (§6)
13      3     reserved         MUST be 0
16      16    vault_id         random, public, no security role
32      8     rollback_counter u64 — RESERVED, see below
40      4     header_checksum  first 4 bytes of SHA-256(header[0..40])
```

`header_checksum` is a **corruption detector and nothing more** — it is
unkeyed, so anyone can recompute it after tampering. It exists so a
bit-rotted header reports "this vault's header is corrupt" instead of "wrong
passphrase", which is the difference between a user restoring a backup and a
user believing they forgot their passphrase.

**Header authenticity comes from the objects, not from the header.** Under
both backends the header's security-relevant fields — `format_major`,
`format_minor`, `vault_id` — appear in every object's `Binding`. Tampering
with them therefore makes every object fail to open (§3.2). The failure is
detected at the first object read rather than at unlock, and it presents as a
corrupt vault rather than as a tamper alarm; that is the trade this format
accepts, and it is the same trade the native backend makes for a different
reason (§6.2).

`backend_id` is *not* covered by any binding. Tampering with it selects the
wrong backend, which fails to open the slots. Fail-closed, no further
protection needed.

**`rollback_counter` is reserved.** M1 writers MUST write 0. M1 readers MUST
ignore its value and MUST NOT reject a non-zero value — but **an M1 client
that reads a non-zero `rollback_counter` MUST open the vault read-only**. A
non-zero counter means some client is maintaining rollback protection this
client cannot maintain, and writing would silently break the invariant that
client depends on. The mechanism is M2's spec; whole-vault rollback remains a
stated non-defense at M1 (parent §3).

That read-only latch is **attacker-triggerable, and accepted as such**.
`rollback_counter` is in no `Binding`, and `header_checksum` is unkeyed, so
anyone holding the vault directory can set the counter, recompute four bytes,
and freeze every M1 client into read-only. This is a denial of service, not a
disclosure or a forgery — the same class as tampering with `backend_id`, and
the same trade this format accepts throughout: an attacker who controls the
bytes can always stop the app working, and no unkeyed header field can
prevent that. What it MUST NOT do is fail silently: a client that latches
read-only for this reason MUST tell the user the vault appears to have been
written by a newer client, rather than presenting a vault whose saves quietly
do nothing.

### 4.3 Object kinds and IDs

| `object_kind` | Object | `object_id` |
|---|---|---|
| 1 | the manifest | 20 zero bytes |
| 2 | an entry | its 20-byte name tag (§4.4) |
| 3 | an attachment blob | its 20-byte name tag (§4.4) |
| 4–255 | reserved | — |

The manifest is mutable and lives at a fixed path, so it is the one object
that is not content-addressed. Its all-zero ID cannot collide with a name tag
because the kind is bound alongside the ID everywhere it matters.

### 4.4 Object naming

Objects are named by a **keyed** tag, never by a bare unkeyed hash of their
content:

```
h        = SHA-256(content)                                    # 32 bytes
tag      = HMAC-SHA-256(K_name, b"JRNLNAM1" || kind_u8 || h)   # 41-byte input
name_tag = tag[0..20]                                          # 160 bits
name     = crockford_base32(name_tag), lowercased              # 32 chars
```

`content` is the object's own bytes — the entry, the blob, the serialized
manifest. It is emphatically **not** the age backend's `plaintext` of §5.5,
which is `preamble || content`: the preamble carries `object_id`, which *is*
`name_tag`, so hashing it would be circular. The hash covers the content
only, under either backend.

**`K_name` is derived per backend**, because the two backends' vault secrets
are different kinds of thing. This is the one place a backend reaches above
the seam, and the two constructions are not interchangeable — deriving the
wrong one yields a vault whose every object name is wrong:

| Backend | `K_name` |
|---|---|
| age (§5.3) | `HKDF-SHA-256(ikm = identity_string_bytes, salt = b"", info = b"JRNLKDF1" \|\| 0x02, L = 32)` — Extract **and** Expand |
| native (§6.3) | `HKDF-Expand-SHA-256(prk = DEK, info = <30-byte info, purpose = 2>, L = 32)` — Expand only |

The age backend uses full HKDF because its input is a Bech32 *encoding* of a
key rather than a uniformly random key, and Extract is what makes that safe.
The native backend uses Expand-only because the DEK is already a uniformly
random 256-bit key (RFC 5869 §3.3), and it shares the single 30-byte `info`
construction of §6.3 with every other key below the DEK rather than
introducing a second, shorter one. §9's vector 2 pins **both**.

`name_tag` is the object's `object_id` in `Binding`, so the name and the
binding agree by construction.

**Why keyed.** An object stored under the hash of its own plaintext is a
confirmation oracle: anyone holding the vault directory can hash a file they
suspect the user has — a specific photograph, a leaked document — and check
whether that name is present. Keying with `K_name` leaves deduplication intact
*inside* a vault (identical input yields an identical name, so a duplicate
image is stored once) while making the name meaningless to anyone without the
vault secret. Task 9's regression test is written to read as what it is:
**a blob's on-disk name equals no hash of its plaintext.**

`kind_u8` inside the HMAC keeps entry names and blob names in separate
namespaces. 160 bits of tag gives a birthday bound around 2^80 objects;
truncation is safe because the tag is keyed, so an attacker cannot search for
a colliding plaintext without `K_name`.

Content addressing makes objects immutable: editing an entry writes a new
object under a new name and repoints the manifest. Deletion is a manifest
tombstone plus a later garbage-collection pass over unreferenced blobs
(parent §6), not the absence of an object.

### 4.5 Recovery code

- **160 bits** — 20 bytes from the CSPRNG (§2.1), generated by the app, never
  user-chosen, never derived from the passphrase, shown exactly once at vault
  creation.
- Encoded with **Crockford Base32**: 160 / 5 = **32 characters exactly**, no
  padding and no leftover bits. An encoding with partial trailing bits admits
  several distinct strings that decode to the same value — an ambiguity a
  recovery path should not have.
- Displayed as **8 groups of 4**, uppercase, hyphen-separated:

  ```
  K7M2-9XQ4-BVN3-8HTP-Z5RW-6JYC-D2GK-4NFB
  ```

Alphabet: `0123456789ABCDEFGHJKMNPQRSTVWXYZ`. `I`, `L`, `O`, and `U` are
absent — which is what satisfies the "no `0`/`O`, no `1`/`l`" requirement: a
code never contains both members of a confusable pair, and a reader who
transcribes `O` for `0` or `l` for `1` produces a string the decoder maps back
to the right value. `U` is excluded so generated codes cannot spell
obscenities.

**Parsing.** Strip hyphens and whitespace; reject unless exactly 32
characters; decode with `base32::decode(Alphabet::Crockford, …)`, which folds
case and maps the confusables (`O`/`o` → `0`, `I`/`i`/`L`/`l` → `1`) as part
of decoding; reject if it returns `None` or anything but 20 bytes. Reject
cleanly — a malformed code is a typo, never a panic.

The **decoded 20 bytes** are what the backend consumes, not the display
string, so formatting, case, and separators cannot affect the derived key.
Steps here are deliberately not a hand-written normalizer: the behaviour to
match is "Crockford Base32", a citable standard, and an independent
implementation should reach for its own library.

There is **no check character**. A typo costs one KDF run and reports the same
generic failure as a wrong passphrase. A check symbol would widen the alphabet
and add another thing for Task 16's decryptor to reproduce, to save a second
on a path most users walk once or never.

At 160 bits the KDF pass over the recovery code is **defence in depth, not the
security boundary**. It is a **second wrapping of the same vault secret, not
an escrow**: no one else holds a copy, and a vault whose passphrase and
recovery code are both lost cannot be recovered.

## 5. M1 backend — `age` (`backend_id = 1`)

### 5.1 Primitives

Supplied entirely by the `age` crate (0.12.x, MIT/Apache-2.0), implementing
the [age v1 specification](https://github.com/C2SP/C2SP/blob/main/age.md):
scrypt for passphrase wrapping, X25519 + HKDF-SHA-256 + ChaCha20-Poly1305 for
recipient stanzas, and ChaCha20-Poly1305 in STREAM over 64 KiB chunks for
payloads. This project adds no cryptography of its own to that stack; it adds
the binding preamble of §5.4 and nothing else.

### 5.2 Key slots as separate files

age's specification requires that "an scrypt stanza, if present, MUST be the
only stanza in the header", so a single age file cannot hold both a passphrase
slot and a recovery slot. The slot *list* is therefore a **directory**:

```
<vault>/slots/
├── passphrase.age      age file, scrypt recipient = passphrase
└── recovery.age        age file, scrypt recipient = recovery code
```

Each file's plaintext is the **same** vault identity string (§5.3). Adding an
unlock method is a new file; M3's device slot is `device.age`. Unlock tries
only the file matching the attempt type — a passphrase MUST NOT be run against
`recovery.age`.

**Work factors MUST be pinned, not calibrated.** `age` defaults to choosing an
scrypt work factor targeting ~1 second *on the encrypting device*, and on
decryption it **rejects** — `DecryptError::ExcessiveWork` — any file whose
work factor exceeds the decrypting device's own target by more than 4. A
desktop-created vault would therefore be not merely slow but *refused* on a
phone. Implementations MUST therefore:

```
on write:  Recipient::set_work_factor(17)     # N = 2^17, r = 8 → 128 MiB
on read:   Identity::set_max_work_factor(18)  # accept up to 256 MiB
```

`log_n = 17` is the M1 pinned value: roughly 128 MiB of scrypt memory, which
stays inside the weakest supported device class (§6.4) while costing about a
second on desktop.

`max_work_factor = 18` caps a read at **256 MiB**, matching the ceiling §6.4
sets for the same device class. The bound matters because `log_n` is read
*from the file* and is therefore attacker-chosen: a corrupt or hostile slot
file gets to name the allocation, and `age`'s own documentation warns that a
loose cap here is a denial-of-service vector. One doubling of headroom is
enough for a future client that raises the pinned value to 18; a client that
wants to go beyond that is making a format change and can raise this bound in
the same revision.

This pair is the age backend's expression of the floor/ceiling contract that
§6.4 states for the native backend: **parameters are device-independent and
written down, and decryption accepts a documented range** — the same 256 MiB
range, so a device that can open a native vault can open an age one.

**Slot writes MUST be crash-atomic.** A half-written `passphrase.age` is a
vault the passphrase no longer opens, with the recovery code as the only way
back in — so every slot write goes to a temporary file in the same directory,
is fsynced, is renamed over its target, and the directory is fsynced after
the rename. Vault creation writes the slot files and the manifest first and
`header.jvh` **last**, so a vault interrupted during creation is a vault that
does not yet exist rather than one that half-exists. This is the same
commit-last ordering the migration uses in §7, for the same reason.

A passphrase change rewrites `passphrase.age` only. `recovery.age` is
untouched and the recovery code still works; no object is read or rewritten.

### 5.3 The vault secret

The vault secret is an **age X25519 identity**, generated by
`age::x25519::Identity::generate()` at vault creation.

```
vault identity  ── to_public() ──►  recipient for every object (§5.5)
                └─ to_string()  ──►  "AGE-SECRET-KEY-1…", the plaintext
                                     sealed inside each slot file (§5.2)
```

`identity_string_bytes` — the ikm of §4.4's age-backend `K_name` row — is the
**ASCII bytes of the uppercase Bech32 string** returned by
`Identity::to_string()`. Note that this returns a `SecretString`, and that it
uppercases: deriving `K_name` from a lowercased Bech32 string yields a
different key and therefore a different name for every object in the vault.
That string is an encoding rather than a uniform key, which is why the age row
of §4.4 uses full HKDF (Extract and Expand) where the native row uses
Expand-only.

Making the vault secret an age identity — rather than deriving one from a
separate symmetric key — is what keeps this backend free of bespoke
construction. Every file in the vault is an ordinary age file with an ordinary
recipient, and the escape hatch is real: given the identity string, `age -d -i
key.txt` opens any object.

The identity MUST be held in a `zeroize`-on-drop wrapper, MUST NOT implement
`Serialize` or `Debug`, and MUST NOT cross the Tauri IPC boundary.

**Per-object key isolation is preserved.** age generates a fresh random
128-bit file key per file, wrapped to the recipient, so compromise of one
object's file key reveals that object and nothing else — the same property the
native backend gets from per-object HKDF, by a different route.

### 5.4 The binding preamble

age fixes `aad = ""`, so `Binding` cannot be carried as associated data. Every
object's plaintext is therefore **prefixed with a 52-byte preamble**, and the
sealed file is an ordinary age file over `preamble || content`:

```
offset  size  field
0       8     b"JRNLBND1"
8       2     format_major   u16
10      2     format_minor   u16
12      16    vault_id
28      1     object_kind
29      3     reserved (0)
32      20    object_id
```

`ObjectSealer::seal` builds this from the `Binding` and prepends it.
`ObjectSealer::open` MUST, after age decryption and **before returning any
plaintext**:

1. Reject if the decrypted output is shorter than 52 bytes.
2. Rebuild the expected preamble from the caller's `Binding` and compare it
   against `plaintext[0..52]` with a constant-time comparison.
3. On mismatch, return `CryptoError::BindingMismatch` and zeroize the
   decrypted buffer. **MUST NOT** return the content.
4. On match, return `plaintext[52..]`.

This recovers, inside the plaintext, what §6.5's associated data provides
against a party who cannot encrypt: an object cannot be substituted for
another, rolled back to an older version of itself, or transplanted between
vaults, and a downgrade of the format version invalidates every object. The
difference is where the check lives — which is why §3.2 makes it unskippable.

**Where it stops short of §6.5.** Associated data is bound under a key only
the DEK holder has, so under the native backend a preamble cannot be
*produced* without the ability to read. Under age it can: the preamble is
plaintext sealed to a public key, so anyone holding the recipient can
construct a valid one for any `Binding` (§1.1). The preamble is therefore a
defence against a storage provider that can *rearrange* ciphertext, not
against one that has somehow learned the recipient. §8's rule that the
recipient never leaves the process is what closes that gap at M1; §6.5 closes
it by construction at M2.

Note what this costs the interop story: a stock `age -d` yields the content
with 52 bytes of binary in front of it. `tools/decrypt.py` strips the preamble
after verifying it.

### 5.5 Sealing an object

```
seal(binding, content):
    plaintext = preamble(binding) || content
    age file encrypted to  identity.to_public()      # one x25519 stanza
open(binding, sealed):
    plaintext = age decrypt with identity
    verify preamble per §5.4, return the remainder
```

Readers MUST reject a sealed file larger than 64 MiB **before allocating**, so
a corrupt or hostile file size cannot become an enormous allocation. Objects
are sealed and opened whole, not streamed: images are decrypted into memory
and then streamed to the WebView by Task 12's protocol handler.

### 5.6 Version rules

- **Unknown `format_major`** — refuse to open, and never write a byte.
- **`format_minor` higher than understood** — MAY open **read-only**.
- **Unknown `backend_id`** — refuse to open. A client that does not implement
  a backend MUST NOT guess.
- **An age file that fails to parse or decrypt** — reject that object. Never
  fall back to another key or backend.

## 6. M2 backend — native (`backend_id = 2`, planned)

**Not implemented at M1 — scheduled as M2 work**, itemised in the
"Deferred to M2" section of
[the M1 plan](../plans/2026-08-14-m1-encrypted-local-vault.md).

This section is specified to the byte rather than sketched, deliberately.
Adopting age for M1 postponed this work; it did not discard it, and the
difference is the whole point — M2 builds from a ratified format instead of
re-deriving one under sync deadlines. §3's seam was designed against this
backend, and Task 16's decryptor will eventually need both. This section is
where Argon2id, and therefore requirement F4, comes back.

### 6.1 Primitives

Argon2id (version 0x13) for slot KEKs; XChaCha20-Poly1305 (256-bit key,
192-bit nonce, 128-bit tag) for objects and for slot wrapping;
HKDF-Expand-SHA-256 for key derivation from the DEK. Every nonce is 192 random
bits drawn fresh per encryption — no counter, no derivation, no reuse. At 2^48
encryptions the collision probability is about 2^-97, which is why this format
needs no nonce-management state anywhere.

### 6.2 Key slots

The header grows a list of 1–8 slot records, LUKS-style, each independently
wrapping the same DEK under its own KEK, salt, parameters, and nonce:

```
offset  size  field           notes
0       1     slot_type       1 = passphrase, 2 = recovery, 3 = device (M3)
1       1     kdf_id          1 = Argon2id v0x13
2       1     lanes           Argon2id p, 1..=8
3       1     reserved        MUST be 0
4       4     mem_kib         u32, Argon2id m in KiB
8       4     iterations      u32, Argon2id t
12      16    slot_id         random, unique within the header
28      16    salt            random, 128 bits, fresh on every wrap
44      24    wrap_nonce      random, 192 bits, fresh on every wrap
68      48    wrapped_dek     32 ciphertext || 16 tag
```

Slots bind to `slot_id`, **not** to their position, so slots may be reordered
or removed without invalidating the others — binding to an index would mean
deleting slot 0 silently broke slot 1. `slot_id` MUST NOT change once
assigned; `salt` and `wrap_nonce` MUST be regenerated on every wrap, including
a passphrase change.

**Slot types.**

| Value | Type | Milestone |
|---|---|---|
| 1 | passphrase | with this backend |
| 2 | recovery code | with this backend |
| 3 | device key (Keychain / Keystore) | M3 |
| 4–255 | reserved | — |

A header MUST contain at least one slot of type 1. A reader that meets an
unknown slot type MUST ignore that slot — it cannot unwrap it — and MUST NOT
treat the header as invalid; that is what makes M3's slot additive. A reader
MUST NOT write to a header containing slot types it does not understand.

**Wrap and unwrap.**

```
KEK         = Argon2id(pwd, salt = slot.salt, m, t, p, out_len = 32)
wrapped_dek = XChaCha20Poly1305_Seal(key = KEK, nonce = slot.wrap_nonce,
                                     plaintext = DEK, aad = SlotAD)
```

`SlotAD` is **exactly 72 bytes**, all fields fixed width:

```
offset  size  field
0       8     b"JRNLSLT1"
8       2     format_major       u16, from the header
10      2     format_minor       u16, from the header
12      16    vault_id           from the header
28      1     slot_type
29      1     kdf_id
30      1     lanes
31      1     reserved (0)
32      4     mem_kib
36      4     iterations
40      16    slot_id
56      16    salt
```

This is what gives the header its integrity, and it is why there is no header
MAC: a header MAC would need a key the reader does not have until *after* it
unwraps a slot. Instead every field an attacker could usefully tamper with is
in the AAD of the wrap it governs. Lowering `mem_kib` to weaken the KDF,
swapping a salt, or editing the format version changes `SlotAD`, so the
Poly1305 tag fails and the vault does not open. The attacker converts a
downgrade into a denial of service, which is the trade this format accepts.
`vault_id` in `SlotAD` additionally prevents transplanting a slot record from
one vault's header into another's.

**Partitioning-oracle mitigation.** Before attempting any unwrap the reader
MUST confirm `wrapped_dek` is exactly 48 bytes — which the fixed-width record
already guarantees structurally, so this is a check an implementation must not
*lose* rather than one it must add. A password-based AEAD whose ciphertext
length is attacker-controlled admits a crafted multi-key ciphertext that
decrypts under many candidate passphrases, turning each online attempt into a
test of a whole set. age applies the same rule to its scrypt stanza.

**Unlock procedure.** Given a candidate secret and its type (`1` for a
passphrase attempt, `2` for a recovery-code attempt):

1. Parse and validate the header. Reject before any KDF work if malformed.
2. Select the slots whose `slot_type` matches the attempt type, in header
   order. Slots of other types MUST NOT be tried — a passphrase must never be
   run against the recovery slot.
3. For each selected slot: derive the KEK, attempt the unwrap. On success the
   plaintext is the DEK; zeroize the KEK and stop.
4. If every selected slot fails, zeroize all intermediates and return a single
   generic failure.

Rules that are part of the format, not implementation taste:

- The error MUST NOT reveal which slot failed, how many slots exist, or how
  far the attempt got. One message: the vault did not unlock.
- The implementation MUST NOT short-circuit on a cheap pre-check of the
  secret. There is no such check — the Argon2id run *is* the check.
- On failure, nothing is written to disk. Not a failure counter, not a
  timestamp. There is no rate limiting at rest; the KDF cost is the rate
  limit, and a write-on-failure would leak attempt metadata to anyone holding
  the directory.

**Passphrase change.** Unwrap the DEK through the existing passphrase slot,
then rewrap **that same DEK** into the same `slot_id` with a fresh salt, fresh
nonce, and freshly calibrated parameters, writing the header atomically. No
object is read, re-encrypted, or rewritten; the recovery slot is untouched and
the recovery code still works. After a passphrase change every object file
MUST be byte-identical.

### 6.3 Key hierarchy

```
passphrase ──── Argon2id(salt_p, m/t/p) ──► KEK_pass ──┐
recovery code ─ Argon2id(salt_r, m/t/p) ──► KEK_rec  ──┤ each slot wraps its
device key ──── OS keystore (M3+) ────────► KEK_dev  ──┘ own copy of one DEK
                                                        │
                                                 DEK (256 bits, random)
                                                        │
     HKDF-Expand(DEK, purpose=1, kind, object_id) ──► K_obj  (per object)
     HKDF-Expand(DEK, purpose=2)                  ──► K_name (§4.4)
     HKDF-Expand(DEK, purpose=3)                  ──► reserved (M2 rollback)
```

All keys below the DEK come from HKDF-Expand-SHA-256 with `L = 32` and an
`info` that is **always exactly 30 bytes** — fixed width in every field, so no
two distinct derivations can produce the same `info`:

```
offset  size  field
0       8     b"JRNLKDF1"
8       1     purpose        u8
9       1     object_kind    u8   (0 when purpose != 1)
10      20    object_id           (20 zero bytes when purpose != 1)
```

| `purpose` | Key | `object_kind` / `object_id` |
|---|---|---|
| 1 | `K_obj` — the AEAD key for one object | the object's kind and ID |
| 2 | `K_name` — the HMAC key for object naming (§4.4) | 0 / zeros |
| 3 | reserved for the M2 rollback-counter MAC key | 0 / zeros |
| 4–255 | reserved; MUST NOT be used | — |

HKDF is Expand-only here because the DEK is already a uniformly random
256-bit key (RFC 5869 §3.3), unlike the age backend's input in §4.4.

The `purpose = 2` row is this backend's `K_name`, and it is the **whole**
definition of `K_name` under the native backend — §4.4 specifies the tag
construction that consumes it, and its per-backend table points back here.
The native backend does **not** use the age backend's shorter
`b"JRNLKDF1" || 0x02` info.

Consequences worth stating: compromise of one `K_obj` reveals one object and
is not invertible back to the DEK; **changing the passphrase rewraps one slot
and re-encrypts nothing**; **rotating the DEK re-encrypts the entire vault**,
because every `K_obj` and every object name changes. The first is routine, the
second is a recovery operation.

### 6.4 Argon2id parameters

```
FLOOR:   m = 65536 KiB (64 MiB),  t = 3,  p = 4   # RFC 9106 §4, 2nd option
CEILING: m = 262144 KiB (256 MiB), t = 10, p = 4
```

The binding constraint is a **ceiling**, not a floor: a vault must stay
openable on the weakest device its owner will ever unlock it on, or a
desktop-calibrated vault becomes unopenable on the phone the user reaches for
at the moment they need it. **Weakest supported device class** (provisional): a
64-bit ARM phone, 2019 or later, ≥3 GiB RAM, where 256 MiB plus the app's
working set stays clear of the low-memory killer; unlock at CEILING within 3
seconds. Parent §9 lists measuring this on real hardware as an open question.

The ceiling MAY be raised for new vaults at any time, and MAY be lowered
**only** for new vaults — lowering it cannot retroactively fix a vault already
calibrated above it. Both bounds are enforced on read as well as write.

`p` is a **format parameter, not a thread count**: it changes the function's
output and must be reproduced exactly, though an implementation MAY run those
lanes on fewer threads.

**Calibration** runs at vault creation and at every passphrase change, on the
creating device, targeting `TARGET_MS = 1000`:

```
1.  p := 4;  m := FLOOR.m;  t := FLOOR.t
2.  d0 := median of 3 runs of Argon2id(FLOOR.m, FLOOR.t, p) over a
         throwaway password and salt, measured as wall clock
    #  cost is near-linear in m and t, so predict(m,t) = d0 * (m/FLOOR.m) * (t/FLOOR.t)
3.  if d0 >= TARGET_MS: return FLOOR          # weak device; never go below it
4.  while 2*m <= CEILING.m and predict(2*m, t) <= TARGET_MS:  m := 2*m
5.  while t+1 <= CEILING.t and predict(m, t+1) <= TARGET_MS:  t := t+1
6.  measured := one run of Argon2id(m, t, p)
7.  if measured > 1.5 * TARGET_MS: step back one increment and repeat
       from 6, at most 3 times, never below FLOOR
8.  return (m, t, p)
```

**Memory before iterations, deliberately**: resistance to GPU and ASIC attack
scales with the memory an attacker must provision per guess, while iterations
raise the cost linearly for attacker and defender alike. Calibration MUST run
on a throwaway password, never the user's real passphrase.

### 6.5 Object envelope

```
offset  size  field
0       4     magic         b"JENV"
4       1     env_version   u8, = 1
5       1     alg_id        u8, 1 = XChaCha20-Poly1305
6       2     reserved      u16, MUST be 0
8       24    nonce         random, 192 bits
32      n     ciphertext
32+n    16    tag
```

File length is `48 + n`; shorter than 48 is truncated and MUST be rejected.
There is no length field — that would be a second source of truth the AEAD
would then have to be trusted to reconcile.

The envelope is deliberately **not self-describing**: it does not record which
object it is, what kind it is, or which vault it belongs to. That comes from
the reader's context and enters the associated data, so an object moved to
another name or copied into another vault fails to open rather than opening as
something it is not.

`ObjectAD` is **exactly 60 bytes**:

```
offset  size  field
0       8     b"JRNLOBJ1"
8       2     format_major       u16, from the header
10      2     format_minor       u16, from the header
12      16    vault_id           from the header
28      1     object_kind        §4.3
29      3     reserved (0)
32      20    object_id          §4.3
52      8     envelope[0..8]     magic, env_version, alg_id, reserved
```

Sealing and opening:

```
K_obj      = HKDF-Expand(DEK, info = §6.3 with purpose=1, kind, object_id, L=32)
ciphertext = XChaCha20Poly1305_Seal(K_obj, nonce, plaintext, aad = ObjectAD)
plaintext  = XChaCha20Poly1305_Open(K_obj, nonce, ciphertext||tag, aad = ObjectAD)
```

What each field buys, since an independent implementation has to reproduce all
of them and a reviewer should be able to check the reasoning:

- `object_kind` + `object_id` — an object cannot be substituted for another,
  nor rolled back to an older version of itself, because an older version of
  an entry has different content and therefore a different name tag and ID.
  Its limit is equally explicit: it does not stop rollback of the *whole
  vault*.
- `format_major` / `format_minor` — a downgrade to an older format's parsing
  rules invalidates every tag.
- `vault_id` — objects cannot be transplanted between vaults.
- `envelope[0..8]` — `alg_id` and `env_version` are authenticated, so an
  attacker cannot claim a weaker algorithm for an existing ciphertext.

`K_obj` is thereby bound to the object ID twice over — once in the derivation,
once in the associated data. That redundancy is intentional: it costs nothing
and means an implementation that gets the derivation right but the AAD wrong
(or vice versa) still fails closed.

`ObjectAD` carries exactly the fields of the `Binding` of §3, which is what
makes the two backends interchangeable behind one contract: the native backend
satisfies `Binding` as associated data, the age backend as an in-plaintext
preamble (§5.4).

**Version rules for this backend** extend §5.6: an unknown `env_version` or
`alg_id` means the object is rejected outright — never decrypted with a
default — and an unknown `slot_type` is ignored rather than fatal (§6.2).

## 7. Migrating age → native

The migration **re-seals and renames every object**. It is not the slot-only
migration an earlier draft of §3.1 implied, and the reason is worth stating
before someone plans it as a one-liner: the two backends have different vault
secrets (an X25519 identity vs. a 256-bit DEK), so `K_name` changes, so every
object name changes — and the object bodies must be re-sealed under the new
backend anyway.

What the seam saves is *design*, not I/O: nothing above it (§4) is
redesigned, the manifest schema is unchanged, and object plaintext is
untouched. What it costs is a full pass over the object directory.

The migration therefore:

1. Unlocks under the age backend, obtaining the identity and `K_name_age`.
2. Generates a DEK, calibrates Argon2id, and writes native slot records.
3. Re-seals every object under the native backend and **renames it** to its
   new `K_name_native` tag, rewriting the manifest's references.
4. Rewrites `header.jvh` with `backend_id = 2`, last, atomically.

Object *plaintext* never changes, and the migration is resumable because step
3 is per-object and the header flip in step 4 is what commits it. A vault
interrupted mid-migration still opens under the age backend.

An alternative that avoids renaming — carrying `K_name` across the migration
by deriving the native `K_name` from the age identity — was rejected: it would
leave an M2 vault whose object names depend on a key the M2 backend otherwise
never uses, permanently coupling the two backends.

## 8. Operational rules

**Zeroization.** The vault identity or DEK, every derived key, the decoded
recovery code, and the normalized passphrase bytes MUST be held in
`zeroize`-on-drop wrappers and dropped as early as possible. Lock zeroizes
everything and drops the in-memory index. Rust cannot promise the compiler
kept no copy, and the OS may have paged one to swap; that limit is a stated
non-defense (parent §3), and it is why locking also tears down the WebView
rather than pretending to scrub its heap.

**No `Serialize`, no `Debug`** on key material types. Task 5 asserts this with
a `trybuild` compile-fail test, so the guarantee is enforced by the build
rather than by review.

**No secrets in logs.** No key material, entry body, title, passphrase, or
recovery code reaches `tracing` at any level. Task 16 audits this with a
canary.

**No secrets across IPC.** No Tauri command returns key material, slot bytes,
or vault file paths. The passphrase enters Rust and never comes back.

**The age recipient is key material.** Under the age backend the X25519
recipient (`identity.to_public()`) MUST be treated exactly like the identity:
never written to disk, never logged, never returned across IPC, never
exported, and never published by a future sync or sharing feature. age
provides no sender authentication, so possession of the recipient is
sufficient to forge any object under any `Binding` (§1.1, §5.4). It is a
public key by type only; in this vault it is a secret, and it is the reason
M1's binding preamble holds against the parent design's §3 adversary. This
rule falls away with the native backend, whose DEK is symmetric — which is
one more reason M2 is worth doing.

**Passphrase encoding.** Normalize to Unicode **NFC**, encode as UTF-8. No
trimming, no case folding, no whitespace collapsing — trimming would silently
discard user-chosen entropy and make two different passphrases unlock the same
vault.

**Comparisons.** Secret-dependent equality goes through `subtle`. This
explicitly includes the §5.4 preamble comparison.

**Failure is closed.** Every parse rule rejects rather than repairs. There is
no best-effort open, no field the reader guesses at, and no path that writes to
a vault it could not fully validate.

## 9. Conformance

Task 5 MUST emit golden vectors under `crates/journal-crypto/tests/vectors/`,
checked into the repository, covering at minimum:

1. **Recovery code** — a fixed 20-byte value, its canonical display form, and
   three sloppy transcriptions (lowercase, no hyphens, `O`/`l` substituted)
   that MUST all decode to the same bytes.
2. **Object naming, under both backends** — a fixed content blob and a fixed
   vault secret of each kind (an age identity string and a 32-byte DEK), with
   `K_name`, the HMAC input, the 20-byte `name_tag`, and the 32-character
   name, as hex. Both rows are required even though the native backend is M2
   work: §4.4's per-backend table is the single likeliest thing in this
   document to be implemented as if it were one construction, and the two
   differ in *both* the HKDF mode and the `info`. The vector is what catches
   that; the native row can be computed today from §6.3 without the backend
   existing.
3. **Binding preamble** — a fixed `Binding` and its 52 bytes, as hex.
4. **A complete age-backend vault** — `header.jvh`, both slot files, the
   manifest, and one entry object, openable end to end from a known
   passphrase and a known recovery code. Its slot files MUST be written at
   the pinned `log_n = 17`, not at a cheaper factor: a vector encrypted at a
   work factor the app never uses would not exercise the pinning rule this
   section exists to protect. That costs the test roughly a second of CPU and
   128 MiB per open, twice, which is the price of the vector being real.
5. When the native backend lands: RFC 9106 Argon2id vectors, a slot wrap with
   its 72-byte associated data, and an envelope with its 60-byte associated
   data, all as hex.

The binding and associated-data structures are recorded as hex on purpose.
They are the part of this format most likely to be implemented subtly
differently by two people reading the same prose, and a mismatch produces an
authentication failure whose cause is invisible without the intermediate
value.

Task 16's `tools/decrypt.py` MUST be written from this document without
reading the Rust implementation, and MUST open vector 4 and a vault produced by
the real app. That is the acceptance test for this specification, not just for
the code.

## 10. Revision notes

**2026-08-14 (third revision)** — corrections from a critical review of the
second revision. Nothing here changes the design's intent; all of it changes
what the document actually *says*, and four of the items were defects an
independent implementer could not have resolved:

- **`K_name` was specified twice, incompatibly.** §4.4 gave a full-HKDF
  derivation over a 9-byte `info` and called it backend-independent, while
  §6.3's `purpose = 2` row gave an Expand-only derivation over the 30-byte
  `info`. Both claimed the native backend. §4.4 now carries a per-backend
  table, §6.3 defers to it, and §9's vector 2 pins both.
- **§4.4 hashed "plaintext"**, a term §5.5 defines as `preamble || content` —
  which is circular, since the preamble contains the name tag. It now hashes
  `content`, and says why.
- **§3.1 claimed object names survive a backend swap**; §7 said every name
  changes. §7 was right. §3.1 now distinguishes a shared *construction* from
  shared *bytes*, and the seam is justified by §3.2's contract instead of by
  a migration property it never had. §7 no longer calls itself "not a
  re-encryption" while describing one.
- **age's lack of sender authentication was unstated.** The recipient public
  key is sufficient to forge any object under any `Binding`, which the native
  backend's symmetric DEK does not permit. §1.1, §5.4, and §8 now say so, and
  §8 makes "the recipient never leaves the process" an explicit MUST — it was
  previously an accident of implementation that the malicious-provider
  defence depended on. §5.4 no longer claims equivalence with §6.5.
- **`max_work_factor` lowered 19 → 18.** 19 admitted a 512 MiB attacker-named
  allocation on the device class §6.4 budgets at 256 MiB.
- **Slot writes gained a crash-atomicity rule** (§5.2). The native backend
  already required an atomic header write; the backend that actually ships
  required nothing, and a torn `passphrase.age` costs the user their
  passphrase.
- **§4.2 now names the `rollback_counter` read-only latch as an
  attacker-triggerable denial of service** and requires the client to say so
  rather than silently dropping writes.
- `KeyStore::change_passphrase` no longer re-takes `root` (§3).

**2026-08-14 (second revision)** — restructured around a swappable sealing
backend; M1 now ships `age` rather than the native format, to save
implementation time. What changed and why:

- Added §3's `KeyStore` / `ObjectSealer` seam and the `Binding` type, so the
  backend is replaceable and the security contract is explicit rather than
  implicit in whichever backend is installed.
- §5 specifies the age backend: slot files instead of slot records (age
  permits only one scrypt stanza per file), an age X25519 identity as the
  vault secret, and pinned scrypt work factors — `age` otherwise calibrates to
  the encrypting device and *rejects* files whose work factor exceeds the
  decrypting device's target by more than 4, which would make a
  desktop-created vault unopenable on a phone.
- §5.4 adds the binding preamble, recovering inside the plaintext what age
  cannot carry as associated data.
- The header lost its slot records and shrank to 44 bytes, gaining
  `backend_id`.
- The previous native format is preserved in §6 as the M2 target, unchanged in
  substance and still byte-exact: the slot record and `SlotAD`, the envelope
  and `ObjectAD`, the HKDF `info`, the slot-type table, the unlock procedure,
  and the Argon2id floor, ceiling, and calibration are all still specified
  here, and are itemised as M2 work in the M1 plan's "Deferred to M2" section.
  Nothing about Argon2id was dropped — an earlier draft of this revision
  condensed the two associated-data layouts to prose, which §9 explicitly
  forbids, and they were restored.
- **Requirement F4 (Argon2 key derivation) is not met at M1** and is deferred
  to M2 (§1.1). Recorded in the parent design's §2.

**2026-08-14 (first revision)** — original ratification of the native format.
Decisions from that review that still hold: header authenticity via
per-slot/per-object binding rather than a header MAC; slots bound to
`slot_id` rather than index; envelopes not self-describing; the name tag
truncated to 160 bits because it is keyed; no check character on the recovery
code; the Argon2id ceiling lowerable only for new vaults; read-only on an
unexpected `rollback_counter`.

### 10.1 Why age, and why not more of it

The survey behind this choice, recorded so it is not re-litigated:

`age` was the only mature candidate — a reviewed spec, a maintained Rust
implementation (MIT/Apache-2.0), and a recipient-stanza header that is
genuinely the same idea as §6.2's slot list. The other crates surveyed
(`aerovault`, `ferrocrypt`, `enc_file`, `rust-keyvault`) are single-author and
months old, and at least one is GPL-3.0-only, which this MIT project could not
take as a dependency regardless of merit.

Its three limitations were understood before adoption: scrypt rather than
Argon2id (§1.1), one scrypt stanza per file (solved by §5.2's slot directory),
and no caller-supplied associated data (solved by §5.4's preamble). The first
is the only one that remains a genuine gap at M1, and it is a deferred
requirement rather than a security hole.

What remains true regardless of backend: **this document specifies a
container, not a cryptosystem.** Every primitive is a third-party audited
implementation — the RustCrypto ChaCha20-Poly1305 that age builds on has been
reviewed by NCC Group — and the encoding is a third-party crate too. What is
written here is framing, field layout, and binding: serialization code, whose
requirements (keyed content-addressing, per-object binding, a device-portable
KDF ceiling, a reserved rollback field) are specific to this app and are not on
offer anywhere.
