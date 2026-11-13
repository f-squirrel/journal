# Assessment: native Swift UI for iOS

Status: research note, not a decision (no ADR). Date: 2026-11-13.

Question: what would we gain by replacing the React/MUI UI on iOS with a
native Swift UI, keeping the Rust backend?

## "Tauri for backend" does not carry over as-is

A Tauri command is a bridge between the webview and Rust (`invoke`,
`Channel`, events). SwiftUI cannot call `#[tauri::command]` functions.
Options:

1. **Reuse the crates, drop the Tauri runtime on iOS.** `vault`, `key`,
   `sync-engine`, `settings`, `api` and the rest are plain synchronous
   workspace crates. Build them as a staticlib and expose them to Swift with
   UniFFI (or swift-bridge) through a new `journal-ffi` crate. `src-tauri`
   is ~20 commands of glue, so this is a second glue layer. Anything in
   `src-tauri` that is not pure glue (OAuth loopback, `blocking` helpers)
   would move into crates. Desktop and Android stay on Tauri.
2. **Hybrid inside Tauri** via the Tauri Swift plugin API (native views or
   controllers). Awkward and thinly documented for full-screen UI; carries
   both stacks.

Option 1 is the realistic one. ADR 0001 already anticipated native UI.

The Tauri mobile plugin and UniFFI details above come from general
knowledge and were not checked against current docs.

## Pros

- **Better text editing.** `UITextView` with TextKit 2 replaces CodeMirror in
  a WKWebView: no keyboard/viewport resizing problems, selection handle
  quirks, or autocorrect/dictation oddities. The biggest gain for a
  journaling app.
- **Native feel.** Navigation, swipe-back, sheets, haptics, context menus,
  Dynamic Type, VoiceOver.
- **Images.** The ~16.7 MP canvas cap (known-issues) goes away. HEIC, AVIF
  and TIFF decode natively, so import no longer fails where the webview
  cannot decode. ImageIO makes the thumbnails.
- **Platform features.** Share extension, widgets, App Intents/Shortcuts,
  Face ID unlock tied to the Keychain, Files integration.
- **Lifecycle reliability.** Idle lock and the autosave flush hook into real
  app lifecycle callbacks, not a suspended JS context.
- **Small performance gain.** No webview cold start, less memory. Decryption
  is in Rust either way, so the gain is small.
- **Crate layering is ready.** Backend crates are already separate and
  synchronous.

## Cons

- **Large rewrite.** ~9.5k lines of TypeScript (of which ~1.3k is the dev
  fallback): `EntryPane`, `AppStateContext` (optimistic updates, rollback),
  `useAutosave` (conditional save, conflicted copies), `LoginPage`,
  Settings, Calendar, `HistoryDialog`, `ImageViewer`, the markdown renderer
  and editor. Inline lazily-decrypted images in TextKit are hard.
- **Two UIs to maintain.** Desktop and Android stay on React; every feature
  is built twice; ts-rs and UniFFI types must be kept in sync.
- **Test coverage drops.** Playwright against the browser fallback is the
  whole UI safety net and does not apply to Swift (XCTest/XCUITest instead).
- **Mac dependency.** Swift needs macOS and Xcode; local iteration on Linux
  goes through a Mac or CI.
- **Convention rework.** Colour schemes, the shortcut registry, and the rem
  and text-size rules are redone natively.
- **Moving target.** Alpha with no users and a UI design still changing
  (ADRs 0015 to 0019 are recent). Waiting costs little.

## Cheaper alternatives

- **Fix the editor only.** Test on a real device whether the CodeMirror and
  keyboard behaviour is tolerable before concluding the whole UI must change.
- **Native pieces as Tauri mobile plugins.** Face ID/Keychain unlock, a share
  extension, and a PHPicker photo picker (also solves HEIC import) are small
  Swift plugins and need no UI rewrite.
- **Native editor view as a plugin.** Possible but fiddly; a one-day
  experiment if the editor is the real pain.

## Recommendation

Not now. If the pain is the editor and iOS integrations, do the plugin route
first (days, versus weeks to months for a rewrite). If iOS-first polish is
worth owning a second UI, go with option 1: keep the crates, add
`journal-ffi` (UniFFI) over `vault`/`sync-engine`/`settings`, move
non-glue code out of `src-tauri`, then build SwiftUI screens one at a time,
starting with the entry editor.
