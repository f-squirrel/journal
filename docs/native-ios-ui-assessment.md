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

See "Validation against docs" at the end: the plugin and UniFFI details were
checked, and several claims below were corrected there.

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

## Validation against docs (2026-11-13)

Checked with web searches against official docs and trackers. Sources are
listed at the end. Search results are summaries, not a full read of each page.

### Confirmed

- **Swift plugins exist and are the supported native route.** A Tauri iOS
  plugin is a Swift class extending `Plugin`, packaged as a Swift package;
  commands are `@objc` methods taking an `Invoke`. So the "native pieces as
  plugins" alternative is the documented path.
- **UniFFI fits the crate layout.** It generates Swift bindings from a Rust
  staticlib, with an Xcode/xcframework workflow documented. It supports
  `async` by mapping to Swift async/await via a foreign executor. Because the
  crates are synchronous, the FFI surface can be plain sync functions called
  from a background `Task`; no Rust async runtime is needed. Caveats from the
  docs: no cancellation support, and crates that start their own runtime
  (e.g. reqwest starts tokio) need a deliberate bridge. Check `dropbox` and
  `remote` for this before committing.
- **iOS WKWebView keyboard/viewport problems are real.** WebKit bugs
  (viewport offset while the keyboard is open, content left offscreen after
  dismissal, `focus()` outside a user gesture not raising the keyboard) and a
  Tauri-specific write-up (viewport does not resize with the keyboard,
  scroll jumps on focus) match what I claimed. The Tauri write-up is one
  author's notes, not official docs; whether these are fixed in current iOS
  was not checked.

### Corrected or weakened

- **The editor gain is smaller than stated.** CodeMirror 6 had an iOS
  missing-selection-handles problem, but `@codemirror/view` 6.43.0 (May 2026)
  now draws its own handles on iOS, and this repo already resolves
  6.43.12. Other reports found (autocorrect on Enter, read-only keyboard,
  Japanese input) are older or unconfirmed for CM6. The remaining keyboard and
  viewport issues are WKWebView problems, not CodeMirror ones. The app has no
  `visualViewport`, safe-area or keyboard handling today, so those are
  likely untested workarounds, not proven failures. Test on a device first.
- **Face ID is not a custom plugin job.** An official
  `tauri-plugin-biometric` exists (iOS and Android; needs
  `NSFaceIDUsageDescription`). Note it only prompts for authentication. It
  does not by itself bind the vault key to the Keychain or Secure Enclave;
  that is a separate design question.
- **A share extension is not "small".** Receiving shares on iOS needs a Share
  Extension target in Xcode writing into an App Group container, with Rust
  reading from it. Community plugins exist but are not official, and the
  closest full example (a Tauri app with an App Group database) moves its
  storage into the shared container. Here that would mean relocating the
  vault and device-settings location, which touches `fs-store` and the
  no-migration rule.
- **PHPicker/HEIC plugin: unverified.** No Tauri-specific PHPicker plugin
  turned up. PHPicker is the standard native picker and avoids the
  canvas-decode limit only if it hands back original bytes (HEIC picking also
  has known simulator limits). Treat "small Swift plugin" as an estimate to
  prototype, not a fact.
- **Native view embedded in the Tauri webview: no documentation found.** The
  mobile plugin guide covers commands and events, not attaching a `UIView` or
  view controller. The hybrid option (and the "native editor as a plugin"
  experiment) should be treated as unsupported territory.

### Net effect on the recommendation

Unchanged in direction, firmer in order: (1) test the current editor on a real
iPhone and add visualViewport/safe-area handling if needed; (2) adopt
`tauri-plugin-biometric` for unlock; (3) prototype a photo picker plugin;
(4) only then consider the share extension or a full native UI. If a native UI
does happen, UniFFI over the existing synchronous crates is well supported and
lower risk than assumed.

### Sources

- Tauri, Mobile Plugin Development: https://v2.tauri.app/develop/plugins/develop-mobile/
- Tauri, Biometric plugin: https://v2.tauri.app/plugin/biometric/
- UniFFI, Integrating with Xcode: https://mozilla.github.io/uniffi-rs/latest/swift/xcode.html
- UniFFI, async overview: https://mozilla.github.io/uniffi-rs/latest/internals/async-overview.html
- WKWebView gotchas in Tauri iOS (community): https://takazudo-zudo-tauri.pages.dev/pj/zudo-tauri/docs/mobile/wkwebview-gotchas
- WebKit bug 195884 (focus without gesture): https://bugs.webkit.org/show_bug.cgi?id=195884
- WebKit bug 292603 (viewport offset with keyboard): https://bugs.webkit.org/show_bug.cgi?id=292603
- WebKit bug 192564 (content offscreen after keyboard): https://bugs.webkit.org/show_bug.cgi?id=192564
- CodeMirror, iOS missing selection handles: https://discuss.codemirror.net/t/ios-safari-missing-selection-drag-handles/9679
- `@codemirror/view` changelog 6.43.0: https://cdn.jsdelivr.net/npm/@codemirror/view@6.43.0/CHANGELOG.md
- Share extension with App Group in a Tauri iOS app (Peek): https://tangled.org/burrito.space/peek/tree/main/docs/mobile.md
- tauri-plugin-mobile-sharetarget: https://docs.rs/crate/tauri-plugin-mobile-sharetarget/latest
