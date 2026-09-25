import { describe, expect, test } from "bun:test";
import { comboKeys, matches, shortcutText, SHORTCUTS } from "../lib/shortcuts";
import { togglePane, type Docks } from "../lib/docks";

const key = (code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "shiftKey" | "altKey", boolean>> = {}) => ({
  code,
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

describe("a shortcut", () => {
  test("is ⌘ on a Mac and Ctrl elsewhere", () => {
    expect(matches(key("KeyB", { metaKey: true }), "sidebar", true)).toBe(true);
    expect(matches(key("KeyB", { ctrlKey: true }), "sidebar", true)).toBe(false);
    expect(matches(key("KeyB", { ctrlKey: true }), "sidebar", false)).toBe(true);
    expect(matches(key("KeyB", { metaKey: true }), "sidebar", false)).toBe(false);
  });

  test("takes Control as Control on a Mac too", () => {
    expect(matches(key("Backquote", { ctrlKey: true }), "terminal", true)).toBe(true);
    expect(matches(key("Backquote", { metaKey: true }), "terminal", true)).toBe(false);
  });

  test("needs exactly its modifiers", () => {
    expect(matches(key("KeyS", { metaKey: true, shiftKey: true }), "save", true)).toBe(false);
    expect(matches(key("Enter", { shiftKey: true }), "send")).toBe(false);
    expect(matches(key("Enter", { shiftKey: true }), "newLine")).toBe(true);
  });

  test("is drawn the way the platform writes it", () => {
    expect(comboKeys(SHORTCUTS.newChat.combos[0], true)).toEqual(["⌘", "N"]);
    expect(comboKeys(SHORTCUTS.newChat.combos[0], false)).toEqual(["Ctrl", "N"]);
    expect(comboKeys(SHORTCUTS.nextFile.combos[0], true)).toEqual(["⌥", "↓"]);
    expect(shortcutText("changes", true)).toBe("⌘1");
    expect(shortcutText("terminal", false)).toBe("Ctrl+`");
  });

  test("no two do the same thing with one key", () => {
    const seen = Object.values(SHORTCUTS).flatMap((s) => s.combos.map((c) => JSON.stringify(c)));
    expect(new Set(seen).size).toBe(seen.length);
  });
});

describe("a pane's shortcut", () => {
  const docks = (top: Docks["top"], topHidden: boolean, bottom: Docks["bottom"] = null): Docks => ({ top, topHidden, bottom });

  test("opens a pane that is not shown, and hides it where it is", () => {
    expect(togglePane(docks("changes", true), "mcp", "right")).toEqual(docks("mcp", false));
    expect(togglePane(docks("mcp", false), "mcp", "right")).toEqual(docks("mcp", true));
    expect(togglePane(docks("changes", false, "mcp"), "mcp", "right")).toEqual(docks("changes", false, null));
  });
});
