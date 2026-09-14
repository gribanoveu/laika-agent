export type ChatSummary = { id: string; title: string };

// The transcript's own types live in `lib/chatTurnReducer.ts`, beside the code
// that builds them. What stays here is what the panels around the chat are
// still drawn from.

export type ChangedFile = { name: string; add: number; del: number };

export type AsideTab = "changes" | "mcp" | "skills" | "rules" | "files" | "terminal";

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
