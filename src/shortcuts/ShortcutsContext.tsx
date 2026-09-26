import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { tinykeys } from "tinykeys";
import { ACTIONS, RESERVED, type ActionId } from "./actions";
import { formatBinding, hasModifier, isWellFormedBinding } from "./binding";

type Handler = () => void;

/// Per-device overrides of `ACTIONS`' defaults. A missing id means "use the
/// default"; a present id with value `null` means "unbound"; a present id
/// with a string means that chord instead.
type Overrides = Partial<Record<ActionId, string | null>>;

const STORAGE_KEY = "journal.shortcuts";

interface ShortcutsContextValue {
  bindingFor(id: ActionId): string | null;
  labelFor(id: ActionId): string;
  register(id: ActionId, handler: Handler): void;
  unregister(id: ActionId, handler: Handler): void;
  setBinding(id: ActionId, b: string | null): string | null;
  resetBinding(id: ActionId): void;
  resetAll(): void;
}

const ShortcutsContext = createContext<ShortcutsContextValue | null>(null);

/// Reads `localStorage["journal.shortcuts"]` once, defensively: this is a
/// preference file, not a format with a migration path, so anything that
/// doesn't look right is dropped rather than repaired. An id not in `ACTIONS`
/// is dropped. A value is kept only if it's `null` (an explicit unbind) or a
/// string that is both well-shaped (`isWellFormedBinding`) and carries a real
/// modifier (`hasModifier`) — the same two checks `setBinding` itself
/// enforces on a fresh rebind. `JSON.parse` or storage access throwing (bad
/// JSON, a private-mode browser, a quota error) yields no overrides at all.
function readStoredOverrides(): Overrides {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return {};
    const parsed: unknown = JSON.parse(raw);
    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Array.isArray(parsed)
    ) {
      return {};
    }
    const result: Overrides = {};
    for (const [id, value] of Object.entries(
      parsed as Record<string, unknown>,
    )) {
      if (!(id in ACTIONS)) continue;
      const actionId = id as ActionId;
      if (value === null) {
        result[actionId] = null;
      } else if (
        typeof value === "string" &&
        isWellFormedBinding(value) &&
        hasModifier(value)
      ) {
        result[actionId] = value;
      }
    }
    return result;
  } catch {
    return {};
  }
}

/// One capture-phase `window` listener, built with `tinykeys`, dispatches
/// every action's effective binding to whichever handler last registered for
/// it (`useShortcut`, below). The map is rebuilt whenever the effective
/// bindings change, which in practice means whenever an override is set,
/// cleared or reset.
export function ShortcutsProvider({ children }: { children: ReactNode }) {
  // A `Map<ActionId, handler[]>`, most recently registered last. Kept in a
  // ref rather than state: registering and unregistering happen on mount and
  // unmount of components all over the tree, and none of that should cause
  // the provider itself to re-render or the listener to be torn down.
  const handlers = useRef(new Map<ActionId, Handler[]>());

  const [overrides, setOverrides] = useState<Overrides>(readStoredOverrides);

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
  }, [overrides]);

  const effectiveBindings = useMemo<Record<ActionId, string | null>>(() => {
    const result = {} as Record<ActionId, string | null>;
    for (const id of Object.keys(ACTIONS) as ActionId[]) {
      result[id] =
        id in overrides ? (overrides[id] ?? null) : ACTIONS[id].default;
    }
    return result;
  }, [overrides]);

  useEffect(() => {
    const map: Record<string, (event: KeyboardEvent) => void> = {};
    for (const id of Object.keys(ACTIONS) as ActionId[]) {
      const binding = effectiveBindings[id];
      if (binding === null) continue;
      map[binding] = (event) => {
        const registered = handlers.current.get(id);
        const handler = registered?.[registered.length - 1];
        // No handler registered for this chord right now: do nothing, and
        // leave the keypress alone for whatever else wants it.
        if (!handler) return;
        // Focus is trapped in an open dialog, so a global shortcut shouldn't
        // reach behind it. This also keeps phase B's key recorder from
        // firing the very action it is capturing a new chord for.
        //
        // This checks for an open dialog anywhere in the document rather
        // than walking up from `event.target`, on purpose: MUI's
        // `FocusTrap` autofocuses its own wrapping `MuiDialog-container`
        // div the instant a dialog opens (see
        // `node_modules/@mui/material/Unstable_TrapFocus/FocusTrap.js`),
        // and that div is the *parent* of the `role="dialog"` element, not
        // a descendant of it — so `event.target.closest('[role="dialog"]')`
        // misses every keypress up until the user tabs or clicks something
        // inside. A backdrop-modal dialog already blocks every other
        // interaction while open, so "a dialog is open" and "the event
        // originated from behind/inside it" are the same thing here.
        if (document.querySelector('[role="dialog"]')) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        handler();
      };
    }
    // tinykeys's default `ignore` skips events from `[contenteditable]`,
    // `input`, `select` and `textarea` targets — which would silence every
    // shortcut here while the user is typing in the editor or the search
    // field, exactly where several of them need to work. Keep only the two
    // checks that matter regardless of target: a held-down key repeating,
    // and an IME still composing a character.
    return tinykeys(window, map, {
      capture: true,
      ignore: (event) => event.repeat || event.isComposing,
    });
  }, [effectiveBindings]);

  const value = useMemo<ShortcutsContextValue>(
    () => ({
      bindingFor: (id) => effectiveBindings[id],
      labelFor: (id) => ACTIONS[id].label,
      register: (id, handler) => {
        const list = handlers.current.get(id) ?? [];
        list.push(handler);
        handlers.current.set(id, list);
      },
      unregister: (id, handler) => {
        const list = handlers.current.get(id);
        if (!list) return;
        const next = list.filter((h) => h !== handler);
        if (next.length > 0) handlers.current.set(id, next);
        else handlers.current.delete(id);
      },
      // `b === null`: unbind, always allowed, no chord to validate.
      // Otherwise: reject a chord with no real modifier, one `RESERVED` for
      // the editor, or one already bound to a different action — in that
      // order, naming the chord (`formatBinding`, not the raw string) and,
      // for a conflict, the other action's label.
      setBinding: (id, b) => {
        if (b !== null) {
          if (!hasModifier(b)) {
            return "Add Ctrl, Alt or ⌘ — a single key would fire while you type.";
          }
          if (RESERVED.includes(b)) {
            return `${formatBinding(b)} is used by the editor.`;
          }
          const conflictId = (Object.keys(ACTIONS) as ActionId[]).find(
            (other) => other !== id && effectiveBindings[other] === b,
          );
          if (conflictId) {
            return `${formatBinding(b)} is already used by ${ACTIONS[conflictId].label}.`;
          }
        }
        setOverrides((prev) => {
          // Storing a copy of the default would be indistinguishable from a
          // deliberate override once it stops being the default (it never
          // does today, but the registry isn't a promise it never will).
          if (b === ACTIONS[id].default) {
            if (!(id in prev)) return prev;
            const next = { ...prev };
            delete next[id];
            return next;
          }
          return { ...prev, [id]: b };
        });
        return null;
      },
      resetBinding: (id) => {
        setOverrides((prev) => {
          if (!(id in prev)) return prev;
          const next = { ...prev };
          delete next[id];
          return next;
        });
      },
      resetAll: () => setOverrides({}),
    }),
    [effectiveBindings],
  );

  return (
    <ShortcutsContext.Provider value={value}>
      {children}
    </ShortcutsContext.Provider>
  );
}

function useShortcutsContext(): ShortcutsContextValue {
  const ctx = useContext(ShortcutsContext);
  if (!ctx) {
    throw new Error("useShortcuts must be used within ShortcutsProvider");
  }
  return ctx;
}

/// Access to the registry and its per-device overrides: `bindingFor` and
/// `labelFor` for showing a chord (rail tooltips, the `⋯` menu,
/// `ShortcutsDialog`); `setBinding`, `resetBinding` and `resetAll` for
/// rebinding it (WP5's dialog). `setBinding` returns a user-facing refusal
/// string, or `null` once the override is saved.
export function useShortcuts(): Pick<
  ShortcutsContextValue,
  "bindingFor" | "labelFor" | "setBinding" | "resetBinding" | "resetAll"
> {
  return useShortcutsContext();
}

/// Registers `handler` for `id` while the calling component is mounted and
/// `enabled` is true — `enabled=false` means "not registered", the same as
/// being unmounted. If more than one component registers the same id at
/// once, the one most recently mounted wins; unregistering removes exactly
/// that registration, wherever it sits in the list.
///
/// `handler` is read through a ref, as `useIdleLock` does, so a new identity
/// on every render doesn't tear down and reinstall the registration.
export function useShortcut(
  id: ActionId,
  handler: Handler,
  enabled = true,
): void {
  const ctx = useShortcutsContext();
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  });

  useEffect(() => {
    if (!enabled) return;
    const stable: Handler = () => handlerRef.current();
    ctx.register(id, stable);
    return () => ctx.unregister(id, stable);
  }, [ctx, id, enabled]);
}
