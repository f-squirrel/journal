# 5. Keys never cross the IPC boundary, and locking is a teardown

## Status

Accepted — 2026-08-15 (ratified in the design doc §4, 2026-08-14; crypto spec
§8)

## Context

[ADR-0004](0004-tauri-2-and-react-for-the-ui.md) puts a JavaScript runtime
inside an end-to-end encrypted app. The UI must hold decrypted entry text in
order to render it, and JavaScript memory cannot be zeroized: there is no
`zeroize` for a JS string, no guarantee about when the heap is collected, and
no way to prove a copy did not survive. Any design that pretends otherwise —
"null out the variables on lock" — is theatre.

So the question is not whether plaintext reaches the WebView (it must) but what
else does, and what happens at lock.

## Decision

Two rules, both hard:

1. **Keys and plaintext-at-rest never cross the Tauri IPC boundary.** No
   command returns key material, key-slot bytes, or raw vault file paths. The
   passphrase enters Rust and never comes back. The UI receives decrypted entry
   text — and nothing else. Images are served through the
   `journal://blob/<name>` protocol handler rather than as base64 in an IPC
   payload. M1 Task 11 asserts this with a test that every command's return
   type is on an explicit allow-list of types known to carry no secrets.
2. **Locking is a teardown, not a cleanup.** On lock (manual or after the
   inactivity timeout, default 15 minutes), Rust zeroizes the keys and drops
   the derived index, and the shell **reloads the WebView** to a blank unlock
   screen so the old heap is discarded wholesale rather than nulled out
   variable by variable.

## Consequences

- The one place plaintext is unavoidable is bounded and short-lived, and the
  disposal mechanism matches the material: `zeroize` in Rust, wholesale
  destruction in JS.
- Unsaved UI state does not survive an auto-lock. That is a real usability
  cost and the app must not lose user work silently — saves and failure states
  are explicit (M1 Tasks 13, 14).
- The rule bans convenient shortcuts permanently: a "debug: dump vault path"
  command, a base64 image payload, or a key returned for a test helper all
  violate it. This is codified in `CLAUDE.md` so it survives contributors who
  never read this ADR.
- "Plaintext in the WebView heap while unlocked" stays on the non-defense list
  of [ADR-0002](0002-local-first-zero-knowledge-encryption.md) — this decision
  bounds the exposure, it does not remove it.
