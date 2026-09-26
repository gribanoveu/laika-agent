/**
 * A command typed into the composer as `/name` or `/name arguments`.
 *
 * The composer knows only this shape — where a command comes from is not its
 * business. The built-in ones are made in `App.tsx`; commands from files (a
 * prompt in `.claude/commands`) will be more entries in the same list, whose
 * `run` sends the prompt with the arguments put in.
 */
export type SlashCommand = {
  name: string;
  /** What it does, on the line under its name in the menu. */
  hint: string;
  /** Why it cannot run now, shown in its place; absent when it can. */
  unavailable?: string;
  run: (args: string) => void;
};

const NAME = "[a-z][\\w:-]*";

/**
 * `/name rest` → the name and what follows it. `null` when the text is not
 * shaped like a command — which is not the same as naming one that exists:
 * see `commandFor`.
 */
export function parseCommand(text: string): { name: string; args: string } | null {
  const m = new RegExp(`^\\/(${NAME})(?:\\s+([\\s\\S]*))?$`, "i").exec(text.trim());
  return m ? { name: m[1].toLowerCase(), args: (m[2] ?? "").trim() } : null;
}

/**
 * The command the text runs, with its arguments. A name no command has is not
 * one — `/usr/bin is missing` is a message, and is sent as one.
 */
export function commandFor(
  commands: readonly SlashCommand[],
  text: string,
): { command: SlashCommand; args: string } | null {
  const parsed = parseCommand(text);
  const command = parsed && commands.find((c) => c.name === parsed.name);
  return command && parsed ? { command, args: parsed.args } : null;
}

/** The commands offered while only a name is being typed: `/` alone offers all. */
export function suggestCommands(commands: readonly SlashCommand[], text: string): SlashCommand[] {
  const m = new RegExp(`^\\/(${NAME})?$`, "i").exec(text);
  if (!m) return [];
  const typed = (m[1] ?? "").toLowerCase();
  return commands.filter((c) => c.name.startsWith(typed));
}
