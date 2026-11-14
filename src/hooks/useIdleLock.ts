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
/// The countdown is a timer *and* a wall-clock deadline. A webview's timers run
/// on a monotonic clock that stops while the app is suspended (iOS backgrounding
/// or a locked screen), so a timer alone never fires for time spent away. The
/// last-activity timestamp is therefore re-checked whenever the page comes back
/// (`visibilitychange`, `pageshow`, `focus`) and on the first input after, and
/// the lock goes through at once if the deadline passed in the meantime.
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
    let lastActive = Date.now();
    let firing = false;
    let disposed = false;

    function arm() {
      clearTimeout(timer);
      timer = setTimeout(() => void fire(), ms);
    }

    async function fire() {
      if (firing) return;
      firing = true;
      try {
        const seen = activity;
        const done = await onIdleRef.current(
          () => !disposed && activity === seen,
        );
        if (!done && !disposed) {
          // Declined: a fresh quiet period, on the wall clock too.
          lastActive = Date.now();
          arm();
        }
      } finally {
        firing = false;
      }
    }

    function expired() {
      return Date.now() - lastActive >= ms;
    }

    function onActivity() {
      // The first touch after a long suspension must not rescue the session.
      if (expired()) {
        void fire();
        return;
      }
      activity++;
      lastActive = Date.now();
      arm();
    }

    function onResume() {
      if (document.visibilityState === "hidden") return;
      if (expired()) void fire();
      else arm();
    }

    for (const type of ACTIVITY_EVENTS) {
      window.addEventListener(type, onActivity, { passive: true });
    }
    document.addEventListener("visibilitychange", onResume);
    window.addEventListener("pageshow", onResume);
    window.addEventListener("focus", onResume);
    arm();

    return () => {
      disposed = true;
      clearTimeout(timer);
      for (const type of ACTIVITY_EVENTS) {
        window.removeEventListener(type, onActivity);
      }
      document.removeEventListener("visibilitychange", onResume);
      window.removeEventListener("pageshow", onResume);
      window.removeEventListener("focus", onResume);
    };
  }, [active, ms]);
}
