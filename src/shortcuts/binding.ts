/// Helpers around the binding strings `actions.ts` declares: detecting the
/// platform, turning a keydown into a binding string, validating one and
/// formatting one for display.

const PURE_MODIFIER_KEYS = ["Shift", "Control", "Alt", "Meta"];

/// The same test `tinykeys` uses internally to decide what `$mod` means, so
/// this module's labels and matching never disagree with what actually fires.
/// See `node_modules/tinykeys/dist/tinykeys.mjs`: `MOD = /Mac|iPod|iPhone|iPad/
/// .test(PLATFORM)`, where `PLATFORM` is `navigator.platform` (or `""` outside
/// a browser).
export function isApple(): boolean {
  const platform = typeof navigator === "object" ? navigator.platform : "";
  return /Mac|iPod|iPhone|iPad/.test(platform);
}

/// Turns a physical keydown into a binding string in the canonical order
/// (`$mod`, `Control`, `Alt`, `Shift`, then `event.code`). Used by phase B's
/// recorder field.
///
/// `event.key` is ignored except to reject a bare modifier press (`Shift`,
/// `Control`, `Alt`, `Meta` on their own), which returns `null` so the
/// recorder keeps waiting for a real chord. Meta is `$mod` on Apple, Ctrl is
/// `$mod` elsewhere; Ctrl on Apple maps to the plain `Control` modifier,
/// since Apple keyboards have both.
export function bindingFromEvent(event: KeyboardEvent): string | null {
  if (PURE_MODIFIER_KEYS.includes(event.key)) return null;

  const apple = isApple();
  const parts: string[] = [];
  if (apple ? event.metaKey : event.ctrlKey) parts.push("$mod");
  if (apple && event.ctrlKey) parts.push("Control");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  parts.push(event.code);
  return parts.join("+");
}

/// True once `b` carries at least one of `$mod`, `Control` or `Alt`. Every
/// stored binding needs one: without it, a single letter would fire while the
/// user is typing, which would need an "is focus in an editable?" rule that
/// nothing else here wants.
export function hasModifier(b: string): boolean {
  const mods = b.split("+").slice(0, -1);
  return (
    mods.includes("$mod") || mods.includes("Control") || mods.includes("Alt")
  );
}

const CHORD_MODIFIERS = ["$mod", "Control", "Alt", "Shift"];

/// True when `b` has the shape this app writes: zero or more of the four
/// modifier tokens (each at most once), then a non-empty `event.code` name.
/// This is a syntax check only — it doesn't require a "real" modifier the way
/// `hasModifier` does, so `"Shift+KeyN"` and even bare `"KeyN"` both pass it.
/// Used to decide whether a `localStorage["journal.shortcuts"]` value is
/// worth trusting before it reaches `hasModifier` and the rest of
/// `setBinding`'s checks: that file is a preference a person (or a stale
/// version of this app) can leave malformed, not a format with its own
/// migration path.
export function isWellFormedBinding(b: string): boolean {
  const parts = b.split("+");
  const code = parts[parts.length - 1];
  if (code.length === 0) return false;
  const mods = parts.slice(0, -1);
  if (mods.some((m) => !CHORD_MODIFIERS.includes(m))) return false;
  return new Set(mods).size === mods.length;
}

/// A handful of `event.code` values that don't read as their own label.
const CODE_LABELS: Record<string, string> = {
  Slash: "/",
  Comma: ",",
  Period: ".",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Quote: "'",
  Semicolon: ";",
  Backquote: "`",
  Space: "Space",
  Enter: "Enter",
  Escape: "Esc",
  Tab: "Tab",
};

function keyLabel(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return CODE_LABELS[code] ?? code;
}

const APPLE_SYMBOLS: Record<string, string> = {
  Control: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  $mod: "⌘",
};

const OTHER_LABELS: Record<string, string> = {
  $mod: "Ctrl",
  Control: "Ctrl",
  Alt: "Alt",
  Shift: "Shift",
};

// Fixed display order, independent of storage order: the conventional Mac
// order is Control, Option, Shift, Command, and it reads just as naturally
// elsewhere.
const DISPLAY_ORDER = ["Control", "Alt", "Shift", "$mod"];

/// `"⌘N"` / `"⇧⌘C"` on Apple, `"Ctrl+N"` / `"Ctrl+Shift+C"` elsewhere. `""`
/// for `null` (an unbound action).
export function formatBinding(b: string | null): string {
  if (b === null) return "";
  const parts = b.split("+");
  const label = keyLabel(parts[parts.length - 1]);
  const mods = parts.slice(0, -1);
  const ordered = DISPLAY_ORDER.filter((m) => mods.includes(m));

  if (isApple()) {
    return [...ordered.map((m) => APPLE_SYMBOLS[m]), label].join("");
  }
  return [...ordered.map((m) => OTHER_LABELS[m]), label].join("+");
}

/// The `aria-keyshortcuts` value for `b` (`"Meta+N"` / `"Control+N"`), or
/// `undefined` for `null` so the attribute is omitted rather than empty.
export function ariaKeyshortcuts(b: string | null): string | undefined {
  if (b === null) return undefined;
  const parts = b.split("+");
  const key = keyLabel(parts[parts.length - 1]);
  const mods = parts
    .slice(0, -1)
    .map((m) => (m === "$mod" ? (isApple() ? "Meta" : "Control") : m));
  return [...mods, key].join("+");
}
