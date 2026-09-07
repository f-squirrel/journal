# 0004. Use Playwright for UI testing

## Status

Accepted — 2026-09-07

## Decision

Use Playwright (`@playwright/test`) for browser-driven UI tests, in `e2e/`,
run via `npm run test:e2e`. `playwright.config.ts` starts the Vite dev server
automatically and points the tests at it. Since these run against
`npm run dev` rather than the packaged Tauri app, `invoke` isn't available
and the app falls back to its seeded in-memory data (see
`context/AppStateContext.tsx`) — the tests exercise frontend logic and
navigation, not the Rust storage backend.

## Reasons

The project had no test suite at all; Playwright is a well-supported way to
drive real browser interactions (clicks, form fills, navigation) and assert
on rendered output, catching regressions in routing, state management, and
component wiring that `tsc` alone can't.

## Alternatives considered

- **Cypress**: comparable capability, but Playwright's built-in
  `webServer` config (auto-starting/stopping the dev server for tests) and
  multi-browser support made it a better fit with less setup.
- **tauri-driver / WebDriver against the packaged app**: would exercise the
  real Tauri backend (`invoke`, Rust storage) instead of the browser
  fallback, but requires a built app binary and platform-specific WebDriver
  setup. Left for later if backend-integrated UI testing becomes necessary.
