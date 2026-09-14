import { useEffect } from "react";

/**
 * Collapses a panel to its rail while the window is too narrow to hold it, and
 * restores it when there is room again. The panel's own toggle keeps working at
 * any width — this only reacts to crossing the threshold.
 */
export function useNarrowCollapse(query: string, setCollapsed: (collapsed: boolean) => void) {
  useEffect(() => {
    const mql = window.matchMedia(query);
    const apply = (e: MediaQueryList | MediaQueryListEvent) => setCollapsed(e.matches);
    apply(mql);
    mql.addEventListener("change", apply);
    return () => mql.removeEventListener("change", apply);
  }, [query, setCollapsed]);
}
