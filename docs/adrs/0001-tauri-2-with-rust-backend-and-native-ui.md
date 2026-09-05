# 0001. Use Tauri 2, with Rust for the backend and native frameworks for the UI

## Status

Accepted — 2026-09-05

## Decision

The app is built on Tauri 2. The backend runs in Rust and owns cloud sync
and cryptography (along with other native, security- or performance-
sensitive concerns). The UI layer is built with native/web-view frontend
frameworks running inside Tauri's webview, communicating with the Rust
backend over Tauri's command/event bridge.

## Reasons

- Tauri produces small, native binaries across desktop platforms while
  reusing a single web-based UI codebase, avoiding a separate native UI
  implementation per platform.
- Rust is well-suited to the backend's responsibilities: cloud sync and
  cryptography benefit from Rust's memory safety, strong type system, and
  mature crypto/networking ecosystem.
- Keeping cryptography and sync logic in the Rust backend (rather than the
  webview/JS layer) keeps sensitive operations out of the less-trusted
  frontend process.

## Alternatives considered

- **Electron**: rejected — ships a full Chromium + Node runtime per app,
  resulting in much larger binaries and higher memory use than Tauri's
  system-webview approach.
- **Fully native UI per platform (Swift/Kotlin/etc.)**: rejected for now —
  would require maintaining separate UI implementations per platform,
  multiplying UI development and maintenance effort.
- **Backend in another language (Node/Go)**: rejected — Rust's safety
  guarantees and ecosystem are a better fit for cryptography and sync code,
  and Tauri's backend is Rust-native.
