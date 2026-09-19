import { describe, expect, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useSteadyValue } from "../hooks/useSteadyValue";

// The folded line of a run shows each step for a moment: quick calls must not
// flicker past, and the line must still end on the latest one.

const sleep = (ms: number) => act(() => new Promise<void>((resolve) => setTimeout(resolve, ms)));

describe("a steady value", () => {
  test("holds each value, skips the ones in between, and lands on the latest", async () => {
    const { result, rerender } = renderHook(({ value }) => useSteadyValue(value, 60), {
      initialProps: { value: "a" },
    });

    rerender({ value: "b" });
    rerender({ value: "c" });
    expect(result.current).toBe("a");

    await sleep(90);
    expect(result.current).toBe("c");
  });

  test("changes at once when the last one has been shown long enough", async () => {
    const { result, rerender } = renderHook(({ value }) => useSteadyValue(value, 20), {
      initialProps: { value: "a" },
    });
    await sleep(30);
    rerender({ value: "b" });
    await sleep(0);
    expect(result.current).toBe("b");
  });
});
