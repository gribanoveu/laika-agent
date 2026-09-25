import { useCallback, useRef } from "react";

/**
 * Keeps a scrolling box at its end while it grows — until the user scrolls
 * away from the end; scrolling back to it picks it up again.
 *
 * The scroll happens in a ResizeObserver, which runs after layout and before
 * paint, so no frame is drawn with the new text below the edge. The box is
 * observed too: a shorter window or a taller composer keeps the end in view.
 *
 * `scrollRef` goes on the scrolling box, `contentRef` on the one child that
 * holds everything inside it.
 */
export function useFollowBottom() {
  const box = useRef<HTMLElement | null>(null);
  const following = useRef(true);

  const stick = useCallback(() => {
    if (following.current && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, []);

  const scrollRef = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      box.current = el;
      // A pixel of slack: scrollTop is fractional on a Retina screen.
      const onScroll = () => {
        following.current = el.scrollHeight - el.clientHeight - el.scrollTop < 2;
      };
      const resize = new ResizeObserver(stick);
      resize.observe(el);
      el.addEventListener("scroll", onScroll, { passive: true });
      return () => {
        resize.disconnect();
        el.removeEventListener("scroll", onScroll);
        box.current = null;
      };
    },
    [stick],
  );

  const contentRef = useCallback(
    (el: HTMLElement | null) => {
      if (!el) return;
      const growth = new ResizeObserver(stick);
      growth.observe(el);
      return () => growth.disconnect();
    },
    [stick],
  );

  /** Back to the end, following again — for when the user just sent something. */
  const scrollToBottom = useCallback(() => {
    following.current = true;
    stick();
  }, [stick]);

  return { scrollRef, contentRef, scrollToBottom };
}
