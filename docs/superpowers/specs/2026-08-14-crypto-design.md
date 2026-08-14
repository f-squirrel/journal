# Journal — Cryptographic Design

Date: 2026-08-14
Status: Ratified — normative for M1 implementation.
Parent design: [`2026-08-14-journaling-app-design.md`](2026-08-14-journaling-app-design.md)
(§3 threat model, §5 cryptography, §6 storage).

This document specifies the scheme the parent design sketched, at the level of
detail required to implement it twice, independently. That is not a figure of
speech: M1 Task 16 writes `tools/decrypt.py` from **this document alone**,
never from the Rust source. If that decryptor cannot open a vault the app
wrote, this document is wrong or incomplete, and the defect is here rather
than in the code.

Everything below is normative. "MUST", "MUST NOT", "SHOULD", and "MAY" carry
their RFC 2119 meanings. Byte offsets are zero-based; all multi-byte integers
are **little-endian**; `X[a..b]` is the half-open byte range.

## 1. Scope and non-goals

In scope (M1): key hierarchy, key slots, Argon2id parameters and calibration,
the AEAD envelope, associated-data construction, the vault header, keyed
object naming, and the recovery code.

Explicitly **not** specified here, because they are not M1:

- The whole-vault rollback mechanism. The header *reserves the field* (§5.2)
  and §9 states the M2 rules that will use it; M1 neither maintains nor
  enforces a counter.
- Device/biometric key slots (M3). The slot type value is reserved (§4.2).
- Object size padding. Ciphertext length equals plaintext length plus a fixed
  48-byte overhead, so entry and image sizes remain visible — a stated
  non-defense (parent §3) and an open question (parent §9).
- DEK rotation. The derivation supports it (§3.4); the operation — re-encrypt
  every object under new per-object keys — is not implemented at M1.
- Sync, conflict handling, and multi-replica garbage collection.

## 2. Primitives

| Role | Primitive | Parameters |
|---|---|---|
| Password hashing | Argon2id | version 0x13 (19), see §6 |
| AEAD | XChaCha20-Poly1305 | 256-bit key, 192-bit nonce, 128-bit tag |
| Key derivation from the DEK | HKDF-Expand-SHA-256 | Expand only, see §3.4 |
| Object naming | HMAC-SHA-256 | truncated to 160 bits, see §8 |
| Content hashing | SHA-256 | full 256-bit output |
| Checksums (non-security) | SHA-256, truncated | see §5.1 |
| Randomness | OS CSPRNG | `getrandom`, see §2.1 |

Rust crates: `argon2`, `chacha20poly1305`, `hkdf`, `hmac`, `sha2`, `zeroize`,
`secrecy`, `getrandom`, `subtle`, and `base32` (for §7.2 and §8). No
hand-rolled primitives, ever — and that extends to the encoding: the
`base32` crate's `Alphabet::Crockford` is exactly the alphabet of §7.2, and
its decoder already performs the case folding and confusable mapping that
§7.3 requires, so neither the encoder nor the normalizer is written here.

HKDF is used in **Expand-only** form wherever its input keying material is
already a uniformly random 256-bit key (RFC 5869 §3.3 explicitly permits
skipping Extract in that case). Running Extract over a value that is already
uniform buys nothing and adds a step the independent decryptor could get
wrong.

### 2.1 Randomness

The DEK, the recovery code, every salt, every nonce, and the vault ID MUST
come from the operating system CSPRNG (`getrandom`), obtained in a single call
per value. Implementations MUST NOT seed a userspace PRNG from it and draw
from that instead, MUST NOT reuse a buffer across two values, and MUST treat a
CSPRNG failure as fatal — never as a reason to fall back to any other source.

### 2.2 Nonces

Every XChaCha20-Poly1305 nonce in this format is 192 random bits drawn fresh
per encryption. No counter, no derivation, no reuse across objects or across
re-encryptions of the same object. At 2^48 encryptions the collision
probability is about 2^-97; a journal will not approach 2^20. This is the
entire reason XChaCha20 was chosen over ChaCha20's 96-bit nonce, and it is why
this format needs no nonce-management state anywhere.

## 3. Key hierarchy

```
passphrase ──── Argon2id(salt_p, m/t/p) ──► KEK_pass ──┐
recovery code ─ Argon2id(salt_r, m/t/p) ──► KEK_rec  ──┤ each slot wraps its
device key ──── OS keystore (M3+) ────────► KEK_dev  ──┘ own copy of one DEK
                                                        │
                                                 DEK (256 bits, random)
                                                        │
     HKDF-Expand(DEK, purpose=1, kind, object_id) ──► K_obj  (per object)
     HKDF-Expand(DEK, purpose=2)                  ──► K_name (object naming)
     HKDF-Expand(DEK, purpose=3)                  ──► reserved (M2 rollback)
```

### 3.1 The DEK

32 bytes from the CSPRNG, generated once at vault creation and never derived
from anything. It is the only value whose loss destroys the vault, which is
why it is wrapped by two independent slots from the first moment the vault
exists (§4).

The DEK exists in plaintext only in process memory between unlock and lock. It
MUST be held in a `zeroize`-on-drop wrapper, MUST NOT implement `Serialize` or
`Debug`, and MUST NOT cross the Tauri IPC boundary in any form.

### 3.2 KEKs

A KEK is the 32-byte Argon2id output for a slot (§6). It is used exactly once
per unlock attempt — to unwrap that slot — and zeroized immediately after,
whether the unwrap succeeded or failed.

### 3.3 Passphrase and recovery-code encoding

The Argon2id password input is bytes, so the encoding is part of the format:

- **Passphrase**: normalize to Unicode **NFC**, then encode as UTF-8. No
  trimming, no case folding, no whitespace collapsing. Trimming would silently
  discard user-chosen entropy and make two different passphrases unlock the
  same vault; a leading space is a character like any other.
- **Recovery code**: the password input is the **decoded 20 bytes** (§7), not
  the display string. Formatting, case, and separators therefore cannot affect
  the derived key, which is what lets §7.3 accept sloppy transcription without
  any leniency in the crypto.

### 3.4 Derivation from the DEK

All keys below the DEK come from HKDF-Expand-SHA-256 with `L = 32` and this
`info`, which is **always exactly 30 bytes** — fixed width in every field, so
no two distinct derivations can produce the same `info` string:

```
offset  size  field
0       8     b"JRNLKDF1"
8       1     purpose        u8
9       1     object_kind    u8   (0 when purpose != 1)
10      20    object_id           (20 zero bytes when purpose != 1)
```

| `purpose` | Key | `object_kind` / `object_id` |
|---|---|---|
| 1 | `K_obj` — the AEAD key for one object | the object's kind and ID (§5.3, §8) |
| 2 | `K_name` — the HMAC key for object naming (§8) | 0 / zeros |
| 3 | reserved for the M2 rollback-counter MAC key | 0 / zeros |
| 4–255 | reserved; MUST NOT be used at M1 | — |

Consequences worth stating, because they are the point of the hierarchy:

- Compromise of one `K_obj` reveals one object and nothing else — it is not
  invertible back to the DEK.
- **Changing the passphrase rewraps one slot and re-encrypts nothing.** Object
  keys derive from the DEK, which does not change.
- **Rotating the DEK re-encrypts the entire vault**, because every `K_obj`
  and every object *name* (§8) changes. This is a recovery operation, not a
  routine one. The parent design's §10 records that the original draft got
  this backwards.

## 4. Key slots

### 4.1 Model

The header carries a list of 1–8 slots, LUKS-style. Each slot independently
wraps the same DEK under its own KEK, with its own salt, its own KDF
parameters, and its own nonce. Adding an unlock method is a new row; it is not
a format migration. A vault created at M1 has exactly two slots: one
passphrase, one recovery.

Each slot has a 16-byte random `slot_id`, unique within the header. The
`slot_id` — **not** the slot's position — is what the wrap binds to (§4.4), so
slots may be reordered or removed without invalidating the others. Binding to
an index would mean deleting slot 0 silently broke slot 1, which is exactly
the kind of latent format trap this design is trying to avoid.

### 4.2 Slot types

| Value | Type | Milestone |
|---|---|---|
| 1 | passphrase | M1 |
| 2 | recovery code | M1 |
| 3 | device key (Keychain / Keystore) | M3 |
| 4–255 | reserved | — |

A header MUST contain at least one slot of type 1. A reader that meets an
unknown slot type MUST ignore that slot (it cannot unwrap it) and MUST NOT
treat the header as invalid — that is what makes M3's slot additive. A reader
MUST NOT write to a header containing slot types it does not understand.

### 4.3 Slot record (116 bytes, fixed)

```
offset  size  field           notes
0       1     slot_type       u8, §4.2
1       1     kdf_id          u8, 1 = Argon2id v0x13; other values: unknown slot
2       1     lanes           u8, Argon2id p, MUST be in 1..=8
3       1     reserved        MUST be 0 on write; readers MUST reject non-zero
4       4     mem_kib         u32, Argon2id m in KiB
8       4     iterations      u32, Argon2id t
12      16    slot_id         random, unique within the header
28      16    salt            random, 128 bits, unique per slot per rewrap
44      24    wrap_nonce      random, 192 bits, fresh on every wrap
68      48    wrapped_dek     XChaCha20-Poly1305: 32 ciphertext || 16 tag
```

`salt` and `wrap_nonce` MUST be regenerated on every wrap, including a
passphrase change. `slot_id` MUST NOT change once assigned — it identifies the
slot across rewraps.

### 4.4 Wrap and unwrap

```
KEK           = Argon2id(pwd, salt = slot.salt, m, t, p, out_len = 32)     §6
wrapped_dek   = XChaCha20Poly1305_Seal(key = KEK, nonce = slot.wrap_nonce,
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

This is what gives the header its integrity, and it is why there is no
header MAC: a header MAC would need a key the reader does not have until
*after* it unwraps a slot. Instead, every field an attacker could usefully
tamper with is in the AAD of the wrap it governs. Lowering `mem_kib` to
weaken the KDF, swapping a salt, or editing the format version changes
`SlotAD`, so the Poly1305 tag fails and the vault does not open. The attacker
converts a downgrade into a denial of service, which is the trade this format
accepts.

`vault_id` in `SlotAD` additionally prevents transplanting a slot record from
one vault's header into another's.

**Partitioning-oracle mitigation.** Before attempting any unwrap, the reader
MUST confirm that `wrapped_dek` is exactly 48 bytes — which the fixed 116-byte
slot record of §4.3 already guarantees structurally, so this is a check that
an implementation must not *lose*, rather than one it must add. A
password-based AEAD whose ciphertext length is attacker-controlled admits a
partitioning oracle: a crafted multi-key ciphertext that decrypts successfully
under many candidate passphrases turns each online attempt into a test of a
whole set rather than a single guess. The age specification learned this the
hard way and imposes the same rule on its scrypt stanza; the fixed-width slot
record is why this format gets it for free.

### 4.5 Unlock procedure

Given a candidate secret and its type (`1` for a passphrase attempt, `2` for a
recovery-code attempt):

1. Parse and validate the header (§5.1). Reject before any KDF work if it is
   malformed.
2. Select the slots whose `slot_type` matches the attempt type, in header
   order. Slots of other types MUST NOT be tried — a passphrase must never be
   run against the recovery slot.
3. For each selected slot: derive the KEK per §6, attempt the unwrap of §4.4.
   On success, the plaintext is the DEK; zeroize the KEK and stop.
4. If every selected slot fails, zeroize all intermediates and return a single
   generic failure.

Rules that are part of the format, not implementation taste:

- The error MUST NOT reveal which slot failed, how many slots exist, or how
  far the attempt got. One message: the vault did not unlock.
- The implementation MUST NOT short-circuit on a cheap pre-check of the
  secret. There is no such check — the Argon2id run *is* the check.
- On failure, nothing is written to disk. Not a failure counter, not a
  timestamp. M1 has no rate limiting at rest; the KDF cost is the rate limit,
  and a write-on-failure would leak attempt metadata to anyone holding the
  directory.

### 4.6 Passphrase change

Unwrap the DEK through the existing passphrase slot, then rewrap **that same
DEK** into the same `slot_id` with a fresh salt, fresh nonce, and freshly
calibrated parameters. Write the header atomically (M1 Task 7). No object is
read, re-encrypted, or rewritten; the recovery slot is untouched and the
recovery code still works. This is verifiable and Task 6 verifies it: after a
passphrase change every object file MUST be byte-identical.

## 5. Vault format

### 5.1 Header — `<vault>/header.jvh`

```
offset       size      field
0            8         magic          b"JRNLVLT\x00"
8            2         format_major   u16, = 1
10           2         format_minor   u16, = 0
12           16        vault_id       random, public, no security role
28           8         rollback_counter u64 — RESERVED, see §5.2
36           1         slot_count     u8, MUST be in 1..=8
37           3         reserved       MUST be 0
40           116*N     slots          N = slot_count, §4.3
40+116*N     4         header_checksum
```

Total size = `44 + 116 * slot_count` bytes; 276 bytes for an M1 vault. A
header of any other length for its `slot_count` MUST be rejected as corrupt.

`header_checksum` = the first 4 bytes of `SHA-256(header[0 .. 40+116*N])`. It
is a **corruption detector and nothing more** — it is unkeyed, so anyone can
recompute it after tampering. It exists so that a truncated or bit-rotted
header produces "this vault's header is corrupt" instead of "wrong
passphrase", which is the difference between a user restoring a backup and a
user believing they forgot their passphrase. Authenticity comes from §4.4.

`vault_id` is public and identifies the vault across replicas (M2). It MUST
NOT be derived from the passphrase, the DEK, or anything secret, and nothing
in this design depends on it being unpredictable.

### 5.2 The reserved rollback-counter field

M1 writers MUST write `rollback_counter = 0`. M1 readers MUST ignore its
value for security purposes, and MUST NOT reject a non-zero value.

One rule makes the field safe to reserve rather than merely present: **an M1
client that reads a header with a non-zero `rollback_counter` MUST open the
vault read-only.** A non-zero counter means some client is maintaining
rollback protection that this client cannot maintain; writing would silently
break the invariant that client depends on. Refusing to write is cheap and
correct; the alternative is a subtle cross-version failure that would surface
only under attack.

The field is 8 bytes at a fixed offset so M2 can populate it without moving
anything. The mechanism itself — how the counter advances, how a client
remembers the last value it saw, how it is authenticated with the
`purpose = 3` key of §3.4 — is M2's spec. Whole-vault rollback remains a
stated non-defense at M1 (parent §3).

### 5.3 Object kinds and IDs

| `object_kind` | Object | `object_id` |
|---|---|---|
| 1 | the manifest | 20 zero bytes |
| 2 | an entry | its 20-byte name tag (§8) |
| 3 | an attachment blob | its 20-byte name tag (§8) |
| 4–255 | reserved | — |

The manifest is mutable and lives at a fixed path, so it is the one object
that is not content-addressed; its all-zero ID cannot collide with a name tag
in another kind's namespace because the kind is bound alongside the ID
everywhere it matters (§3.4, §5.5).

### 5.4 Directory layout

```
<vault>/
├── header.jvh                      §5.1, plaintext, public
├── manifest.jenv                   envelope, kind = 1, id = 20 zero bytes
└── objects/
    └── <name[0..2]>/<name>.jenv    envelope, kind = 2 or 3, id = name tag
```

`<name>` is the 32-character lowercase encoding of §8. The two-character shard
directory keeps any one directory to roughly 1/1024 of the objects, which
matters on filesystems that degrade with large directories and costs nothing
on those that do not.

The layout carries **no plaintext metadata**: no titles, no dates, no
extensions that distinguish an entry from an image. A directory listing
reveals the object count and each object's size — the shape metadata the
parent design's §3 already concedes.

### 5.5 The AEAD envelope — `*.jenv`

Every encrypted object on disk, manifest included, is one envelope:

```
offset  size  field
0       4     magic         b"JENV"
4       1     env_version   u8, = 1
5       1     alg_id        u8, 1 = XChaCha20-Poly1305
6       2     reserved      u16, MUST be 0; readers MUST reject non-zero
8       24    nonce         random, 192 bits, §2.2
32      n     ciphertext    n = plaintext length
32+n    16    tag           Poly1305
```

File length is `48 + n`; a file shorter than 48 bytes is truncated and MUST be
rejected as such. Plaintext length is the file length minus 48 — there is no
length field, because a length field would be a second source of truth that
the AEAD would then have to be trusted to reconcile.

Readers MUST reject `n > 64 MiB` **before allocating**, so a corrupt or
hostile file size cannot turn into an enormous allocation. Objects are sealed
and opened whole, not streamed: images are decrypted into memory and then
streamed to the WebView by M1 Task 12's protocol handler.

The envelope is deliberately **not self-describing**. It does not record which
object it is, what kind it is, or which vault it belongs to. That information
comes from the reader's context — the path it opened, the manifest entry it
followed — and enters the AAD (§5.6). An object moved to another name, copied
into another vault, or served in place of a different object therefore fails
to open rather than opening as something it is not.

### 5.6 Associated data for objects

`ObjectAD` is **exactly 60 bytes**:

```
offset  size  field
0       8     b"JRNLOBJ1"
8       2     format_major       u16, from the header
10      2     format_minor       u16, from the header
12      16    vault_id           from the header
28      1     object_kind        §5.3
29      3     reserved (0)
32      20    object_id          §5.3
52      8     envelope[0..8]     magic, env_version, alg_id, reserved
```

Sealing and opening:

```
K_obj      = HKDF-Expand(DEK, info = §3.4 with purpose=1, kind, object_id, L=32)
ciphertext = XChaCha20Poly1305_Seal(K_obj, nonce, plaintext, aad = ObjectAD)
plaintext  = XChaCha20Poly1305_Open(K_obj, nonce, ciphertext||tag, aad = ObjectAD)
```

What each AAD field buys, since Task 16's decryptor has to reproduce all of
them and a reviewer should be able to check the reasoning:

- `object_kind` + `object_id` — an object cannot be substituted for another,
  nor rolled back to an older version of itself, because an older version of
  an entry has different content and therefore a different name tag and a
  different ID. This is the per-object binding the parent §3 credits, and its
  limit is equally explicit: it does not stop rollback of the *whole vault*.
- `format_major` / `format_minor` — a downgrade to an older format's parsing
  rules invalidates every tag.
- `vault_id` — objects cannot be transplanted between vaults.
- `envelope[0..8]` — `alg_id` and `env_version` are authenticated, so an
  attacker cannot claim a weaker algorithm for an existing ciphertext.

`K_obj` is bound to the object ID twice over — once in the derivation, once in
the AAD. That redundancy is intentional: it costs nothing and it means an
implementation that gets the derivation right but the AAD wrong (or vice
versa) still fails closed.

### 5.7 Version rules

- **Unknown `format_major`** — the client MUST refuse to open the vault and
  MUST NOT write a single byte to it. No partial upgrades, no half-migrated
  vaults (parent §5).
- **`format_minor` higher than the client understands** — the client MAY open
  the vault **read-only** and MUST NOT write to it. A higher minor may carry
  fields this client would drop on rewrite; refusing the write preserves them.
- **`format_minor` lower** — open read-write; the format is compatible by
  construction.
- **Unknown `slot_type`** — ignore that slot, do not write (§4.2).
- **`env_version` or `alg_id` unknown in an envelope** — reject that object as
  unreadable. Do not attempt to decrypt with a default.

A "major" bump is any change to the layouts in §4.3, §5.1, §5.5, either AAD
construction, or any derivation in §3.4. A "minor" bump is an additive change
that older readers can ignore safely — a new slot type, a new object kind.

## 6. Argon2id parameters and calibration

### 6.1 Fixed choices

Argon2**id**, version 0x13 (19), 128-bit salt, 32-byte output, no secret key,
no associated data. Lanes `p = 4` is fixed for M1 and stored per slot.

`p` is a **format parameter, not a thread count**. It changes the function's
output, so it must be reproduced exactly; an implementation MAY execute those
4 lanes on fewer threads (and on a single core it simply runs them in
sequence). Task 16's decryptor, single-threaded Python, must produce identical
output — which it will, because Argon2id is defined that way.

### 6.2 The floor

```
FLOOR:   m = 65536 KiB (64 MiB),  t = 3,  p = 4
```

This is RFC 9106 §4's SECOND RECOMMENDED option verbatim. Parameters below the
floor MUST be rejected at unlock, not merely warned about: a vault whose
header says `m = 8 KiB` is either corrupt or attacker-edited, and in the
latter case opening it is precisely the downgrade §4.4 exists to stop. The
AAD binding already makes such a header fail to unwrap; the explicit floor
check makes it fail with an accurate error and covers the case of a vault
legitimately created by a defective client.

Rejecting at unlock means a below-floor vault cannot be opened at all rather
than being opened and upgraded. That is the intended behavior: the app cannot
know the vault was not created under attack.

### 6.3 The ceiling

```
CEILING: m = 262144 KiB (256 MiB),  t = 10,  p = 4
```

The binding constraint on this format is a **ceiling**, not a floor, and this
is the direction the parent design's §10 records as previously wrong. A vault
must stay openable on the weakest device its owner will ever unlock it on. A
desktop that calibrates to 2 GiB of Argon2 memory produces a vault that a
phone cannot open — and the failure arrives months later, on the device the
user reaches for at the moment they need it.

**Weakest supported device class** (provisional): a 64-bit ARM phone,
2019 or later, with at least 3 GiB of RAM, where a foreground allocation of
256 MiB plus the app's own working set stays clear of the platform's
low-memory killer. Budget: an unlock at CEILING parameters completes within
3 seconds on that class.

These numbers are provisional and the parent design's §9 lists measuring them
on real hardware as an open question, to be resolved when a phone build
exists (M3). The revision rule matters more than the numbers:

- The ceiling MAY be **raised** for newly created vaults at any time.
- The ceiling MAY be **lowered** only for newly created vaults. Existing
  vaults keep their header parameters and keep opening on capable devices —
  lowering the ceiling cannot retroactively fix a vault already calibrated
  above it. If M3 measurement shows 256 MiB is too much for the weakest class,
  vaults created before that discovery may be unopenable on such a phone, and
  the honest remedy is to unlock on a capable device and change the passphrase
  (§4.6), which recalibrates.

Both bounds MUST be enforced on read as well as on write: parameters outside
`[FLOOR, CEILING]` are rejected at unlock.

### 6.4 Calibration procedure

Run at vault creation and at every passphrase change, on the creating device.
Target `TARGET_MS = 1000` — a delay a user will accept at unlock, repeated
every time they open the app.

```
1.  p := 4;  m := FLOOR.m;  t := FLOOR.t
2.  d0 := median of 3 runs of Argon2id(FLOOR.m, FLOOR.t, p) over a
         throwaway password and salt, measured as wall clock

    #  cost is near-linear in both m and t, so from that one measurement:
    #     predict(m, t) = d0 * (m / FLOOR.m) * (t / FLOOR.t)

3.  if d0 >= TARGET_MS:
        return FLOOR                       # weak device; never go below it
4.  while 2*m <= CEILING.m  and  predict(2*m, t) <= TARGET_MS:
        m := 2*m                           # memory first — see below
5.  while t+1 <= CEILING.t  and  predict(m, t+1) <= TARGET_MS:
        t := t + 1
6.  measured := one run of Argon2id(m, t, p)
7.  if measured > 1.5 * TARGET_MS:
        step back one increment — halve m if m > FLOOR.m,
        else decrement t if t > FLOOR.t, else return FLOOR —
        then repeat from 6, at most 3 times
8.  return (m, t, p)                       # inside [FLOOR, CEILING] by 3-7
```

Argon2id's cost is close to linear in both `m` and `t`, which is what makes
the prediction in steps 4–5 sound; step 6 re-measures rather than trusting it,
and step 7 bounds the correction so calibration cannot loop.

**Memory before iterations, deliberately.** Resistance to GPU and ASIC attack
scales with the memory an attacker must provision per guess; iterations only
raise the cost linearly for attacker and defender alike. Given a fixed time
budget, spending it on `m` buys strictly more than spending it on `t`.

Calibration MUST run on a throwaway password and salt — never on the user's
real passphrase, which would put it through timing paths that vary with the
value being measured.

Both slots created at vault creation get the same calibrated parameters and
different salts. A passphrase change recalibrates the passphrase slot only;
the recovery slot keeps the parameters it was created with, which is correct
— its input has 160 bits of entropy and the KDF is not load-bearing there
(§7.1).

Task 4 verifies: RFC 9106 known-answer vectors pass; calibration on the CI
runner lands inside `[FLOOR, CEILING]`; a header below the floor is rejected.

## 7. Recovery code

### 7.1 Entropy and encoding

- **160 bits** — 20 bytes from the CSPRNG (§2.1), generated by the app, never
  user-chosen, never derived from the passphrase, shown exactly once at vault
  creation.
- Encoded with **Crockford Base32** (§7.2): 160 bits / 5 bits per character =
  **32 characters exactly**, with no padding and no leftover bits. This
  matters beyond tidiness: an encoding with partial trailing bits admits
  several distinct strings that decode to the same value, which is an
  ambiguity a recovery path should not have.
- Displayed as **8 groups of 4**, uppercase, hyphen-separated:

  ```
  K7M2-9XQ4-BVN3-8HTP-Z5RW-6JYC-D2GK-4NFB
  ```

160 bits comfortably exceeds the parent design's ≥128-bit requirement and is
the same width as the object name tags (§8), so one encoder serves both.

At this entropy the Argon2id pass over the recovery code is **defence in
depth, not the security boundary** — brute-forcing 160 bits is infeasible
regardless of the KDF. It runs anyway because a uniform slot format is worth
more than the second it saves, and because it keeps the recovery path from
becoming a fast oracle if a future code is ever shortened.

The recovery code is a **second wrapping of the same DEK, not an escrow**. No
one else holds a copy; the app cannot recover a vault whose passphrase and
recovery code are both lost. That is the cost of the model (parent §3).

### 7.2 Alphabet

Crockford Base32, 32 symbols:

```
0 1 2 3 4 5 6 7 8 9 A B C D E F G H J K M N P Q R S T V W X Y Z
```

`I`, `L`, `O`, and `U` are **absent**. This is what satisfies the plan's
"no `0`/`O`, no `1`/`l`" requirement: the confusable letters are not in the
alphabet at all, so a code never contains both members of a confusable pair,
and a reader who transcribes `O` for `0` or `l` for `1` produces a string the
decoder maps back to the right value (§7.3). `U` is excluded so that generated
codes cannot accidentally spell obscenities.

Canonical **display** form is uppercase with hyphens. Canonical **object-name**
form is lowercase without hyphens (§8) — lowercase because object names are
filenames, and case-insensitive filesystems on macOS and Windows make any
mixed-case naming scheme a collision waiting to happen.

### 7.3 Parsing

Before any KDF work, normalize the input:

1. Strip all hyphens, spaces, tabs, and newlines.
2. Reject if the result is not exactly 32 characters. Reject cleanly — a
   malformed code is a typo, not an exception, and never a panic.
3. Decode with `base32::decode(Alphabet::Crockford, …)`, which folds case and
   maps the confusables (`O`/`o` → `0`, `I`/`i`/`L`/`l` → `1`) as part of
   decoding; reject if it returns `None` or anything other than 20 bytes.
4. Those 20 bytes are the Argon2id password input (§3.3).

Steps 2–3 are deliberately *not* a hand-written normalizer. An independent
implementation (Task 16, Python) should reach for its own Crockford library
rather than reproducing a mapping table; the behaviour to match is
"Crockford Base32", which is a citable standard, not a table in this
document. The mapping is stated in parentheses above only so a reviewer can
confirm a candidate library does it.

There is **no check character**. A typo that survives normalization costs one
Argon2id run (~1 second) and reports the same generic failure as a wrong
passphrase (§4.5). The alternative — a check symbol — would add a symbol
outside the alphabet and a second thing for Task 16's decryptor to reproduce,
in exchange for saving a second on a path most users walk once or never.

Task 6 verifies: unlock through the recovery slot; case, hyphen, and
confusable variations of the same code all unlock; malformed codes are
rejected without panicking.

## 8. Object naming

Objects are named by a **keyed** tag, never by the hash of their plaintext:

```
K_name   = HKDF-Expand(DEK, info = §3.4 with purpose = 2, L = 32)
h        = SHA-256(plaintext)                          # 32 bytes
tag      = HMAC-SHA-256(K_name, b"JRNLNAM1" || kind_u8 || h)   # 41-byte input
name_tag = tag[0..20]                                  # 160 bits
name     = crockford_base32(name_tag), lowercased       # exactly 32 chars
                                                        # `base32` crate, §2
```

`name_tag` is the object's `object_id` in §3.4 and §5.6, so the name, the key,
and the AAD all agree by construction.

**Why keyed.** An object stored under the hash of its own plaintext is a
confirmation oracle: anyone holding the vault directory can hash a file they
suspect the user has — a specific photograph, a leaked document — and check
whether that name is present. Keying with `K_name` leaves deduplication intact
*inside* a vault (identical input yields an identical name, so a duplicate
image is stored once) while making the name meaningless to anyone without the
DEK. This closes the oracle the parent design's §10 records as a revision.

Task 9's regression test is written to read as what it is: **a blob's on-disk
name equals no hash of its plaintext.**

`kind_u8` inside the HMAC keeps entry names and blob names in separate
namespaces, so an entry whose serialized bytes happened to equal an image's
bytes would still be two objects.

160 bits of tag gives a birthday bound around 2^80 objects. Truncation is safe
here for a reason worth stating: the tag is keyed, so an attacker cannot
search for a colliding plaintext without `K_name`, and a *user* cannot produce
one by accident at any vault size this app will ever see.

Content addressing makes objects immutable: editing an entry writes a new
object under a new name and repoints the manifest. Deletion is a manifest
tombstone plus a later garbage-collection pass over unreferenced blobs
(parent §6), not the absence of an object.

## 9. Operational rules

**Zeroization.** The DEK, every KEK, every `K_obj`, `K_name`, the decoded
recovery code, and the normalized passphrase bytes MUST be held in
`zeroize`-on-drop wrappers and dropped as early as possible — a KEK dies at
the end of its unwrap attempt, not at the end of unlock. Lock zeroizes the DEK
and every derived key and drops the in-memory index. Rust cannot promise the
compiler kept no copy, and the OS may have paged one to swap; that limit is a
stated non-defense (parent §3), and it is the reason locking also tears down
the WebView rather than pretending to scrub its heap.

**No `Serialize`, no `Debug`.** Key material types MUST implement neither.
Task 5 asserts this with a `trybuild` compile-fail test, so the guarantee is
enforced by the build rather than by review.

**No secrets in logs.** No key material, entry body, title, passphrase, or
recovery code reaches `tracing` at any level. Task 16 audits this by running
the app at maximum verbosity and grepping for a canary.

**No secrets across IPC.** No Tauri command returns key material, slot bytes,
or vault file paths. The passphrase enters Rust and never comes back.

**Comparisons.** All secret-dependent equality checks go through `subtle`'s
constant-time comparisons. In practice the AEAD tag checks are the only ones
that matter and the AEAD crate handles them; object names are public and may
be compared normally.

**Failure is closed.** Every parse rule in §5 rejects rather than repairs.
There is no "best effort" open, no field the reader guesses at, and no path
that writes to a vault it could not fully validate.

## 10. Conformance

M1 Task 5 MUST emit golden test vectors under
`crates/journal-crypto/tests/vectors/`, checked into the repository, covering
at minimum:

1. **Argon2id** — RFC 9106 known-answer vectors, plus one vector at FLOOR
   parameters with a fixed salt and password.
2. **Slot wrap** — a fixed DEK, KEK inputs, salt, `slot_id`, nonce, and header
   fields, with the resulting 72-byte `SlotAD` and 48-byte `wrapped_dek`
   recorded as hex.
3. **Object envelope** — a fixed DEK, kind, object ID, nonce, and plaintext,
   with the 30-byte HKDF `info`, the derived `K_obj`, the 60-byte `ObjectAD`,
   and the complete envelope file recorded as hex.
4. **Object naming** — a fixed DEK and plaintext, with `K_name`, the HMAC
   input, the 20-byte `name_tag`, and the 32-character name.
5. **Recovery code** — a fixed 20-byte value, its canonical display form, and
   three sloppy transcriptions (lowercase, no hyphens, `O`/`l` substituted)
   that MUST all decode to the same bytes.
6. **A complete two-slot vault** — header hex, manifest envelope, and one
   entry object, openable end to end.

The AAD structures are recorded as hex on purpose. They are the part of this
format most likely to be implemented subtly differently by two people reading
the same prose, and a mismatch there produces an authentication failure whose
cause is invisible without the intermediate value.

Task 16's `tools/decrypt.py` MUST be written from this document without
reading the Rust implementation, and MUST open vector 6 and a vault produced
by the real app. That is the acceptance test for this specification, not just
for the code.

## 11. Review notes

Reviewed 2026-08-14 before any crypto code was written, per M1 Task 2. Points
raised and settled during the review, recorded because the reasoning is the
part that will be hard to reconstruct later:

- **Header authenticity without a header MAC.** A MAC over the header needs a
  key that only exists after a slot is unwrapped. Binding the header's
  security-relevant fields into each slot's AAD (§4.4) achieves the same end
  with no chicken-and-egg, at the cost that tampering yields "vault will not
  open" rather than "vault was tampered with". Accepted: for a local-first
  vault with no server to appeal to, both outcomes lead the user to the same
  action — restore a backup.
- **Slots bound to `slot_id`, not to their index** (§4.1). Index binding would
  make removing a slot break the remaining ones — a trap that would surface
  the first time a user deleted a device slot in M3.
- **Envelopes are not self-describing** (§5.5). Storing the object ID in the
  file would make misplaced objects openable and would add a second source of
  truth for a value the AAD already fixes.
- **Truncating the name tag to 160 bits** (§8). Justified because the tag is
  keyed; the encoding then aligns with the recovery code so one base32 routine
  serves both.
- **Crockford Base32 for the recovery code** (§7.2), rather than an alphabet
  that also drops the digits `0` and `1`. Both are exactly 32 symbols and
  both pack 160 bits cleanly; Crockford wins because it is a named standard
  with existing implementations, which matters specifically for Task 16 —
  the independent decryptor can use an off-the-shelf codec instead of
  re-deriving a bespoke table from this prose. This diverges from the M1
  plan's literal "no `0`/`O`, no `1`/`l`" wording and was confirmed with the
  project owner before ratification; the plan's Task 6 line now points here.
- **No check character on the recovery code** (§7.3). The cost of a typo is
  one second and a clear error; a check symbol would widen the alphabet and
  add another thing for the independent decryptor to reproduce.
- **The ceiling can be lowered only for new vaults** (§6.3). Stated because
  the failure it describes — an existing vault that a phone cannot open —
  cannot be fixed by changing a constant, and someone will eventually try.
- **Read-only on an unexpected `rollback_counter`** (§5.2). Chosen over
  ignoring the field so that M1 clients cannot silently undermine an M2
  client's rollback protection.

### 11.1 Why this format rather than an off-the-shelf one

Asked and answered before ratification, and recorded here so it is not
re-litigated at implementation time. The survey found no crate that provides
this format, and one — [`age`](https://github.com/C2SP/C2SP/blob/main/age.md)
— that comes close enough to be worth ruling out explicitly.

`age` is the only mature candidate: a reviewed spec, a maintained Rust
implementation (`age`/rage, MIT/Apache-2.0), and a recipient-stanza header
that is genuinely the same idea as §4's slot list. It was rejected on three
points, each of which is a requirement rather than a preference:

1. **The passphrase recipient is scrypt, not Argon2id.** Requirement F4 of
   the parent design names Argon2, and §6's memory-hard calibration story is
   built on Argon2id's `m`/`t`/`p`.
2. **"An scrypt stanza, if present, MUST be the only stanza in the header."**
   A passphrase-protected age file therefore cannot also carry a recovery
   slot — which is the entire point of §4, and the extension point M3's
   device key depends on.
3. **No caller-supplied associated data.** age fixes `aad = ""`. §5.6's
   binding of object kind, object ID, and format version into the AAD is what
   stops object substitution and per-object rollback in the sync threat
   model, and it has nowhere to live in an age file.

Point 1 and 2 could be worked around by deriving an X25519 identity from an
Argon2id output and using ordinary recipient stanzas; point 3 could not, and
the workaround is exactly the sort of clever repurposing of a primitive that
this document exists to prevent. The other crates surveyed
(`aerovault`, `ferrocrypt`, `enc_file`, `rust-keyvault`) are single-author
and months old, and at least one is GPL-3.0-only, which this MIT project
could not take as a dependency regardless of its merits.

The conclusion worth carrying forward: **this document specifies a container,
not a cryptosystem.** Every primitive in §2 is an audited third-party crate —
the RustCrypto ChaCha20-Poly1305 implementation has been reviewed by NCC
Group — and the encoding of §7.2 is a third-party crate too. What is written
here is framing, field offsets, and AAD layout: serialization code, whose
requirements (keyed content-addressing, per-object AAD binding, a
cross-device KDF ceiling, a reserved rollback field) are specific to this
app and are not on offer anywhere.
```

