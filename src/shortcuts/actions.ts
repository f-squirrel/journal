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

/// Chords CodeMirror's own keymap (`@codemirror/commands`'s `standardKeymap`,
/// `defaultKeymap` and `historyKeymap`, wired up in `MarkdownEditor`) or the
/// OS already give a meaning to. `setBinding` refuses to rebind an action onto
/// one of these, and a stored override naming one is dropped. The list is the
/// union of every platform's keymap (CodeMirror's `mac:` variants become
/// `Control+…`/`Alt+…` here), so a chord that is free on one platform but
/// taken on another is refused everywhere.
///
/// One default overlaps on purpose: `help.shortcuts`' `$mod+Slash` is
/// CodeMirror's `Mod-/` (`toggleComment`, which wraps markdown in `<!-- -->`
/// — rarely wanted in a journal), and the global listener wins it. It is left
/// out of this list so that chord can still be recorded back onto an action.
export const RESERVED: readonly string[] = [
  // Clipboard and window chords the OS owns.
  "$mod+KeyC",
  "$mod+KeyV",
  "$mod+KeyX",
  "$mod+KeyQ",
  "$mod+KeyW",
  // History.
  "$mod+KeyZ",
  "$mod+KeyY",
  "$mod+Shift+KeyZ",
  "$mod+KeyU",
  "$mod+Shift+KeyU",
  "Alt+KeyU",
  // Selection and editing.
  "$mod+KeyA",
  "$mod+KeyI",
  "$mod+KeyM",
  "$mod+Enter",
  "$mod+BracketLeft",
  "$mod+BracketRight",
  "$mod+Shift+KeyK",
  "$mod+Shift+Backslash",
  "$mod+Alt+Backslash",
  "$mod+Alt+ArrowUp",
  "$mod+Alt+ArrowDown",
  "$mod+Backspace",
  "$mod+Delete",
  "Alt+KeyL",
  "Alt+Shift+KeyA",
  "Alt+Shift+KeyM",
  "Alt+Backspace",
  "Alt+Delete",
  // Cursor movement, each with its Shift-extends-the-selection twin.
  ...["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].flatMap(
    (code) => [`$mod+${code}`, `$mod+Shift+${code}`],
  ),
  ...["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].flatMap((code) => [
    `Alt+${code}`,
    `Alt+Shift+${code}`,
    `Control+${code}`,
    `Control+Shift+${code}`,
  ]),
  // macOS's Emacs-style Ctrl chords (plain `Control`, not `$mod`, on Apple).
  ...["KeyB", "KeyF", "KeyP", "KeyN", "KeyA", "KeyE"].flatMap((code) => [
    `Control+${code}`,
    `Control+Shift+${code}`,
  ]),
  "Control+KeyD",
  "Control+KeyH",
  "Control+KeyK",
  "Control+KeyO",
  "Control+KeyT",
  "Control+KeyV",
  "Control+KeyL",
  "Control+Alt+KeyH",
];
