# 20. Pin the toolchain, deny warnings in CI, and audit dependencies

## Status

Accepted — 2026-08-15 (M1 Task 1, implemented 2026-08-14)

## Context

T2 makes security a first-class constraint, and in a Rust project a
non-trivial share of that is mechanical: lint classes that catch real bugs,
panicking constructs that have no place in code handling a user's only copy of
their journal, and a dependency tree that ships in a binary and therefore needs
license and advisory review. Left to per-developer discretion, all three drift.

## Decision

- **Pinned toolchain.** `rust-toolchain.toml` pins the stable version
  (currently 1.97.1) with `rustfmt` and `clippy`, so every machine and CI job
  lint identically. `rustfmt.toml` settles formatting.
- **Lints declared once, denied in CI.** `[workspace.lints.clippy]` in the root
  `Cargo.toml` sets `all` and `pedantic` to warn, plus `unwrap_used` and
  `expect_used`; every crate inherits them. CI turns warnings into errors via
  `RUSTFLAGS: -D warnings`. `clippy.toml` configures lint *thresholds* only —
  it cannot deny warnings, and documentation says so because the distinction is
  routinely misremembered.
- **`Cargo.lock` is committed.** This workspace ships binaries, so builds must
  be reproducible.
- **`cargo deny check`** runs in CI over advisories, licenses and bans, across
  the four target triples we build for. Advisory ignores are individually
  justified in `deny.toml` with the reason and the condition for revisiting —
  most of them are the unmaintained GTK3/`unic-*` crates that Tauri 2's Linux
  backend still requires ([ADR-0004](0004-tauri-2-and-react-for-the-ui.md)).
- **CI runs fmt, clippy, test, deny, and a release build on Linux, macOS and
  Windows** on every push to `main` and every pull request. The frontend is
  built in the same job; `oxlint` lints the TypeScript.
- **Release profile** uses `lto = true` and `codegen-units = 1`.

## Consequences

- `unwrap`/`expect` require a deliberate, visible override, which is the
  correct friction for code that must not panic on a corrupt vault or a full
  disk.
- Pedantic clippy produces some noise on otherwise fine code. Accepted: a
  denied warning is a conversation at review time, not a broken build later.
- A new advisory against an unavoidable transitive dependency can turn CI red
  through no change of ours. The ignore list absorbs that, with a written
  reason each time so the list is auditable rather than a silencer.
- The three-platform build matrix catches platform-specific breakage on the PR
  that causes it, at the cost of CI minutes.
- Toolchain bumps are an explicit, reviewable commit.
