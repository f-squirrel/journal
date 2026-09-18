import { useEffect, useState } from "react";

/// The current time, re-read every `intervalMs` while `active`, so a label
/// like "5 min ago" goes on being true without the data behind it changing.
/// Does nothing when inactive: no timer runs for a label that is not showing.
export function useNow(active: boolean, intervalMs = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [active, intervalMs]);
  return now;
}
