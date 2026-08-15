# 19. Prove crypto and storage before adding sync and mobile

## Status

Accepted — 2026-08-15 (design doc §8; M1 plan, 2026-08-14)

## Context

The obvious first release of a cross-platform journaling app is a thin vertical
slice: an editor, a file on disk, a Dropbox account, running on a phone.
Everything is demonstrable early and the encryption gets hardened later.

That order optimizes for the wrong thing. The on-disk format and the key
hierarchy are the parts that are expensive to change once real vaults exist —
every mistake becomes a migration — while an editor, a sync provider, or a
mobile shell can be rewritten at any time with no consequence for user data.

## Decision

Sequence the milestones so the expensive-to-change layers are proven first:

| Milestone | Scope |
|---|---|
| **M1** | Rust core + encrypted local vault (`age` backend) + Markdown editing + images + plaintext export, **desktop only**. No sync, no mobile. |
| **M2** | Sync engine + Dropbox provider, conflict handling. Plus the native Argon2id backend, which satisfies F4 and migrates M1 vaults. |
| **M3** | iOS build: Keychain-wrapped key slot, biometric unlock, mobile editor UX. |
| **M4** | Android build (Keystore), plus a second sync provider to prove the trait. |
| **M5** | Hardening: external crypto review, reproducible builds, signed releases. |

M1 is deliberately narrow, and its done-criterion is a security property rather
than a feature list: a user can create a vault, write an entry with an image,
quit, reopen, unlock, and see it — and **every byte the app wrote anywhere on
disk** is unreadable without the passphrase or recovery code. "Anywhere" means
the whole application data directory, including the search index, temp files
and WebView caches; a leak sweep with a high-entropy canary proves it.

## Consequences

- No sync and no mobile until the format is stable, which is a genuinely
  unglamorous first release.
- The format-critical work happens while there are zero real vaults to migrate,
  which is the only time it is cheap.
- M5's external review has a stable target rather than a moving one.
- Deferred items are tracked explicitly rather than forgotten: the M1 plan's
  "Deferred to M2" section itemises every piece of the native backend, each
  with a byte-level home in the crypto spec already.
- Releases before M5 are **unsigned**, so Gatekeeper and SmartScreen will warn.
  Release notes must say so and publish checksums instead of pretending
  otherwise.
