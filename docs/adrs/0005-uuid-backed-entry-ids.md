# 0005. Use UUID-backed entry ids generated on the frontend

## Status

Accepted — 2026-09-07

## Decision

Entry ids are a `journal-entry::Id` newtype wrapping `uuid::Uuid`
(serialized transparently as a string), generated on the frontend via
`crypto.randomUUID()` when a new entry is created. The Rust backend
(`journal-storage`, the Tauri commands) never generates an id — it only
stores and looks entries up by the `Id` it's given.

## Reasons

Ids were previously `String(Date.now())`, generated in `EditorPage.tsx`.
That has a real collision window: two new entries saved within the same
millisecond (e.g. a fast test loop, or — once sync across devices lands —
two devices creating entries around the same time) would silently
overwrite each other, since storage keys files by id. UUIDs make that
collision probability negligible without needing any coordination, and
`uuid`'s serde support makes the wire format a plain JSON string, so the
frontend's `id: string` type didn't need to change.

Generation stays on the frontend rather than moving to the backend because
the backend has no id-issuing authority of its own — it's a thin
per-file store (see [0003](0003-file-per-entry-json-storage.md)) — and the
browser fallback path (`npm run dev`, no Tauri) has no backend to ask at
all.

## Alternatives considered

- **Keep `Date.now()`-derived strings**: simplest, no new dependency, but
  carries the collision risk above and leaks creation time into the id.
- **Backend-generated id** (e.g. `save_entry` assigns one before writing):
  would need `save_entry` to return the assigned id, and doesn't work in
  the browser-fallback (no-Tauri) mode where there is no backend.
- **A shorter id scheme (e.g. nanoid)**: also a valid choice, but UUID v4
  is a well-known standard with first-class serde support in Rust and
  built-in generation (`crypto.randomUUID()`) in browsers, so no extra
  frontend dependency was needed either.
