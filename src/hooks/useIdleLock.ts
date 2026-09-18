import { useEffect, useRef } from "react";

const ACTIVITY_EVENTS = [
  "pointerdown",
  "pointermove",
  "keydown",
  "wheel",
  "touchstart",
] as const;

/// Calls `onIdle` once `ms` pass with no input while `active`.
///
/// `onIdle` may take a while (the lock has to write pending drafts first), so
/// it is handed `stillIdle`, which turns false the moment the user touches
/// anything or the hook is torn down. It resolves `false` to decline — the
/// timer then starts over, so a lock that could not go through is retried after
/// another quiet period rather than never or in a tight loop.
///
/// `onIdle` is read through a ref: its identity follows the context value, and
/// a timer keyed on it would restart on every unrelated state change.
export function useIdleLock(
  active: boolean,
  onIdle: (stillIdle: () => boolean) => Promise<boolean>,
  ms: number,
) {
  const onIdleRef = useRef(onIdle);
  useEffect(() => {
    onIdleRef.current = onIdle;
  });

  useEffect(() => {
    if (!active) return;

    let timer: ReturnType<typeof setTimeout> | undefined;
    let activity = 0;
    let disposed = false;

    function arm() {
      clearTimeout(timer);
      timer = setTimeout(() => void fire(), ms);
    }

    async function fire() {
      const seen = activity;
      const done = await onIdleRef.current(
        () => !disposed && activity === seen,
      );
      if (!done && !disposed) arm();
    }

    function onActivity() {
      activity++;
      arm();
    }

    for (const type of ACTIVITY_EVENTS) {
      window.addEventListener(type, onActivity, { passive: true });
    }
    arm();

    return () => {
      disposed = true;
      clearTimeout(timer);
      for (const type of ACTIVITY_EVENTS) {
        window.removeEventListener(type, onActivity);
      }
    };
  }, [active, ms]);
}
