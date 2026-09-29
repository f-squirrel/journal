/// Helpers for a `TextField select` whose value is minutes, or `null` for
/// "never" / "only when I sync".

/// A `Select`-flavoured `TextField` whose value is `number | null`: MUI's
/// `Select` wants a primitive it can compare by `===`, and `null` fails that
/// silently (the menu just shows nothing selected), so `null` travels as the
/// sentinel string `"never"` between the field and the caller.
export const NEVER = "never";

export function minutesLabel(minutes: number): string {
  return minutes === 1 ? "1 minute" : `${minutes} minutes`;
}

/// `options` plus `current`, if it isn't already one of them — sorted back
/// into place, `null` (always "Never") last. The backend
/// accepts any value in its range (1..=240), not just the handful
/// the caller's options offer by default, so a value set some other way (an
/// older build, a future one, directly through `update_settings`) still shows
/// as a selected option instead of leaving the field blank.
export function withCurrent(
  options: readonly (number | null)[],
  current: number | null,
): (number | null)[] {
  if (current === null || options.includes(current)) return [...options];
  const numbers = options.filter((o): o is number => o !== null);
  const sorted: (number | null)[] = [...numbers, current].sort((a, b) => a - b);
  return [...sorted, null];
}
