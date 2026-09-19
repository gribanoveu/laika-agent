import { describe, expect, test } from "bun:test";
import { fireEvent, render, screen } from "@testing-library/react";
import { Sidebar } from "../components/Sidebar";
import type { ChatSummary } from "../lib/chat";

function sidebar(repo: string | null, recent: string[], chats: ChatSummary[] = []) {
  const opened: string[] = [];
  let picked = 0;
  render(
    <Sidebar
      chats={chats}
      repo={repo}
      recent={recent}
      onOpenFolder={(path) => opened.push(path)}
      onPickFolder={() => picked++}
      activeChat={null}
      onSelectChat={() => {}}
      onNewChat={() => {}}
      onToggleCollapse={() => {}}
      onOpenSettings={() => {}}
      onOnboardingAction={() => {}}
    />,
  );
  return { opened, picked: () => picked };
}

describe("the folder switcher", () => {
  test("lists the recent folders by name and opens the one picked", () => {
    const h = sidebar("/work/a", ["/work/a", "/work/b"]);
    fireEvent.click(screen.getByTitle("Switch folder"));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
      "a/work/a",
      "b/work/b",
      "Open folder…",
    ]);
    fireEvent.click(screen.getByRole("option", { name: /^b/ }));
    expect(h.opened).toEqual(["/work/b"]);
  });

  test("the folder already open is not opened again", () => {
    const h = sidebar("/work/a", ["/work/a"]);
    fireEvent.click(screen.getByTitle("Switch folder"));
    fireEvent.click(screen.getByRole("option", { name: /^a/ }));
    expect(h.opened).toEqual([]);
  });

  test("Open folder… asks for one", () => {
    const h = sidebar(null, []);
    fireEvent.click(screen.getByTitle("Switch folder"));
    fireEvent.click(screen.getByRole("option", { name: "Open folder…" }));
    expect(h.picked()).toBe(1);
    expect(h.opened).toEqual([]);
  });
});

describe("the folder's name", () => {
  /// It folds the folder's chats, the way a project's do — switching
  /// folders is the icon beside it, not a click on the name.
  test("folds and unfolds the chats under it", () => {
    sidebar("/work/a", ["/work/a"], [{ id: "c1", title: "fix the parser", updatedAt: 0 }]);
    const name = screen.getByRole("button", { name: "a" });

    fireEvent.click(name);
    expect(screen.queryByText("fix the parser")).toBeNull();
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(name.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(name);
    expect(screen.getByText("fix the parser")).toBeTruthy();
  });
});
