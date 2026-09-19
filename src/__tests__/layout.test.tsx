import { afterEach, describe, expect, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { isBoolean, useStoredState } from "../hooks/useStoredState";
import { PANEL_LIMITS, usePanelSizes } from "../hooks/usePanelSizes";
import { useNarrowCollapse } from "../hooks/useNarrowCollapse";
import { isAsideTab } from "../types";

afterEach(() => localStorage.clear());

describe("a stored layout", () => {
  test("comes back on the next start", () => {
    const first = renderHook(() => useStoredState("k", false, isBoolean));
    act(() => first.result.current[1](true));
    first.unmount();
    const next = renderHook(() => useStoredState("k", false, isBoolean));
    expect(next.result.current[0]).toBe(true);
  });

  test("nothing stored, damaged, or of another shape is the default", () => {
    for (const raw of [null, "{oops", '"yes"']) {
      if (raw !== null) localStorage.setItem("k", raw);
      const { result, unmount } = renderHook(() => useStoredState("k", false, isBoolean));
      expect(result.current[0]).toBe(false);
      unmount();
    }
  });

  test("a tab since renamed opens the default one", () => {
    localStorage.setItem("tab", '"gone"');
    const { result } = renderHook(() => useStoredState("tab", "changes", isAsideTab));
    expect(result.current[0]).toBe("changes");
    expect(isAsideTab("terminal")).toBe(true);
  });
});

describe("panel widths", () => {
  const controls = {
    sidebar: { collapsed: false, collapse: () => {}, expand: () => {} },
    aside: { collapsed: false, collapse: () => {}, expand: () => {} },
  };

  test("are kept across starts", () => {
    const first = renderHook(() => usePanelSizes(controls));
    act(() => first.result.current.resizeSidebarBy(40));
    first.unmount();
    const next = renderHook(() => usePanelSizes(controls));
    expect(next.result.current.widths.sidebar).toBe(PANEL_LIMITS.sidebar.initial + 40);
  });

  test("outside today's limits are not trusted", () => {
    localStorage.setItem("atlas-panel-widths", JSON.stringify({ sidebar: 9999, aside: 300 }));
    const { result } = renderHook(() => usePanelSizes(controls));
    expect(result.current.widths.sidebar).toBe(PANEL_LIMITS.sidebar.initial);
  });
});

describe("a narrow window at start", () => {
  const original = window.matchMedia;
  afterEach(() => {
    window.matchMedia = original;
  });
  const withWidth = (matches: boolean) => {
    window.matchMedia = (() => ({
      matches,
      addEventListener: () => {},
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
  };

  test("collapses the panel", () => {
    withWidth(true);
    const set: boolean[] = [];
    renderHook(() => useNarrowCollapse("(max-width: 1px)", (v) => set.push(v)));
    expect(set).toEqual([true]);
  });

  test("but a wide one does not reopen a panel closed last time", () => {
    withWidth(false);
    const set: boolean[] = [];
    renderHook(() => useNarrowCollapse("(max-width: 1px)", (v) => set.push(v)));
    expect(set).toEqual([]);
  });
});
