import { useEffect } from "react";

/**
 * Collapses a panel to its rail while the window is too narrow to hold it, and
 * restores it when there is room again. The panel's own toggle keeps working at
 * any width — this only reacts to crossing the threshold.
 *
 * At mount it only collapses: a wide window opening says nothing about the
 * panel, and the one the user closed last time stays closed.
 */
export function useNarrowCollapse(query: string, setCollapsed: (collapsed: boolean) => void) {
  useEffect(() => {
    const mql = window.matchMedia(query);
    const apply = (e: MediaQueryListEvent) => setCollapsed(e.matches);
    if (mql.matches) setCollapsed(true);
    mql.addEventListener("change", apply);
    return () => mql.removeEventListener("change", apply);
  }, [query, setCollapsed]);
}
