import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import "./PanelResizeHandle.css";

type Props = {
  /** Positive delta grows the panel this handle belongs to (see invert). */
  onResize: (delta: number) => void;
  onResizeEnd: () => void;
  /** True when the panel being sized sits to the right of the handle. */
  invert?: boolean;
  ariaLabel: string;
};

function clearDragStyles() {
  document.body.classList.remove("is-resizing");
  document.body.style.userSelect = "";
  document.body.style.cursor = "";
}

/**
 * The gap between two panels, made draggable. Ported from docflow's
 * PanelResizeHandle; it lives in the layout flow instead of on a panel edge.
 */
export function PanelResizeHandle({
  onResize,
  onResizeEnd,
  invert = false,
  ariaLabel,
}: Props) {
  const [active, setActive] = useState(false);
  const lastX = useRef(0);
  const activeRef = useRef(false);
  const onResizeRef = useRef(onResize);
  const onResizeEndRef = useRef(onResizeEnd);
  const invertRef = useRef(invert);

  onResizeRef.current = onResize;
  onResizeEndRef.current = onResizeEnd;
  invertRef.current = invert;
  activeRef.current = active;

  const finishDrag = useCallback(() => {
    if (!activeRef.current) return;
    activeRef.current = false;
    setActive(false);
    clearDragStyles();
    onResizeEndRef.current();
  }, []);

  useEffect(() => {
    if (!active) return;

    const onPointerMove = (event: PointerEvent) => {
      const raw = event.clientX - lastX.current;
      lastX.current = event.clientX;
      if (raw === 0) return;
      onResizeRef.current(invertRef.current ? -raw : raw);
    };

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);

    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      // The drag can end with the panel collapsed — never leave the resize
      // cursor and the selection lock behind.
      clearDragStyles();
    };
  }, [active, finishDrag]);

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    lastX.current = event.clientX;
    activeRef.current = true;
    setActive(true);
    document.body.classList.add("is-resizing");
    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";
  }, []);

  return (
    <div
      className={`panel-resize-handle${active ? " is-active" : ""}`}
      role="separator"
      aria-orientation="vertical"
      aria-label={ariaLabel}
      onPointerDown={onPointerDown}
    />
  );
}
