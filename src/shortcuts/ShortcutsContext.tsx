import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  type ReactNode,
} from "react";
import { tinykeys } from "tinykeys";
import { useSettings } from "../context/SettingsContext";
import { ACTIONS, RESERVED, type ActionId } from "./actions";
import { formatBinding, hasModifier, isWellFormedBinding } from "./binding";

type Handler = () => void;

/// Per-device overrides of `ACTIONS`' defaults. A missing id means "use the
/// default"; a present id with value `null` means "unbound"; a present id
/// with a string means that chord instead.
type Overrides = Partial<Record<ActionId, string | null>>;

interface ShortcutsContextValue {
  bindingFor(id: ActionId): string | null;
  labelFor(id: ActionId): string;
  register(id: ActionId, handler: Handler): void;
  unregister(id: ActionId, handler: Handler): void;
  setBinding(id: ActionId, b: string | null): string | null;
  resetBinding(id: ActionId): string | null;
  resetAll(): void;
}

const ShortcutsContext = createContext<ShortcutsContextValue | null>(null);

/// Every action's chord once `overrides` is applied over `ACTIONS`' defaults.
function effectiveOf(overrides: Overrides): Record<ActionId, string | null> {
  const result = {} as Record<ActionId, string | null>;
  for (const id of Object.keys(ACTIONS) as ActionId[]) {
    result[id] =
      id in overrides ? (overrides[id] ?? null) : ACTIONS[id].default;
  }
  return result;
}

/// Sanitises `useSettings().settings.shortcuts` (the device setting), fed in
/// as `unknown` because it crosses `invoke`/the fallback and is opaque to
/// Rust beyond size limits — the
/// action registry and its chord rules live only here. Defensive like a
/// preference file with no migration path: anything that doesn't look right
/// is dropped rather than repaired. An id not in `ACTIONS` is dropped. A
/// value is kept only if it's `null` (an explicit unbind) or a string that is
/// well-shaped (`isWellFormedBinding`), carries a real modifier
/// (`hasModifier`) and isn't `RESERVED` — the checks `setBinding` itself
/// enforces on a fresh rebind. An override whose chord another action also
/// ends up on is sanitised to unbound instead, so no chord ever fires one
/// action while the dialog shows it on two.
function sanitizeOverrides(raw: unknown): Overrides {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return {};
  }
  const result: Overrides = {};
  for (const [id, value] of Object.entries(raw as Record<string, unknown>)) {
    if (!(id in ACTIONS)) continue;
    const actionId = id as ActionId;
    if (value === null) {
      result[actionId] = null;
    } else if (
      typeof value === "string" &&
      isWellFormedBinding(value) &&
      hasModifier(value) &&
      !RESERVED.includes(value)
    ) {
      result[actionId] = value;
    }
  }
  const effective = effectiveOf(result);
  for (const id of Object.keys(result) as ActionId[]) {
    const b = result[id];
    if (
      b != null &&
      (Object.keys(ACTIONS) as ActionId[]).some(
        (other) => other !== id && effective[other] === b,
      )
    ) {
      result[id] = null;
      effective[id] = null;
    }
  }
  return result;
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

  const { settings, update } = useSettings();
  // `settings.shortcuts` already reflects `confirmed ⊕ pending`
  // (`SettingsContext`), so a rebind shows at once, and a refused one reverts
  // on its own once the context re-fetches — no local state needed here.
  const overrides = useMemo(
    () => sanitizeOverrides(settings.shortcuts),
    [settings.shortcuts],
  );
  // Tracks the latest overrides `setBinding`/`resetBinding`/`resetAll` should
  // build `next` from. Assigned on every render (so it never falls behind
  // `settings.shortcuts`), *and* right after every write (so two calls made
  // back to back, before React has committed the render that `update`'s
  // `setPending` will eventually cause, still see each other's change —
  // reading the closed-over `overrides` instead would let the second call
  // start from the same stale snapshot the first one did, and undo it).
  const overridesRef = useRef(overrides);
  overridesRef.current = overrides;

  // Fire-and-forget: `update` queues the write and, on refusal, re-fetches
  // and drops the optimistic patch by itself (`SettingsContext`). Nothing
  // here needs to await it or show its own error for that.
  const writeOverrides = (next: Overrides) => {
    overridesRef.current = next;
    update({ shortcuts: next as Record<string, string | null> }).catch(() => {
      // Nothing to do here; see above.
    });
  };

  const effectiveBindings = useMemo(() => effectiveOf(overrides), [overrides]);

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

  const value = useMemo<ShortcutsContextValue>(() => {
    // The refusal for putting `b` on `id` when another action already has it.
    const conflictFor = (id: ActionId, b: string): string | null => {
      const conflictId = (Object.keys(ACTIONS) as ActionId[]).find(
        (other) => other !== id && effectiveBindings[other] === b,
      );
      return conflictId
        ? `${formatBinding(b)} is already used by ${ACTIONS[conflictId].label}.`
        : null;
    };
    return {
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
          const conflict = conflictFor(id, b);
          if (conflict) return conflict;
        }
        // Storing a copy of the default would be indistinguishable from a
        // deliberate override once it stops being the default (it never
        // does today, but the registry isn't a promise it never will).
        if (b === ACTIONS[id].default) {
          if (!(id in overridesRef.current)) return null;
          const next = { ...overridesRef.current };
          delete next[id];
          writeOverrides(next);
        } else {
          writeOverrides({ ...overridesRef.current, [id]: b });
        }
        return null;
      },
      // Refused when another action has since taken the default chord (e.g.
      // New entry cleared, Lock journal moved onto its Ctrl+N): restoring it
      // would put two actions on one chord.
      resetBinding: (id) => {
        const b = ACTIONS[id].default;
        if (b !== null) {
          const conflict = conflictFor(id, b);
          if (conflict) return conflict;
        }
        if (id in overridesRef.current) {
          const next = { ...overridesRef.current };
          delete next[id];
          writeOverrides(next);
        }
        return null;
      },
      resetAll: () => writeOverrides({}),
    };
  }, [effectiveBindings, overrides, update]);

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
/// rebinding it (the shortcuts dialog). `setBinding` and `resetBinding` return a
/// user-facing refusal string, or `null` once the change is saved.
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
