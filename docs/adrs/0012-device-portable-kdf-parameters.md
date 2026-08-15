# 12. KDF parameters are bounded by the weakest device, not by the strongest

## Status

Accepted — 2026-08-15 (design doc §5; crypto spec §5.2, §6.4; ceiling lowered
to `log_n = 18` in the spec's third revision, 2026-08-14)

## Context

The usual advice for a password KDF is "calibrate to about a second on the
device doing the encryption, and set a floor so weak devices cannot pick weak
parameters." For a vault that is created on a desktop and later opened on a
phone, that advice produces a specific disaster: parameters tuned to a
workstation, and a vault the user cannot open on the device they are holding at
the moment they need it.

`age` makes this concrete rather than theoretical. It calibrates scrypt to the
encrypting device *and*, on decryption, **rejects** — `DecryptError::ExcessiveWork`
— any file whose work factor exceeds the decrypting device's own target by more
than 4. A desktop-created vault would be not merely slow on a phone but
refused.

## Decision

The binding constraint is a **ceiling**, not a floor, and the ceiling is set by
the weakest device class the vault must ever open on: provisionally a 64-bit
ARM phone, 2019 or later, with ≥3 GiB RAM, unlocking within 3 seconds.

- **age backend (M1):** work factors are **pinned, never calibrated**.
  `set_work_factor(17)` on write (N = 2^17, r = 8 → 128 MiB) and
  `set_max_work_factor(18)` on read (accept up to 256 MiB). The read cap
  matters because `log_n` is read *from the file* and is therefore
  attacker-chosen: a hostile slot file gets to name the allocation. One
  doubling of headroom is enough for a future client that raises the pinned
  value; anything beyond that is a format change.
- **Native backend (M2):** Argon2id with `FLOOR = 64 MiB / t=3 / p=4`
  (RFC 9106 §4) and `CEILING = 256 MiB / t=10 / p=4`, both enforced on read as
  well as write. Calibration runs at vault creation and at every passphrase
  change, on a throwaway password, targeting 1000 ms — raising **memory before
  iterations**, because memory is what an attacker must provision per guess
  while iterations cost attacker and defender alike. The ceiling may be raised
  for new vaults at any time and lowered **only** for new vaults; lowering it
  cannot retroactively fix a vault already calibrated above it.

Both backends land on the same 256 MiB ceiling, so a device that can open a
native vault can open an age one.

## Consequences

- A vault created anywhere opens everywhere, which is the property that makes
  the recovery path usable under stress.
- Desktop users get weaker parameters than their hardware could afford. That is
  the cost of portability, and it is stated rather than hidden.
- The weakest-device class is **provisional** — measuring it on real hardware
  once a phone build exists is an open question in design doc §9.
- Argon2id's `p` is a *format parameter*, not a thread count: it changes the
  function's output and must be reproduced exactly, though an implementation
  may run those lanes on fewer threads.
- Calibration is slow enough to look like a hang, so the UI must show progress
  during vault creation and passphrase change (M1 Task 14).
