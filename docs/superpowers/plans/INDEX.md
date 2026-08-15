# Plan Index

Every plan gets a row here when it's created, and the row is kept current
as work moves. This table is the fastest way to see project state without
opening every plan file. Newest first.

| Date | Plan | Status | Branch / PR | Notes |
|------|------|--------|-------------|-------|
| 2026-08-14 | [M1 — Encrypted local vault](2026-08-14-m1-encrypted-local-vault.md) | In Progress | `claude/task-4-plan-wnzf3r` | Desktop only. Designs: [journaling app](../specs/2026-08-14-journaling-app-design.md), [crypto](../specs/2026-08-14-crypto-design.md). Plan and design both revised 2026-08-14 after review. Tasks 1 (workspace skeleton), 2 (crypto spec), 3 (`journal-model` domain types), and 4 (backend seam + object naming) done. Spec §9's naming vectors (item 2, both `K_name` rows) landed with Task 4 rather than Task 5; the remaining vectors are still Task 5's. Crypto spec revised 2026-08-14: M1 seals with `age` behind a swappable backend, so **F4 (Argon2) is deferred to M2** — the Argon2id scheme stays fully specified (crypto spec §6) and is itemised under the plan's "Deferred to M2". |
