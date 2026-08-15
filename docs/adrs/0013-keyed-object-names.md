# 13. Name objects with a keyed HMAC tag, not a raw content hash

## Status

Accepted — 2026-08-15 (design doc §6; crypto spec §4.4; first revision
2026-08-14, corrected in the third revision)

## Context

Content addressing is the natural fit for this vault: an immutable object named
by its own content deduplicates for free, makes editing an entry a write of a
new object plus a manifest repoint, and reduces sync to "copy the objects the
other side is missing."

The obvious implementation — name each object `SHA-256(plaintext)` — quietly
destroys the property the encryption exists for. A raw content hash is a
**confirmation oracle**: anyone holding the vault directory can hash a file
they suspect the user has (a specific photograph, a leaked document, a known
article) and check whether that name is present. No decryption required.

This was caught in review of the first M1 plan, before implementation.

## Decision

Objects are named by a **keyed** tag:

```
h        = SHA-256(content)
tag      = HMAC-SHA-256(K_name, b"JRNLNAM1" || kind_u8 || h)
name_tag = tag[0..20]                      # 160 bits
name     = crockford_base32(name_tag), lowercased     # 32 chars
```

`content` is the object's own bytes — **not** `preamble || content`, since the
preamble carries the name tag and hashing it would be circular. `kind_u8`
inside the HMAC keeps entry names and blob names in separate namespaces.
`name_tag` *is* the `object_id` in the object's `Binding`, so the name and the
binding agree by construction.

`K_name` is derived **per backend** — full HKDF (Extract *and* Expand) over the
age identity string, because that string is a Bech32 encoding rather than a
uniform key; Expand-only over the native DEK, which is already uniformly random.
The two are not interchangeable, and golden vector 2 pins both.

Files live at `objects/<name[0..2]>/<name>.jobj`, sharding any one directory to
roughly 1/1024 of the objects.

## Consequences

- Deduplication still works *inside* a vault — identical input yields an
  identical name, so an image added twice is stored once — while the name means
  nothing to anyone without the vault secret. The regression test is written to
  read as what it is: **a blob's on-disk name equals no hash of its plaintext.**
- Truncating to 160 bits is safe precisely *because* the tag is keyed: an
  attacker cannot search for a colliding plaintext without `K_name`. The
  birthday bound is around 2^80 objects.
- Deduplication does **not** work across vaults, and that is intended.
- Because `K_name` lives below the backend seam, the M2 migration renames every
  object ([ADR-0007](0007-pluggable-sealing-backend-seam.md)). The alternative —
  carrying the age-derived `K_name` into the native backend — was rejected as
  permanently coupling the two backends.
- Objects are immutable, so deletion needs an explicit mechanism:
  [ADR-0015](0015-tombstones-and-blob-gc.md).
