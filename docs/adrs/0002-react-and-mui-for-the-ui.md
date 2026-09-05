# 0002. Use React and MUI (v6) for the UI

## Status

Accepted — 2026-09-05

## Decision

The frontend is built with React (via `@vitejs/plugin-react`) and Material
UI (`@mui/material`), replacing the initial vanilla TypeScript + manual DOM
scaffold. Routing uses `react-router-dom` (`HashRouter`, for compatibility
with Tauri's static asset serving). MUI is pinned to the v6 line rather than
the current `latest` v9 release.

## Reasons

- The app needs a professional, standard Material Design look without
  hand-building components (forms, app bar, dialogs, buttons) from scratch.
- React + MUI is the most mature Material Design implementation available,
  with the largest ecosystem for things this app will need soon (markdown
  rendering, form handling, editors).
- MUI v9 (the `latest` npm tag at migration time) produced numerous
  TypeScript overload-resolution errors on core components (`Typography`,
  `Stack`) under this project's TypeScript/React 19 combination — common
  props like `color`, `textAlign`, and `fontWeight` failed to type-check.
  MUI v6 is a stable, widely-adopted release with mature types and installed
  cleanly with a passing `tsc --noEmit`.

## Alternatives considered

- **Stay vanilla TypeScript + hand-rolled CSS**: rejected — becomes painful
  once real Material components, form validation, and a markdown/code editor
  are needed.
- **Svelte + a Material Web Components wrapper**: lighter weight, but the
  Material ecosystem there is much thinner; more "professional" polish would
  need to be hand-built.
- **MUI v9 (latest)**: rejected for now due to the type-checking breakage
  described above; can be revisited once the v9 typings stabilize.
