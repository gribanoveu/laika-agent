import { useEffect, useRef, useState } from "react";

/**
 * `value`, but each one stays at least `minMs` before the next replaces it —
 * and the next is always the latest, so a burst of quick changes shows its
 * first and last rather than flickering through the middle.
 */
export function useSteadyValue<T>(value: T, minMs: number): T {
  const [shown, setShown] = useState(value);
  const since = useRef(Date.now());

  useEffect(() => {
    if (Object.is(value, shown)) return;
    const apply = () => {
      since.current = Date.now();
      setShown(value);
    };
    const wait = minMs - (Date.now() - since.current);
    if (wait <= 0) {
      apply();
      return;
    }
    const timer = setTimeout(apply, wait);
    return () => clearTimeout(timer);
  }, [value, shown, minMs]);

  return shown;
}
