# plans

Implementation plans from the [superpowers workflow](../README.md): each
turns a spec (or a smaller idea that doesn't need one) into a checklist of
bite-sized tasks with exact file paths and verification steps. Checkboxes
are the source of truth for task-level progress within a plan.

Naming: `YYYY-MM-DD-<feature-name>.md`.

## Task conventions

Every task checklist includes a final step to review docs (use `/docs-review-guide`)
after implementation is done. This ensures CLAUDE.md, design specs, and other
documentation stay in sync with code changes as work progresses.

## Index

[`INDEX.md`](INDEX.md) is the single glance-able view of every plan's
status (`Draft`, `In Progress`, `Blocked`, `Done`, `Abandoned`) — check it
before opening individual plan files to see what's actually current.

## Current plans

- [`2026-08-14-m1-encrypted-local-vault.md`](2026-08-14-m1-encrypted-local-vault.md)
  — M1: a desktop-only app that creates a passphrase-protected vault,
  writes and renders Markdown entries with embedded images, and stores
  everything as ciphertext on disk. In progress; see `INDEX.md` for current
  task status.

Plan files are immutable history once done — a finished plan doesn't get
rewritten for later scope changes; a new plan is written and linked back
instead.
