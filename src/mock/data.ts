// Static stand-in for the backend. Replace each export with a src/lib/ command
// wrapper once the matching #[tauri::command] exists.
import type { ChangedFile, ChatSummary, PanelItem, Turn } from "../types";

export const CHATS: ChatSummary[] = [
  { id: "npe", title: "Fix null pointer in calc tax" },
  { id: "bump", title: "Bump release to 0.2.0-alpha" },
];

export const SESSION = {
  repo: "tax-service",
  branch: "fix/npe-tax-calc",
  title: "Fix null pointer in calc tax",
  time: "55 minutes ago",
  context: { used: 16_000, limit: 260_000 },
};

export const TURNS: Turn[] = [
  {
    id: "t1",
    role: "user",
    text: "There's a NullPointerException when we calculate tax for entrepreneurs with zero declared income this quarter. Can you find the cause and fix it?",
  },
  {
    id: "t2",
    role: "agent",
    text: "Looking at the tax calculation path for zero-income filers. Starting with where the NPE originates.",
    tools: [
      {
        id: "c1",
        name: "Read",
        arg: "TaxCalculationService.java",
        meta: "lines 119-124",
        detail: `119  IncomeAmount income = profile.getDeclaredIncome();
120  if (income == null) {
121      // NPE here when mapper returns null
122  }`,
      },
      {
        id: "c2",
        name: "Grep",
        arg: "getDeclaredIncome(",
        meta: "6 matches · 4 files",
        detail: `TaxProfileMapper.java:42
TaxCalculationService.java:119
TaxCalculationTest.java:28
…`,
      },
    ],
  },
  {
    id: "t3",
    role: "agent",
    text: "Found it — TaxProfileMapper returns null for zero income instead of a zero-value IncomeAmount, and the calculator never guards against it. Fixing the mapper and adding a defensive check.",
    tools: [
      {
        id: "c3",
        name: "Edit",
        arg: "TaxProfileMapper.java",
        stat: { add: 4, del: 1 },
        detail: "- return null;\n+ return IncomeAmount.zero();",
      },
      {
        id: "c4",
        name: "Edit",
        arg: "TaxCalculationService.java",
        stat: { add: 3, del: 0 },
        detail: "+ if (income == null) income = IncomeAmount.zero();",
      },
      {
        id: "a1",
        tool: "Bash",
        command: "./gradlew test -tests TaxCalculationTest",
        approved: true,
      },
      {
        id: "c5",
        name: "Bash",
        arg: "./gradlew test -tests TaxCalculationTest",
        detail: "BUILD SUCCESSFUL in 12s\n2 tests passed",
      },
    ],
  },
  {
    id: "t4",
    role: "agent",
    text: "Fixed. The mapper now returns a zero-value IncomeAmount instead of null, and the calculator guards explicitly. Both tests pass, including the new zero-income case — ready to commit in the panel.",
  },
];

export const UNSTAGED: ChangedFile[] = [
  { name: "TaxProfileMapper.java", add: 4, del: 1 },
  { name: "TaxCalculationService.java", add: 3, del: 0 },
  { name: "TaxCalculationTest.java", add: 12, del: 0 },
];

export const DIFFS: Record<string, string> = {
  "TaxProfileMapper.java": "- return null;\n+ return IncomeAmount.zero();",
  "TaxCalculationService.java": "+ if (income == null) income = IncomeAmount.zero();",
  "TaxCalculationTest.java": "+ @Test void zeroIncome_returnsZeroTax() { … }",
};

export const GENERATED_COMMIT_MESSAGE = `fix: guard zero-income tax calculation

Return zero-value IncomeAmount from TaxProfileMapper instead of null
and add a defensive check in TaxCalculationService.`;

export const MCP_SERVERS: PanelItem[] = [
  {
    id: "context7",
    kind: "mcp",
    badge: "C7",
    title: "context7",
    status: { label: "connected", tone: "ok" },
    desc: "Library docs lookup — resolve-library-id, query-docs",
    meta: "2 tools · stdio",
    enabled: true,
    rows: [
      { name: "resolve-library-id", desc: "Find library ID by name" },
      { name: "query-docs", desc: "Fetch documentation snippets" },
    ],
  },
  {
    id: "tauri",
    kind: "mcp",
    badge: "T",
    title: "tauri",
    status: { label: "connected", tone: "ok" },
    desc: "Webview automation, IPC monitor, driver sessions",
    meta: "14 tools · local",
    enabled: true,
    rows: [
      { name: "webview_dom_snapshot", desc: "Capture DOM tree" },
      { name: "ipc_monitor", desc: "Listen to Tauri commands" },
      { name: "ipc_execute_command", desc: "Invoke backend command" },
    ],
  },
  {
    id: "git",
    kind: "mcp",
    badge: "G",
    title: "git",
    status: { label: "auth needed", tone: "warn" },
    desc: "Branch status, diff, commit helpers for the workspace repo",
    meta: "6 tools · disabled",
    enabled: false,
    rows: [
      { name: "status", desc: "Working tree summary" },
      { name: "diff", desc: "Staged and unstaged changes" },
    ],
  },
];

export const SKILLS: PanelItem[] = [
  {
    id: "ponytail",
    kind: "skill",
    badge: "P",
    title: "ponytail",
    status: { label: "active", tone: "ok" },
    desc: "Minimal solution first — YAGNI, stdlib before dependencies",
    tags: ["coding", "review"],
    enabled: true,
    note: 'Triggered on: coding tasks, "ponytail", "keep it simple"',
  },
  {
    id: "tauri-mcp-cli",
    kind: "skill",
    badge: "T",
    title: "tauri-mcp-cli",
    desc: "Start driver sessions, automate webviews, debug IPC",
    tags: ["tauri", "debug"],
    enabled: true,
    note: "Use when agent needs to operate the Tauri app from terminal.",
  },
  {
    id: "review-bugbot",
    kind: "skill",
    badge: "R",
    title: "review-bugbot",
    desc: "Defect-first review via Bugbot subagent",
    tags: ["review"],
    enabled: false,
    note: "Enable when user asks for Bugbot-style review.",
  },
  {
    id: "context7-mcp",
    kind: "skill",
    badge: "C",
    title: "context7-mcp",
    desc: "Fetch up-to-date library documentation via Context7",
    tags: ["docs"],
    enabled: true,
    note: "Auto-activates on library/API questions.",
  },
];

export const RULES: PanelItem[] = [
  {
    id: "agents-md",
    kind: "rule",
    badge: "A",
    title: "AGENTS.md",
    desc: "Architecture, IPC conventions, layered backend",
    note: `commands → services → domain
Every #[tauri::command] gets a typed wrapper in src/lib/
No unwrap() outside tests`,
    source: "atlas-desktop/AGENTS.md",
  },
  {
    id: "user-rules",
    kind: "rule",
    badge: "U",
    title: "User rules",
    desc: "Commit policy, Context7 for docs, bun only",
    note: `Only create commits when requested
Use Context7 MCP for library docs
Always use bun, never npm/yarn`,
    source: ".cursor/rules/",
  },
];

export const WORKSPACE_FILES = [
  "src/main/java/.../TaxProfileMapper.java",
  "src/main/java/.../TaxCalculationService.java",
  "src/test/java/.../TaxCalculationTest.java",
  "build.gradle",
];

export const LAST_COMMAND = {
  cmd: "./gradlew test -tests TaxCalculationTest",
  lines: [
    "TaxCalculationTest > zeroIncome_returnsZeroTax() PASSED",
    "TaxCalculationTest > standardRate_appliesCorrectly() PASSED",
  ],
  result: "BUILD SUCCESSFUL in 12s",
};

export const ONBOARDING = [
  {
    title: "Configure git",
    text: "Настройте git, чтобы коммитить изменения из чата.",
    action: "Configure git →",
    tab: null,
  },
  {
    title: "Connect MCP",
    text: "Подключите MCP-серверы для docs, git и автоматизации.",
    action: "Open MCP settings →",
    tab: "mcp",
  },
  {
    title: "Stage and commit",
    text: "Перенесите файлы в staged и создайте коммит в правой панели.",
    action: "Open Context panel →",
    tab: "context",
  },
] as const;

export const MODES = ["Auto", "Ask", "Manual"] as const;
export const MODELS = ["deepseek-flash", "claude-sonnet", "gpt-4.1"];
