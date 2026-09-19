# 0008. Use tauri-plugin-log as the app's log sink

## Status

Accepted — 2026-09-09

Amended 2026-09-20: the targets are `Stderr` plus the plugin's log file
(`TargetKind::LogDir`), set with `targets([..])` so they replace the
plugin's defaults instead of adding to them. Adding `Stderr` to the default
`Stdout` printed every line twice. The file is kept so a desktop user, who
rarely starts the app from a terminal, still has a record of a failure.

## Decision

Route all backend logging through `tauri-plugin-log`, registered as
`logging_plugin()` in `src-tauri/src/lib.rs` and installed first in the
Tauri builder. Backend crates log with the `log` crate, not `tracing`.
The plugin is configured with a single `TargetKind::Stderr` target at
`LevelFilter::Info`.

## Reasons

The app had no subscriber installed at all, so `tracing::warn!` compiled
down to nothing — including the one call site that reports an entry
skipped because it would not decrypt (`journal_storage::list_entries`).
That made a silently-dropped entry indistinguishable from an empty vault.

`tauri-plugin-log` routes its `Stdout`/`Stderr` targets to `android_logger`
on Android and oslog on iOS. Since the project targets desktop, iOS and
Android, one registration covers all three; a plain stderr sink would have
left two of the three blind and booked a migration for later.

## Alternatives considered

- **`tracing-subscriber` with `env-filter`**: the smaller diff, and it
  reads `RUST_LOG` for free. Rejected because `fmt` writes to stderr, which
  goes nowhere a developer will see on Android or iOS. The cost of
  switching — that the plugin is a `log`-crate sink, not a `tracing` one —
  was a single line, because there was exactly one call site.
- **`tauri-plugin-log` plus the `tracing-log` bridge**: would have kept
  `tracing::warn!` at the call sites. Rejected as an extra dependency and a
  less obvious wiring path to buy back a macro spelling.
