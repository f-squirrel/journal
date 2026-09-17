# 0011. Set a Content-Security-Policy, and stop exposing `window.__TAURI__`

## Status

Accepted — 2026-09-17

## Decision

Two changes in `src-tauri/tauri.conf.json`, replacing the template
defaults:

- `withGlobalTauri: false`. `invoke` is no longer reachable from page
  scripts as `window.__TAURI__.core.invoke`; the app reaches it through
  `@tauri-apps/api` (`src/api/entries.ts`), which is unaffected.
- `security.csp` set rather than `null`:

  ```text
  default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
  img-src 'self' data:; font-src 'self';
  connect-src 'self' ipc: http://ipc.localhost;
  object-src 'none'; base-uri 'self'
  ```

## Reasons

The app's premise is that entries are unreadable at rest. With no policy
at all, any script that reaches the webview — a compromised transitive
frontend dependency, or a `rehype-raw` added to the markdown pipeline
later — can call `invoke("list_entries")` and `invoke("get_entry")` and
POST the decrypted result anywhere, with nothing in `connect-src` to stop
it. There is no live exploit today: `react-markdown` (^10.1.0) renders no
raw HTML and its default `urlTransform` strips dangerous protocols, and
`rehype-raw` appears nowhere in the tree. This is defence-in-depth, taken
while the app is small enough that the policy is cheap to get right.

`withGlobalTauri` is the same argument one layer up: the global is a
second, ambient path to the same commands, and nothing uses it.

The directives that are not obvious:

- **`style-src 'unsafe-inline'`** is required by emotion, MUI's style
  engine, which injects `<style>` elements at runtime. It cannot be
  replaced by a nonce without configuring an emotion cache, and Tauri's
  own nonce substitution (`manager/mod.rs`) only rewrites nonce tokens
  that appear in the HTML — of which the app's `index.html` has none.
  Do not remove it without replacing MUI's style setup.
- **`connect-src ipc: http://ipc.localhost`** is how `invoke` travels.
  Tauri's IPC script `fetch`es `convertFileSrc(cmd, 'ipc')`, which is
  `ipc://localhost/<cmd>` on Linux, macOS and iOS, and
  `http://ipc.localhost/<cmd>` on Windows and Android (`http`, not
  `https`, because `useHttpsScheme` is left at its `false` default). A
  blocked IPC fetch does not fail loudly — it falls back to the
  `postMessage` interface with a console warning — so an over-tight
  `connect-src` would show up as a warning and a slower path rather than
  a broken app.
- **No `asset:`** in `img-src`, because the asset protocol is not enabled
  in the config. Fonts are bundled (`@fontsource/roboto` via
  `src/index.css`, emitted under `/assets/`), so `'self'` covers them;
  `data:` is there for inline images in entry markdown. Remote images in
  an entry will not load, which for a local-first journal is a privacy
  gain rather than a regression — revisit that line, not the policy as a
  whole, if it ever needs to change.

## Alternatives considered

- **Leave `csp: null` and rely on the sanitizer.** The status quo, and
  defensible as long as `react-markdown` is the only thing rendering
  user content and nobody adds `rehype-raw`. Rejected because the cost of
  being wrong is the whole journal exfiltrated in plaintext, and the
  guard is one config line — a policy set now is also much easier to
  tighten than one retrofitted over a grown frontend.
- **A stricter `style-src` with an emotion nonce.** The right end state,
  and it would remove the one `'unsafe-inline'` in the policy. Rejected
  for now as a frontend change (an emotion `CacheProvider` wired to a
  nonce Tauri generates per page load) rather than a configuration one;
  it belongs in its own change, not bundled with turning the policy on.
- **Keep `withGlobalTauri: true` for debugging convenience.** Rejected:
  the app never used it, and a console that can call `invoke` is exactly
  the capability the CSP above is there to contain.

## Verification

The policy only takes effect where Tauri serves the frontend itself. In
development the webview navigates to `devUrl` (Vite), so `make run`,
`npm run dev` and the Playwright suite apply no CSP at all and prove
nothing about it — they are guards against breaking everything else.
Checking the policy means a packaged build (or a dev build with `devUrl`
unset, which makes Tauri serve `dist/` over `tauri://localhost`):
confirm the theme and fonts render, an entry opens, a save round-trips,
and the webview console reports no violations.
