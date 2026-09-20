import {
  BookText,
  ClipboardList,
  FolderClosed,
  Plug,
  Sparkles,
  SquareTerminal,
  Table2,
  Webhook,
} from "lucide-react";
import type { AsideTab } from "../types";

// What the side panel can show. One list, because two places name these: the
// chat header's "⋮", which opens them, and the panel's own heading, which says
// which one is open. There is no tab strip — eight icons in a 300px column
// were unreadable, and only one of them is opened often.
export const ASIDE_PANELS: { id: AsideTab; label: string; icon: typeof Table2 }[] = [
  { id: "changes", label: "Changes", icon: Table2 },
  { id: "plan", label: "Plan", icon: ClipboardList },
  { id: "mcp", label: "MCP", icon: Plug },
  { id: "hooks", label: "Hooks", icon: Webhook },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "rules", label: "Rules", icon: BookText },
  { id: "files", label: "Files", icon: FolderClosed },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
];
