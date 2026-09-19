import { describe, expect, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useChatFontSize } from "../hooks/useChatFontSize";

// The preference lives in one CSS variable that the chat's type scale is
// multiplied by, and it survives a restart.

describe("the font size", () => {
  test("scales the type and is remembered", () => {
    localStorage.removeItem("atlas-cli-chat-font-size");
    const { result } = renderHook(() => useChatFontSize());
    expect(document.documentElement.style.getPropertyValue("--chat-font-scale")).toBe("1.1");

    act(() => result.current.setSize("larger"));
    expect(document.documentElement.style.getPropertyValue("--chat-font-scale")).toBe("1.2");
    expect(renderHook(() => useChatFontSize()).result.current.size).toBe("larger");
  });

  test("an unknown stored value falls back to large, the default", () => {
    localStorage.setItem("atlas-cli-chat-font-size", "huge");
    expect(renderHook(() => useChatFontSize()).result.current.size).toBe("large");
  });
});
