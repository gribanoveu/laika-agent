import { useCallback, useRef } from "react";
import { useStoredState } from "./useStoredState";

type PanelKey = "sidebar" | "aside" | "bottom";
const KEYS: PanelKey[] = ["sidebar", "aside", "bottom"];
const zeros = (): Record<PanelKey, number> => ({ sidebar: 0, aside: 0, bottom: 0 });

export const PANEL_LIMITS: Record<PanelKey, { min: number; max: number; initial: number; rail?: number }> = {
  // `rail` is the collapsed width — mirrors the CSS in Sidebar.css. The side
  // panel has none: it is hidden from the chat header, never by dragging.
  sidebar: { min: 180, max: 420, initial: 248, rail: 58 },
  aside: { min: 260, max: 560, initial: 300 },
  // A height: the pane docked under the chat. Dragged past its minimum it closes.
  bottom: { min: 120, max: 640, initial: 240 },
};

/** How far past the minimum the drag must continue before the panel snaps shut. */
const COLLAPSE_OVERSHOOT_RATIO = 0.4;
/** How far a collapsed panel must be pulled out before it opens again. */
const EXPAND_THRESHOLD = 40;

// Within the limits too: they may have changed since the width was stored.
const isWidths = (value: unknown): value is Record<PanelKey, number> => {
  const v = value as Record<PanelKey, unknown> | null;
  return KEYS.every(
    (key) =>
      typeof v?.[key] === "number" && v[key] >= PANEL_LIMITS[key].min && v[key] <= PANEL_LIMITS[key].max,
  );
};

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
 * A panel without a control only stops at its minimum.
 */
export function usePanelSizes(controls: Partial<Record<PanelKey, PanelControl>>) {
  const [widths, setWidths] = useStoredState(
    "atlas-panel-widths",
    { sidebar: PANEL_LIMITS.sidebar.initial, aside: PANEL_LIMITS.aside.initial, bottom: PANEL_LIMITS.bottom.initial },
    isWidths,
  );
  const widthsRef = useRef(widths);
  const overshoot = useRef(zeros());
  // Distance the pointer still has to travel before it meets the panel edge
  // again. A panel reopens at its minimum width, which is wider than the rail
  // the pointer just left — without this it would keep widening from under the
  // cursor instead of waiting for it.
  const catchUp = useRef(zeros());
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

      if (control?.collapsed) {
        if (delta <= 0) return;
        overshoot.current[key] += delta;
        if (overshoot.current[key] >= EXPAND_THRESHOLD) {
          const travelled = overshoot.current[key];
          overshoot.current[key] = 0;
          catchUp.current[key] = Math.max(0, min - (PANEL_LIMITS[key].rail ?? 0) - travelled);
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
      if (!control) return;
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
    resizeBottomBy: useCallback((delta: number) => resize("bottom", delta), [resize]),
    endResize: useCallback(() => {
      overshoot.current = zeros();
      catchUp.current = zeros();
    }, []),
  };
}
