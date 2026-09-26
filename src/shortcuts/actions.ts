/// The single source of truth for every keyboard shortcut: its id, its
/// display label and its default binding. Nothing else in the app spells out
/// a key string directly — components attach handlers by `ActionId`, and
/// `ShortcutsDialog` (phase A) and the rebind UI (phase B) both read this
/// table rather than duplicating it.
///
/// Binding syntax is tinykeys': `$mod` (Cmd on Apple, Ctrl elsewhere), then
/// `Control`, `Alt`, `Shift` in that order, then an `event.code` name (e.g.
/// `KeyN`, never the character `n`) — codes are what makes a binding match
/// regardless of keyboard layout. `null` means the action has no binding.
export const ACTIONS = {
  "entry.new": { label: "New entry", default: "$mod+KeyN" },
  "search.focus": { label: "Search entries", default: "$mod+KeyF" },
  "entry.toggleMode": { label: "Switch edit / read", default: "$mod+KeyE" },
  "vault.lock": { label: "Lock journal", default: "$mod+KeyL" },
  "view.calendar": { label: "Calendar", default: "$mod+Shift+KeyC" },
  "help.shortcuts": { label: "Keyboard shortcuts", default: "$mod+Slash" },
} as const satisfies Record<string, { label: string; default: string | null }>;

export type ActionId = keyof typeof ACTIONS;

/// Chords CodeMirror's own keymap (`@codemirror/commands`'s `defaultKeymap` +
/// `historyKeymap`, wired up in `MarkdownEditor`) or the OS already give a
/// meaning to. Phase B (`setBinding`) refuses to rebind an action onto one of
/// these; phase A's fixed defaults above already avoid them, so WP1 only
/// needs the list to exist for that later work.
export const RESERVED: readonly string[] = [
  "$mod+KeyZ",
  "$mod+KeyY",
  "$mod+Shift+KeyZ",
  "$mod+KeyA",
  "$mod+KeyC",
  "$mod+KeyV",
  "$mod+KeyX",
  "$mod+KeyQ",
  "$mod+KeyW",
  "$mod+KeyU",
  "$mod+KeyI",
  "$mod+BracketLeft",
  "$mod+BracketRight",
  "$mod+Enter",
  "$mod+Shift+KeyK",
];
