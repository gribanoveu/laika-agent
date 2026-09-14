export type ChatSummary = { id: string; title: string };

export type Session = {
  repo: string;
  branch: string;
  title: string;
  updatedAt: string;
  context: { used: number; limit: number };
};

export type ToolCall = {
  id: string;
  name: "Read" | "Grep" | "Edit" | "Bash";
  arg: string;
  meta?: string;
  stat?: { add: number; del: number };
  detail: string;
};

export type Approval = { id: string; tool: string; command: string; approved: boolean };

export type Turn = {
  id: string;
  role: "user" | "agent";
  text: string;
  tools?: (ToolCall | Approval)[];
};

export const isApproval = (t: ToolCall | Approval): t is Approval => "command" in t;

export type ChangedFile = { name: string; add: number; del: number };

export type AsideTab = "context" | "mcp" | "skills" | "rules" | "files" | "terminal";

export type PanelItem = {
  id: string;
  badge: string;
  kind: "mcp" | "skill" | "rule";
  title: string;
  status?: { label: string; tone: "ok" | "warn" | "off" };
  desc: string;
  meta?: string;
  tags?: string[];
  enabled?: boolean;
  rows?: { name: string; desc: string }[];
  note?: string;
  source?: string;
};
