import { describe, expect, test } from "bun:test";
import { commandFor, parseCommand, suggestCommands, type SlashCommand } from "../lib/slashCommands";

const command = (name: string): SlashCommand => ({ name, hint: "", run: () => {} });
const commands = [command("compact"), command("fork")];

describe("a typed command", () => {
  test("is its name and what follows it", () => {
    expect(parseCommand("/fork")).toEqual({ name: "fork", args: "" });
    expect(parseCommand("  /Compact keep the plan \n")).toEqual({ name: "compact", args: "keep the plan" });
    expect(parseCommand("/review src/a.ts\nand b")).toEqual({ name: "review", args: "src/a.ts\nand b" });
  });

  test("is not text that merely starts with a slash", () => {
    expect(parseCommand("/")).toBeNull();
    expect(parseCommand("fork")).toBeNull();
    expect(parseCommand("/usr/bin is missing")).toBeNull();
    expect(parseCommand("// comment")).toBeNull();
  });

  test("runs only when a command has that name", () => {
    expect(commandFor(commands, "/fork now")?.command.name).toBe("fork");
    expect(commandFor(commands, "/fork now")?.args).toBe("now");
    expect(commandFor(commands, "/tmp is full")).toBeNull();
    expect(commandFor(commands, "/for")).toBeNull();
  });
});

describe("the offered commands", () => {
  test("are those starting with what is typed, all of them for a bare slash", () => {
    expect(suggestCommands(commands, "/").map((c) => c.name)).toEqual(["compact", "fork"]);
    expect(suggestCommands(commands, "/F").map((c) => c.name)).toEqual(["fork"]);
    expect(suggestCommands(commands, "/fork").map((c) => c.name)).toEqual(["fork"]);
    expect(suggestCommands(commands, "/x")).toEqual([]);
  });

  test("are gone once arguments start, and for anything else", () => {
    expect(suggestCommands(commands, "/fork ")).toEqual([]);
    expect(suggestCommands(commands, " /fork")).toEqual([]);
    expect(suggestCommands(commands, "hello")).toEqual([]);
    expect(suggestCommands(commands, "")).toEqual([]);
  });
});
