# 4. Tauri 2 with a React/TypeScript UI

## Status

Accepted — 2026-08-15 (ratified in the design doc §4.1, 2026-08-14)

## Context

The app must ship on Linux, macOS, Windows, iOS and Android (F1, F2) with a
Markdown editor and inline image rendering (F3, F6), while keeping every
security-relevant line in Rust (T3) and maximizing shared code (T1).

Candidates considered: Tauri 2, Dioxus, Flutter, and per-platform native UI.
Flutter and native UI both mean the editor is written once per stack or in a
language where our crypto core is a foreign function; Dioxus keeps everything
in Rust but has a far thinner ecosystem for the two hardest UI pieces we need —
a mature Markdown editor and image handling.

## Decision

Use **Tauri 2** as the shell — one project producing desktop, iOS and Android
builds — with a **React + TypeScript** frontend (Vite, CodeMirror 6 for the
editor) running in the platform WebView.

The WebView is treated as a *partially trusted rendering surface*, not as
part of the security core: strict CSP, no remote origins, no `eval`, devtools
disabled in release builds, filesystem and shell APIs not enabled, and the
`tauri.conf.json` allowlist reviewed line by line (M1 Task 12).

## Consequences

- One UI codebase across five platforms, and the web ecosystem supplies the
  editor and image handling we would otherwise write ourselves.
- A JavaScript layer now exists inside the app, and it will hold decrypted
  entry text — which is exactly why [ADR-0005](0005-ipc-boundary-and-lock-teardown.md)
  draws a hard line at the IPC boundary and makes locking a teardown.
- Per-platform WebView quirks (WKWebView, WebView2, WebKitGTK) become a real
  cost, and some of them are security-relevant: WebView response caching is
  what forces `Cache-Control: no-store` on the blob protocol handler.
- Linux builds pull in the GTK3/webkit2gtk stack, whose Rust bindings are
  unmaintained upstream — several `cargo deny` advisory ignores in `deny.toml`
  exist solely because Tauri 2 still targets GTK3
  ([ADR-0020](0020-toolchain-lints-and-supply-chain.md)).
- Images must not be handed to the WebView as base64 in IPC payloads; they are
  served through a custom `journal://blob/<name>` protocol handler so bytes
  stream from Rust without transiting JS.
