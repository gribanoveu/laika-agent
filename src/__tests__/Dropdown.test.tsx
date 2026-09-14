// Guards the dropdown contract from AGENTS.md: the app draws its own menus
// (trigger button + role="listbox"), and they close on an outside pointerdown
// or Escape. Those two dismissals live in a `useEffect` that only runs while
// the menu is open — easy to break silently, since the menu still opens.
import { test, expect } from "bun:test";
import { fireEvent, render, screen } from "@testing-library/react";
import { Dropdown } from "../components/Dropdown";

const OPTIONS = [{ value: "main" }, { value: "fix/npe-tax-calc", hint: "current" }];

function open() {
  const picked: string[] = [];
  render(<Dropdown label="branch" options={OPTIONS} value="main" onPick={(v) => picked.push(v)} />);
  fireEvent.click(screen.getByRole("button", { name: /branch/ }));
  return picked;
}

test("opens a listbox rather than a native select", () => {
  open();
  expect(screen.getByRole("listbox")).toBeTruthy();
  expect(screen.getAllByRole("option")).toHaveLength(2);
  expect(document.querySelector("select")).toBeNull();
});

test("picking an option reports it and closes the menu", () => {
  const picked = open();
  fireEvent.click(screen.getByRole("option", { name: /fix\/npe-tax-calc/ }));
  expect(picked).toEqual(["fix/npe-tax-calc"]);
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("closes on an outside pointerdown", () => {
  open();
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("closes on Escape", () => {
  open();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("listbox")).toBeNull();
});
