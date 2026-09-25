import { describe, expect, test } from "bun:test";
import { fireEvent, render, screen } from "@testing-library/react";
import { FolderTab } from "../components/FolderTab";
import type { ChangeTotals } from "../lib/chat";
import { fromSnapshot, type IndexState } from "../lib/indexStatus";

// The strip on the composer's top edge: which folder the next message is
// worked on, and what that folder is like — its branch, index and changes.

type Over = {
  path?: string | null;
  recent?: string[];
  branch?: string | null;
  index?: IndexState | null;
  changes?: ChangeTotals | null;
  onOpenFolder?: (path: string) => void;
  onPickFolder?: () => void;
  onOpenChanges?: () => void;
};

const tab = (over: Over = {}) =>
  render(
    <FolderTab
      path={over.path === undefined ? "/work/kibo" : over.path}
      recent={over.recent ?? []}
      branch={over.branch}
      index={over.index}
      changes={over.changes}
      onOpenFolder={over.onOpenFolder ?? (() => {})}
      onPickFolder={over.onPickFolder ?? (() => {})}
      onOpenChanges={over.onOpenChanges ?? (() => {})}
    />,
  );

const trigger = () => screen.getByRole("button", { expanded: false });

describe("the folder", () => {
  test("asks for one while none is open, and shows nothing else", () => {
    let picks = 0;
    tab({ path: null, branch: "main", changes: { files: 1, add: 1, del: 0 }, onPickFolder: () => picks++ });
    expect(trigger().textContent).toContain("Select folder");
    expect(screen.queryByText("main")).toBeNull();
    expect(screen.queryByTitle(/changed since the last commit/)).toBeNull();

    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("option", { name: /Open folder/ }));
    expect(picks).toBe(1);
  });

  /// Picking the folder already open is not a switch: it would reload its chats for nothing.
  test("names the open one, the whole path on hover, and switches only to another", () => {
    const opened: string[] = [];
    tab({ recent: ["/work/kibo", "/work/atlas"], onOpenFolder: (p) => opened.push(p) });
    expect(trigger().textContent).toContain("kibo");
    expect(trigger().getAttribute("title")).toBe("/work/kibo");

    fireEvent.click(trigger());
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
      "kibo/work/kibo",
      "atlas/work/atlas",
      "Open folder…",
    ]);
    fireEvent.click(screen.getByRole("option", { name: /^kibo/ }));
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("option", { name: /^atlas/ }));
    expect(opened).toEqual(["/work/atlas"]);
  });
});

describe("the branch", () => {
  test("is shown where there is one", () => {
    tab({ branch: "feature/parser" });
    expect(screen.getByTitle("Checked-out branch").textContent).toBe("feature/parser");
  });

  test("outside a repository there is none", () => {
    tab({ branch: null });
    expect(screen.queryByTitle("Checked-out branch")).toBeNull();
  });
});

describe("the index", () => {
  test("its state is shown, with the detail on hover", () => {
    tab({ index: fromSnapshot({ root: "/work/kibo", syncing: false, embedded: 3, skipped: 2, embeddingError: null }) });
    const badge = screen.getByRole("status");
    expect(badge.textContent).toBe("Indexed");
    expect(badge.getAttribute("title")).toContain("2 files not indexed");
  });

  test("nothing is shown before anything is known", () => {
    tab();
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("the changes", () => {
  test("shows lines added and removed, and opens Changes", () => {
    let opened = 0;
    tab({ changes: { files: 2, add: 12, del: 3 }, onOpenChanges: () => opened++ });
    const button = screen.getByTitle("2 files changed since the last commit");
    expect(button.textContent).toBe("+12−3");
    fireEvent.click(button);
    expect(opened).toBe(1);
  });

  test("a clean folder, or no repository, shows nothing", () => {
    tab({ changes: { files: 0, add: 0, del: 0 } });
    expect(screen.queryByTitle(/changed since the last commit/)).toBeNull();
  });
});
