# 0015. Use `tinykeys` behind an action registry for keyboard shortcuts

## Status

Accepted — 2026-09-26

## Decision

Keyboard shortcuts are declared once, in `src/shortcuts/actions.ts`: an
`ACTIONS` registry of action id, label and default chord. Components attach a
handler by action id with `useShortcut(id, handler)` and never mention a key.
`ShortcutsProvider` (in `App.tsx`, around the router) installs one
capture-phase `window` listener through `tinykeys`, built from the effective
bindings.

- **Bindings are `event.code` names, not characters** — `$mod+KeyN`, not
  `$mod+n` — so they fire on any keyboard layout. `$mod` is Cmd on Apple
  platforms and Ctrl elsewhere.
- **Every binding needs a modifier** (`$mod`, `Control` or `Alt`). That is
  why shortcuts can fire inside text fields and the editor with no "is focus
  editable?" rule; `tinykeys`' default input filter is turned off.
- **Capture beats CodeMirror.** App chords win over the editor's keymap;
  chords the editor or the OS own are protected by a `RESERVED` list that
  rebinding refuses, not by listener order.
- **Nothing fires while a dialog is open.**
- **Overrides are per device**, in `localStorage["journal.shortcuts"]`, edited
  in the "Keyboard shortcuts" dialog. A conflicting chord is refused with the
  other action named; nothing is reassigned automatically.

## Reasons

The app needs shortcuts to be rebindable, which means their bindings have to
live in one place rather than at each call site. `tinykeys` is ~650 B, has no
dependencies, does `$mod` and code-or-key matching, and takes a capture
option — everything the provider needs and nothing it doesn't.

Matching on characters is what broke the old hand-rolled Ctrl-F: under a
Russian layout `event.key` is `"а"`, so it never fired. Codes don't have that
problem.

Keyboard preferences belong to a device, like the colour mode; they have no
place in the encrypted journal or in sync.

## Alternatives considered

**`react-hotkeys-hook`.** One hook per component with the key string at the
call site — the opposite of one registry. Its scopes add nothing the
dialog rule doesn't.

**A hand-rolled matcher.** About 80 lines of `$mod` detection and
code-vs-key edge cases, owned for no gain over `tinykeys`.

**`hotkeys-js` / `mousetrap`.** Older and character-based: the same layout
bug.

**Tauri global shortcuts or native menu accelerators.** Global shortcuts fire
while the app is unfocused and would have to cope with a locked vault; menu
accelerators exist only where there is a menu bar. Both are desktop-only.
Accelerators could later be built from the same registry.

**Synced overrides.** Rejected above.
