import { useCallback, useSyncExternalStore } from "react";

/**
 * Whether the media query matches now. The component redraws only when the
 * answer changes — a window resized pixel by pixel crosses a width once.
 */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const mql = window.matchMedia(query);
      mql.addEventListener("change", onChange);
      return () => mql.removeEventListener("change", onChange);
    },
    [query],
  );
  return useSyncExternalStore(subscribe, () => window.matchMedia(query).matches);
}
