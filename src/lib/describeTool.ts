import type { Block } from "./chatTurnReducer";
import { processStatus, type ProcessState } from "./chat";

// One tool call, as a row in the transcript: what it did, to what, and the
// body you get when you expand it.
//
// Rendering, not logic — which is why it lives on this side. The backend sends
// the call's raw arguments and its typed result; turning `{"path":"a.rs",
// "startLine":119}` into `a.rs · lines 119-124` is a question about what reads
// well in a list, and the answer changes without the protocol changing.
//
// Everything here is defensive: arguments arrive a character at a time while
// the model is still writing them, so half of them are not valid JSON yet.

export type ToolDisplay = {
  /** Short label, as the prototype spells it: Read, Grep, Edit, Bash. */
  name: string;
  /** What it acted on — the path, the pattern, the command line. */
  arg: string;
  /** The secondary fact: a line range, a match count, a diff size, an exit code. */
  meta?: string;
  /** The expandable body. Empty means there is nothing to expand. */
  detail: string;
};

export const LABELS: Record<string, string> = {
  readFile: "Read",
  grep: "Grep",
  listFiles: "List",
  writeFile: "Write",
  editFile: "Edit",
  createDirectory: "Mkdir",
  deleteFile: "Delete",
  deleteDirectory: "Delete",
  move: "Move",
  todo: "Todo",
  gitStatus: "Status",
  gitDiff: "Diff",
  gitBlame: "Blame",
  runCommand: "Bash",
  semanticSearch: "Search",
  skill: "Skill",
  writePlan: "Plan",
  readOutput: "Output",
  stopProcess: "Stop",
};

/** `mcp__<server>__<tool>` as `server · tool`; `null` for any other name. */
export function mcpParts(wireName: string): { server: string; tool: string } | null {
  if (!wireName.startsWith("mcp__")) return null;
  const rest = wireName.slice("mcp__".length);
  const cut = rest.indexOf("__");
  return cut < 0 ? { server: rest, tool: "" } : { server: rest.slice(0, cut), tool: rest.slice(cut + 2) };
}

/** A tool this build does not know is shown by its wire name rather than hidden. */
export const toolLabel = (wireName: string) => {
  const mcp = mcpParts(wireName);
  if (mcp) return `${mcp.server} · ${mcp.tool}`;
  return LABELS[wireName] ?? wireName;
};

type Json = Record<string, unknown>;

const asObject = (value: unknown): Json => (value && typeof value === "object" ? (value as Json) : {});

/** Partial JSON is the normal case while a call is still being written. */
function parseArguments(raw: string): Json {
  try {
    return asObject(JSON.parse(raw));
  } catch {
    return {};
  }
}

const str = (value: unknown) => (typeof value === "string" ? value : undefined);
const num = (value: unknown) => (typeof value === "number" ? value : undefined);

export function describeTool(block: Extract<Block, { kind: "tool" }>): ToolDisplay {
  const args = parseArguments(block.arguments);
  const result = asObject(block.result);
  const name = toolLabel(block.name);

  if (block.error) {
    return { name, arg: primaryArgument(block.name, args), meta: "failed", detail: block.error };
  }

  // A connected server's tool. Its arguments are shown whole: nothing here
  // knows which of a foreign tool's fields matters, and the approval card
  // shows this line — agreeing to a call must not mean agreeing blind.
  if (mcpParts(block.name)) {
    return { name, arg: Object.keys(args).length ? JSON.stringify(args) : "", detail: str(result.text) ?? "" };
  }

  switch (block.name) {
    case "readFile": {
      if (Array.isArray(result.entries)) {
        const entries = result.entries as Json[];
        return {
          name,
          arg: str(args.path) ?? "",
          meta: `outline · ${entries.length} entries · ${num(result.totalLines) ?? 0} lines`,
          detail: entries.map((e) => `${num(e.startLine)}-${num(e.endLine)}  ${str(e.name)}`).join("\n"),
        };
      }
      const from = num(result.startLine) ?? num(args.startLine);
      const to = num(result.endLine) ?? num(args.endLine);
      return {
        name,
        arg: str(args.path) ?? "",
        meta: from && to ? `lines ${from}-${to}` : undefined,
        detail: str(result.content) ?? "",
      };
    }

    case "grep": {
      const matches = Array.isArray(result.matches) ? (result.matches as Json[]) : [];
      const files = new Set(matches.map((m) => str(m.path)).filter(Boolean));
      return {
        name,
        arg: str(args.pattern) ?? "",
        meta: matches.length
          ? `${matches.length}${result.truncated ? "+" : ""} matches · ${files.size} files`
          : undefined,
        detail: matches.map((m) => `${str(m.path)}:${num(m.line)}  ${str(m.text) ?? ""}`).join("\n"),
      };
    }

    case "semanticSearch": {
      const matches = Array.isArray(result.matches) ? (result.matches as Json[]) : [];
      const meta = asObject(result.meta);
      const lines = matches.map((m) => {
        const name = str(m.name);
        return `${str(m.path)}:${num(m.startLine)}-${num(m.endLine)}${name ? `  ${name}` : ""}`;
      });
      // The hint is what the model was told to do next; the reader should see it too.
      const hint = str(meta.hint);
      return {
        name,
        arg: str(args.query) ?? "",
        meta: block.result === undefined ? undefined : `${matches.length} matches`,
        detail: [...lines, ...(hint ? ["", hint] : [])].join("\n"),
      };
    }

    case "listFiles": {
      const entries = Array.isArray(result.entries) ? (result.entries as Json[]) : [];
      return {
        name,
        arg: str(args.path) ?? ".",
        meta: entries.length ? `${entries.length}${result.truncated ? "+" : ""} entries` : undefined,
        detail: entries.map((e) => str(e.path) ?? "").join("\n"),
      };
    }

    case "writeFile":
    case "editFile":
    case "deleteFile": {
      const diff = asObject(result.diff);
      const added = num(diff.linesAdded);
      const removed = num(diff.linesRemoved);
      return {
        name,
        arg: str(args.path) ?? "",
        meta: added === undefined ? undefined : `+${added} -${removed ?? 0}`,
        detail: str(diff.unifiedDiff) ?? "",
      };
    }

    case "move":
      return {
        name,
        arg: `${str(args.path) ?? ""} → ${str(args.newPath) ?? ""}`,
        detail: "",
      };

    case "runCommand": {
      // Started to run on: the answer is a number, not an exit code.
      if (args.background === true) {
        const id = num(result.id);
        return { name, arg: str(args.command) ?? "", meta: id === undefined ? "background" : `background #${id}`, detail: "" };
      }
      const streamed = block.output;
      const settled = `${str(result.stdout) ?? ""}${str(result.stderr) ?? ""}`;
      const code = num(result.exitCode);
      return {
        name,
        arg: str(args.command) ?? "",
        // Two different facts, and a reader needs to tell them apart: a killed
        // command has no exit code at all, and calling that "exit 0" would read
        // as success.
        meta: result.timedOut ? "timed out" : code === undefined ? undefined : `exit ${code}`,
        // While it runs there is only what has streamed in; once it settles the
        // captured output is authoritative — and shorter, being truncated in
        // the middle rather than cut off wherever the turn ended.
        detail: settled || streamed,
      };
    }

    case "readOutput":
    case "stopProcess": {
      const state = asObject(result.state);
      return {
        name,
        arg: [num(args.id) === undefined ? "" : `#${num(args.id)}`, str(result.command) ?? ""].filter(Boolean).join(" "),
        meta: state.state === undefined ? undefined : [processStatus(state as ProcessState), result.missed ? "some output lost" : ""].filter(Boolean).join(" · "),
        detail: str(result.output) ?? "",
      };
    }

    case "writePlan": {
      // The text is in the arguments; the result only counts its lines.
      const content = str(args.content) ?? "";
      const title = content.split("\n").find((line) => line.trim())?.replace(/^#+\s*/, "") ?? "";
      return {
        name,
        arg: title,
        meta: num(result.lines) === undefined ? undefined : `${num(result.lines)} lines`,
        detail: content,
      };
    }

    case "skill": {
      // A skill's files are listed after its instructions, as the model got them.
      const files = Array.isArray(result.files) ? (result.files as string[]) : [];
      const path = str(args.path);
      return {
        name,
        arg: [str(args.name) ?? "", path].filter(Boolean).join("/"),
        meta: files.length ? `${files.length} ${files.length === 1 ? "file" : "files"}` : undefined,
        detail: [(str(result.instructions) ?? str(result.content) ?? "").trimEnd(), files.map((f) => `· ${f}`).join("\n")]
          .filter(Boolean)
          .join("\n\n"),
      };
    }

    case "todo": {
      const tasks = Array.isArray(result.tasks) ? (result.tasks as Json[]) : [];
      const done = tasks.filter((t) => str(t.status) === "completed").length;
      return {
        name,
        arg: str(args.op) ?? "",
        meta: tasks.length ? `${done}/${tasks.length} done` : undefined,
        detail: tasks.map((t) => `${statusMark(str(t.status))} ${str(t.title) ?? ""}`).join("\n"),
      };
    }

    default:
      return {
        name,
        arg: primaryArgument(block.name, args),
        detail: block.result === undefined ? "" : JSON.stringify(block.result, null, 2),
      };
  }
}

function primaryArgument(wireName: string, args: Json): string {
  return (
    str(args.path) ??
    str(args.command) ??
    str(args.pattern) ??
    str(args.query) ??
    (wireName === "gitStatus" ? "" : "")
  );
}

function statusMark(status: string | undefined) {
  if (status === "completed") return "✓";
  if (status === "cancelled") return "✗";
  if (status === "inProgress") return "→";
  return "·";
}

// A run of calls folded into one line, the way a person would say it: "Read 3
// files, searched 2 patterns, ran a command". Each verb takes the count; a
// label without a phrase here is counted under its own name.
const RUN_PHRASES: Record<string, (n: number) => string> = {
  Read: (n) => `read ${n === 1 ? "a file" : `${n} files`}`,
  Grep: (n) => `searched ${n === 1 ? "a pattern" : `${n} patterns`}`,
  Search: (n) => `searched the code${n > 1 ? ` ${n} times` : ""}`,
  List: (n) => `listed ${n === 1 ? "a folder" : `${n} folders`}`,
  Write: (n) => `wrote ${n === 1 ? "a file" : `${n} files`}`,
  Edit: (n) => `edited ${n === 1 ? "a file" : `${n} files`}`,
  Delete: (n) => `deleted ${n === 1 ? "an item" : `${n} items`}`,
  Mkdir: (n) => `created ${n === 1 ? "a folder" : `${n} folders`}`,
  Move: (n) => `moved ${n === 1 ? "an item" : `${n} items`}`,
  Bash: (n) => `ran ${n === 1 ? "a command" : `${n} commands`}`,
  Todo: () => "updated the checklist",
  Plan: () => "wrote the plan",
};

export function describeRun(tools: Extract<Block, { kind: "tool" }>[]): string {
  const counts = new Map<string, number>();
  for (const tool of tools) {
    const name = toolLabel(tool.name);
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  const parts = [...counts].map(([name, n]) => RUN_PHRASES[name]?.(n) ?? (n === 1 ? name : `${name} ×${n}`));
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}
