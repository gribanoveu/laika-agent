import { useCallback, useRef, useState } from "react";

export const PANEL_LIMITS = {
  // `rail` is the collapsed width — mirrors the CSS in Sidebar.css/AsidePanel.css.
  sidebar: { min: 180, max: 420, initial: 248, rail: 58 },
  aside: { min: 260, max: 560, initial: 300, rail: 52 },
} as const;

/** How far past the minimum the drag must continue before the panel snaps shut. */
const COLLAPSE_OVERSHOOT_RATIO = 0.4;
/** How far a collapsed panel must be pulled out before it opens again. */
const EXPAND_THRESHOLD = 40;

type PanelKey = keyof typeof PANEL_LIMITS;

export type PanelControl = {
  collapsed: boolean;
  collapse: () => void;
  expand: () => void;
};

const clamp = (key: PanelKey, width: number) =>
  Math.min(PANEL_LIMITS[key].max, Math.max(PANEL_LIMITS[key].min, width));

/**
 * Panel widths driven by the edge handles. Dragging below the minimum holds the
 * panel at that minimum and banks the extra movement; once it adds up the panel
 * collapses. Pulling a collapsed panel outward reopens it the same way — the
 * drag-to-collapse gesture from docflow's usePanelLayout, plus the way back.
 */
export function usePanelSizes(controls: Record<PanelKey, PanelControl>) {
  const [widths, setWidths] = useState({
    sidebar: PANEL_LIMITS.sidebar.initial,
    aside: PANEL_LIMITS.aside.initial,
  });
  const widthsRef = useRef(widths);
  const overshoot = useRef<Record<PanelKey, number>>({ sidebar: 0, aside: 0 });
  // Distance the pointer still has to travel before it meets the panel edge
  // again. A panel reopens at its minimum width, which is wider than the rail
  // the pointer just left — without this it would keep widening from under the
  // cursor instead of waiting for it.
  const catchUp = useRef<Record<PanelKey, number>>({ sidebar: 0, aside: 0 });
  const controlsRef = useRef(controls);
  controlsRef.current = controls;

  const apply = useCallback((key: PanelKey, width: number) => {
    const next = { ...widthsRef.current, [key]: clamp(key, width) };
    widthsRef.current = next;
    setWidths(next);
  }, []);

  const resize = useCallback(
    (key: PanelKey, delta: number) => {
      const { min } = PANEL_LIMITS[key];
      const control = controlsRef.current[key];

      if (control.collapsed) {
        if (delta <= 0) return;
        overshoot.current[key] += delta;
        if (overshoot.current[key] >= EXPAND_THRESHOLD) {
          const travelled = overshoot.current[key];
          overshoot.current[key] = 0;
          catchUp.current[key] = Math.max(0, min - PANEL_LIMITS[key].rail - travelled);
          apply(key, min);
          control.expand();
        }
        return;
      }

      if (catchUp.current[key] > 0) {
        // Hold the width until the pointer has caught up with the edge.
        if (delta <= 0) {
          catchUp.current[key] -= delta;
          return;
        }
        const paid = Math.min(delta, catchUp.current[key]);
        catchUp.current[key] -= paid;
        delta -= paid;
        if (delta === 0) return;
      }

      const current = widthsRef.current[key];

      if (delta > 0) {
        overshoot.current[key] = 0;
        apply(key, current + delta);
        return;
      }

      const next = current + delta;
      if (next > min) {
        overshoot.current[key] = 0;
        apply(key, next);
        return;
      }

      apply(key, min);
      overshoot.current[key] += current <= min ? -delta : min - next;
      if (overshoot.current[key] >= min * COLLAPSE_OVERSHOOT_RATIO) {
        overshoot.current[key] = 0;
        catchUp.current[key] = 0;
        control.collapse();
      }
    },
    [apply],
  );

  return {
    widths,
    resizeSidebarBy: useCallback((delta: number) => resize("sidebar", delta), [resize]),
    resizeAsideBy: useCallback((delta: number) => resize("aside", delta), [resize]),
    endResize: useCallback(() => {
      overshoot.current = { sidebar: 0, aside: 0 };
      catchUp.current = { sidebar: 0, aside: 0 };
    }, []),
  };
}
