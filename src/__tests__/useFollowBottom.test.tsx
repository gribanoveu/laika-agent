import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { render } from "@testing-library/react";
import { useFollowBottom } from "../hooks/useFollowBottom";

// The chat thread stays at its end while the answer grows, and lets go of it
// the moment the user scrolls up to read.

let resized: (() => void)[] = [];
const RealObserver = globalThis.ResizeObserver;
beforeEach(() => {
  resized = [];
  globalThis.ResizeObserver = class {
    constructor(private cb: () => void) {}
    observe() {
      resized.push(this.cb);
    }
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});
afterEach(() => {
  globalThis.ResizeObserver = RealObserver;
});

function Thread({ onReady }: { onReady: (api: ReturnType<typeof useFollowBottom>) => void }) {
  const api = useFollowBottom();
  onReady(api);
  return (
    <div ref={api.scrollRef} data-testid="box">
      <div ref={api.contentRef} />
    </div>
  );
}

function setup() {
  let api!: ReturnType<typeof useFollowBottom>;
  const { getByTestId } = render(<Thread onReady={(a) => (api = a)} />);
  const box = getByTestId("box");
  let height = 1000;
  Object.defineProperty(box, "scrollHeight", { get: () => height });
  Object.defineProperty(box, "clientHeight", { get: () => 300 });
  const grow = (by: number) => {
    height += by;
    for (const cb of resized) cb();
  };
  const userScrollsTo = (top: number) => {
    box.scrollTop = top;
    box.dispatchEvent(new Event("scroll"));
  };
  return { box, grow, userScrollsTo, api: () => api };
}

describe("following the end of the thread", () => {
  test("scrolls to the end as the content grows", () => {
    const { box, grow } = setup();
    grow(200);
    expect(box.scrollTop).toBe(1200);
  });

  test("lets go once the user scrolls up, and picks up again at the end", () => {
    const { box, grow, userScrollsTo } = setup();
    grow(0);
    userScrollsTo(400);
    grow(200);
    expect(box.scrollTop).toBe(400);

    userScrollsTo(900); // 1200 - 300: the end again
    grow(100);
    expect(box.scrollTop).toBe(1300);
  });

  test("scrollToBottom follows again even from far up", () => {
    const { box, grow, userScrollsTo, api } = setup();
    grow(0);
    userScrollsTo(0);
    api().scrollToBottom();
    expect(box.scrollTop).toBe(1000);
    grow(50);
    expect(box.scrollTop).toBe(1050);
  });
});
