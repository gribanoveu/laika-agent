import type { Block } from "./chatTurnReducer";

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

const LABELS: Record<string, string> = {
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
};

/** A tool this build does not know is shown by its wire name rather than hidden. */
export const toolLabel = (wireName: string) => LABELS[wireName] ?? wireName;

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

  switch (block.name) {
    case "readFile": {
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
    str(args.path) ?? str(args.command) ?? str(args.pattern) ?? (wireName === "gitStatus" ? "" : "")
  );
}

function statusMark(status: string | undefined) {
  if (status === "completed") return "✓";
  if (status === "cancelled") return "✗";
  if (status === "inProgress") return "→";
  return "·";
}
