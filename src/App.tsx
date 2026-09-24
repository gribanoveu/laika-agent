import { useCallback, useEffect, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { ChatPanel } from "./components/ChatPanel";
import { Composer } from "./components/Composer";
import { FolderTab } from "./components/FolderTab";
import { AsidePanel } from "./components/AsidePanel";
import { FileViewer } from "./components/FileViewer";
import { PANES, type Dock, type PaneContext } from "./components/panes";
import { Modal } from "./components/Modal";
import { Settings } from "./components/Settings";
import { ToolLog } from "./components/ToolLog";
import { ConfigFileEditor } from "./components/ConfigFileEditor";
import { PanelResizeHandle } from "./components/PanelResizeHandle";
import { Toast } from "./components/Toast";
import { WindowControls } from "./components/WindowControls";
import { useAgentTurn } from "./hooks/useAgentTurn";
import { useChatHistory } from "./hooks/useChatHistory";
import { useNarrowCollapse } from "./hooks/useNarrowCollapse";
import { useLlmSettings } from "./hooks/useLlmSettings";
import { useWorkspace } from "./hooks/useWorkspace";
import { useIndexStatus } from "./hooks/useIndexStatus";
import { useMcp } from "./hooks/useMcp";
import { useHooks } from "./hooks/useHooks";
import { useSkills } from "./hooks/useSkills";
import { useToolLog } from "./hooks/useToolLog";
import { usePanelSizes } from "./hooks/usePanelSizes";
import { useTheme } from "./hooks/useTheme";
import { useChatFontSize } from "./hooks/useChatFontSize";
import { useGitBranch } from "./hooks/useGitBranch";
import { useChangeTotals } from "./hooks/useChangeTotals";
import { useToast } from "./hooks/useToast";
import { nativeFrame, startWindowDrag, toggleMaximizeWindow } from "./lib/window";
import { pickSavePath } from "./lib/dialog";
import { useBackendSetting } from "./hooks/useBackendSetting";
import { useApprovalMemory } from "./hooks/useApprovalMemory";
import { useFolderConversation } from "./hooks/useFolderConversation";
import { isBoolean, useStoredState } from "./hooks/useStoredState";
import { McpServerForm, HookForm } from "./components/ConfigEntryForm";
import { removeHook, removeMcpServer } from "./lib/configEntries";
import { HOOKS_EXAMPLE, MCP_EXAMPLE, mergeHooks, mergeMcp } from "./lib/configSnippets";
import { changesShown, openPane, toggleChanges, toggleTerminal, type Docks } from "./lib/docks";
import { exportChat, setConversationMode, type ConversationMode } from "./lib/chat";
import { isAsideTab, type AsideTab } from "./types";
import { useFolderSwitch } from "./hooks/useFolderSwitch";
import { useOpenFiles } from "./hooks/useOpenFiles";
import { FolderSwitchDialog } from "./components/FolderSwitchDialog";
import "./App.css";

// Titlebar drag: single press drags the window, double press zooms it — the macOS
// titlebar contract, driven explicitly so clicks on the controls stay clicks.
const dragOrMaximize = (e: React.MouseEvent) => {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
  // Without this the webview keeps extending a text selection while the window
  // moves under the cursor, which flickers through whatever it passes over.
  e.preventDefault();
  if (e.detail === 2) toggleMaximizeWindow();
  else startWindowDrag();
};

/** A stored pane id, if it still opens in `dock` — a pane may have moved docks since it was stored. */
const isPaneIn =
  (dock: Dock) =>
  (value: unknown): value is AsideTab =>
    isAsideTab(value) && PANES.find((p) => p.id === value)?.dock === dock;
// Any pane may sit under the top one: Changes goes there when the top is taken.
const isBottomTab = (value: unknown): value is AsideTab | null => value === null || isAsideTab(value);

/** A config file's dialog: the whole file, one entry of it (null for a new one), or closed. */
type EntryDialog<K> = "json" | { entry: K | null } | null;

/** What "Implement in Agent mode" says on the user's behalf. Shown in the transcript like anything they type. */
const IMPLEMENT_PLAN = "Implement the plan above. Work through the checklist in order.";

export default function App() {
  // Laid out as it was left.
  const [collapsed, setCollapsed] = useStoredState("atlas-sidebar-collapsed", false, isBoolean);
  // Hidden until the chat header's button asks for it.
  const [asideHidden, setAsideHidden] = useStoredState("atlas-aside-hidden", true, isBoolean);
  const [tab, setTab] = useStoredState<AsideTab>("atlas-aside-tab", "changes", isPaneIn("right"));
  // The strip under the chat: closed when null.
  const [bottomTab, setBottomTab] = useStoredState<AsideTab | null>("atlas-bottom-tab", null, isBottomTab);
  // On screen in either dock: what decides whether a pane's data is read.
  const shown = (pane: AsideTab) => (tab === pane && !asideHidden) || bottomTab === pane;
  // Held here, not in the Changes pane: closing the pane, or moving it to the
  // other dock, would otherwise throw away a message half written. A different
  // folder is a different repository, and starts empty.
  const [commitMessage, setCommitMessage] = useState("");
  // Which background process a chat row asked the Terminal tab to show. The
  // row and the tab are in different subtrees, so it passes through here.
  const [processFocus, setProcessFocus] = useState<{ id: number } | null>(null);
  // A terminal selection on its way to the composer, from the other subtree.
  const [quote, setQuote] = useState<{ text: string; seq: number } | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  // The MCP and hooks files open as a whole (JSON) or one entry at a time
  // (a form): `entry` is the server's name or the hook's row, null for a new one.
  const [mcpDialog, setMcpDialog] = useState<EntryDialog<string>>(null);
  const [hooksDialog, setHooksDialog] = useState<EntryDialog<number>>(null);
  const mcpEditing = mcpDialog !== null;
  const hooksEditing = hooksDialog !== null;
  // What the agent may do this turn, and whether anyone is asked before it
  // does it. Two chips, two questions — and both are enforced on the backend,
  // so these hold only what the chips read back.
  const conversation = useBackendSetting(setConversationMode, "agent" as ConversationMode);
  const toast = useToast();
  const workspace = useWorkspace();
  const index = useIndexStatus(workspace.path);
  useEffect(() => setCommitMessage(""), [workspace.path]);
  // The files open in the viewer beside the chat, from Changes or Files.
  const viewer = useOpenFiles(workspace.path);
  const toolLog = useToolLog(logOpen);
  const history = useChatHistory(workspace.path);
  // The list is redrawn from disk after every save rather than guessed at
  // here: what belongs in it, and in what order, is the store's rule.
  const agent = useAgentTurn({ onSaved: history.refresh });
  const approval = useApprovalMemory(agent.chatId, workspace.path, () =>
    toast.show("Auto is on here — the agent will change files without asking"),
  );
  const branch = useGitBranch(workspace.path, agent.turn.status);
  const changeTotals = useChangeTotals(workspace.path);
  useFolderConversation(workspace.path, workspace.resumed, history.chats[0]?.id, agent);
  // Servers start with an Agent turn and may stop during one.
  // Settings shows both in "Where your data goes".
  const mcp = useMcp(shown("mcp") || mcpEditing || settingsOpen, agent.turn.status);
  const hooks = useHooks(shown("hooks") || hooksEditing || settingsOpen);
  // Settings has its own copy: which skills folders are read. The Skills
  // pane reads the same list for itself while it is open.
  const skillSources = useSkills(settingsOpen, workspace.path);
  const llm = useLlmSettings();
  const theme = useTheme();
  const fontSize = useChatFontSize();
  const panels = usePanelSizes({
    sidebar: {
      collapsed,
      collapse: () => setCollapsed(true),
      expand: () => setCollapsed(false),
    },
  });

  // Narrow window: the sidebar falls back to its rail and the side panel
  // hides, rather than both squeezing the chat. Widening again does not bring
  // the side panel back — it opens only when asked for.
  useNarrowCollapse("(max-width: 760px)", setCollapsed);
  const hideAsideWhenNarrow = useCallback(
    (narrow: boolean) => {
      if (!narrow) return;
      setAsideHidden(true);
      setBottomTab(null);
    },
    [setAsideHidden, setBottomTab],
  );
  useNarrowCollapse("(max-width: 900px)", hideAsideWhenNarrow);

  // Where each pane goes is `lib/docks.ts`'s rule; this only stores the answer.
  const docks: Docks = { top: tab, topHidden: asideHidden, bottom: bottomTab };
  const setDocks = (next: Docks) => {
    setTab(next.top);
    setAsideHidden(next.topHidden);
    setBottomTab(next.bottom);
  };
  const openTab = (next: AsideTab) =>
    setDocks(openPane(docks, next, PANES.find((p) => p.id === next)?.dock ?? "right"));

  // Ctrl+` shows and hides the Terminal, as in VS Code. Caught before the
  // terminal itself sees it, which would send the shell a NUL. Focus leaves
  // the composer, so the shell that opens takes the typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.code !== "Backquote") return;
      e.preventDefault();
      e.stopPropagation();
      (document.activeElement as HTMLElement | null)?.blur();
      setDocks(toggleTerminal({ top: tab, topHidden: asideHidden, bottom: bottomTab }));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [tab, asideHidden, bottomTab]);

  // A plan the agent has just finished writing is shown, once, when its turn
  // ends — not mid-turn, while it is still filling in the checklist.
  useEffect(() => {
    if (agent.planWritten > 0) openTab("plan");
  }, [agent.planWritten]);

  // The conversation just left is already on disk and stays in the sidebar;
  // this only stops pointing at it.
  const newChat = () => agent.reset();

  // The window the meter is drawn against, and the provider a turn talks to.
  const compactNow = async () => {
    if (!(await agent.compact(true))) {
      toast.show(agent.error ?? "Nothing worth folding away yet");
    }
  };

  const pickConversation = async (mode: ConversationMode) => {
    const failed = await conversation.pick(mode);
    if (failed) toast.show(failed);
  };

  // Said out loud in one direction only. Turning confirmations back on needs
  // no warning; turning them off means the next write happens without anyone
  // seeing it, and the chip alone is a small thing to have noticed.
  const pickUnattended = async (next: boolean) => {
    const failed = await approval.pick(next);
    if (failed) toast.show(failed);
    else if (next) toast.show("Auto — the agent will change files without asking");
  };

  // Read from state rather than after the `await`: the closure there still
  // holds the render before the failure.
  useEffect(() => {
    if (workspace.error) toast.show(workspace.error);
  }, [workspace.error]);

  // Leaving the folder ends what runs in it: the user is told first.
  const folderSwitch = useFolderSwitch(agent.turn.status === "running");
  const openFolder = (path: string) => folderSwitch.guard(() => void workspace.open(path));
  const chooseFolder = () => folderSwitch.guard(() => void workspace.pick());

  // Asking before the first message rather than refusing it — and then sending
  // it: the composer has already cleared the box, so anything not sent here is
  // typed twice.
  const send = async (text: string) => {
    if (!workspace.path && !(await workspace.pick())) return;
    agent.send(text);
  };

  const openChat = history.chats.find((one) => one.id === agent.chatId);

  // The conversation as a file, for reading it somewhere else. Only what is
  // on disk can be written out — a turn saves when it comes to rest, so this
  // exports everything up to the one still running.
  const exportOpenChat = async () => {
    const chat = openChat;
    if (!chat) return toast.show("Nothing saved to export yet");
    const path = await pickSavePath(chat.title, "md");
    if (!path) return;
    try {
      await exportChat(chat.id, path);
      toast.show(`Exported to ${path.split("/").pop()}`);
    } catch (e) {
      toast.show(String(e));
    }
  };

  // The mode first, and only then the message: the backend reads the mode
  // when the turn starts, and a turn sent a moment early would still be a
  // plan that cannot write.
  const implement = async () => {
    const failed = await conversation.pick("agent");
    if (failed) return toast.show(failed);
    agent.send(IMPLEMENT_PLAN);
  };

  // What every pane is drawn from, whichever dock it sits in.
  const panes: Omit<PaneContext, "active"> = {
    workspace: workspace.path,
    onNotify: toast.show,
    commitDraft: { message: commitMessage, onMessage: setCommitMessage },
    processFocus,
    onAddToChat: (text) => setQuote((last) => ({ text, seq: (last?.seq ?? 0) + 1 })),
    chatBlocks: agent.turn.blocks,
    openFile: viewer.active,
    onOpenFile: viewer.open,
    mcp: {
      view: mcp.view,
      error: mcpEditing ? null : mcp.error,
      onAdd: () => setMcpDialog({ entry: null }),
      onEditServer: (name) => setMcpDialog({ entry: name }),
      onRemoveServer: (name) => {
        const next = removeMcpServer(mcp.view?.text ?? "", name);
        if (next) void mcp.save(next);
      },
      onToggle: mcp.setEnabled,
      onOpen: mcp.connect,
      onEditFile: () => setMcpDialog("json"),
    },
    hooks: {
      view: hooks.view,
      error: hooksEditing ? null : hooks.error,
      onAdd: () => setHooksDialog({ entry: null }),
      onEditHook: (index) => setHooksDialog({ entry: index }),
      onRemoveHook: (index) => {
        const next = removeHook(hooks.view?.text ?? "", index);
        if (next) void hooks.save(next);
      },
      onEditFile: () => setHooksDialog("json"),
    },
    plan: {
      plan: agent.plan,
      checklist: agent.checklist,
      onEdit: agent.editPlan,
      onImplement: conversation.value === "plan" ? implement : undefined,
      locked: agent.turn.status === "running",
    },
  };

  return (
    <div
      className={`window${nativeFrame ? " native-frame" : ""}${collapsed ? " collapsed" : ""}${asideHidden ? " aside-hidden" : ""}${bottomTab ? "" : " bottom-closed"}`}
      style={
        {
          "--sidebar-width": `${panels.widths.sidebar}px`,
          "--aside-width": `${panels.widths.aside}px`,
          "--bottom-height": `${panels.widths.bottom}px`,
          "--viewer-width": `${panels.widths.viewer}px`,
        } as React.CSSProperties
      }
    >
      <div className="titlebar" onMouseDown={dragOrMaximize}>
        <WindowControls />
        <span className="titlebar-title">
          Laika{workspace.path && <span> · {workspace.path.split("/").pop()}</span>}
        </span>
      </div>

      <div className="body">
        <Sidebar
          chats={history.chats}
          activeChat={agent.chatId}
          onSelectChat={agent.open}
          onNewChat={newChat}
          onToggleCollapse={() => setCollapsed((v) => !v)}
          onOpenSettings={() => setSettingsOpen(true)}
          onOnboardingAction={openTab}
        />

        <PanelResizeHandle
          ariaLabel="Resize the chat list"
          onResize={panels.resizeSidebarBy}
          onResizeEnd={panels.endResize}
        />

        <main className="main">
          <ChatPanel
            title={openChat?.title ?? null}
            branched={Boolean(openChat?.branchedFrom)}
            workspace={workspace.path}
            turn={agent.turn}
            onDecide={agent.decide}
            onOpenRepo={chooseFolder}
            asideOpen={changesShown(docks)}
            onToggleAside={() => setDocks(toggleChanges(docks))}
            terminalOpen={bottomTab === "terminal"}
            onToggleTerminal={() => setDocks(toggleTerminal(docks))}
            onOpenPanel={openTab}
            onExport={exportOpenChat}
            onImplement={conversation.value === "plan" ? implement : undefined}
            onOpenPlan={() => openTab("plan")}
            onOpenProcess={(id) => {
              openTab("terminal");
              setProcessFocus({ id });
            }}
            branchable={agent.branchable}
            onBranch={agent.branch}
          />
          <Composer
            tab={
              <FolderTab
                path={workspace.path}
                recent={workspace.recent}
                onOpenFolder={openFolder}
                onPickFolder={chooseFolder}
                branch={branch}
                index={index}
                changes={changeTotals}
                onOpenChanges={() => openTab("changes")}
              />
            }
            onSend={send}
            onStop={agent.cancel}
            running={agent.turn.status === "running"}
            conversation={conversation.value}
            onConversation={pickConversation}
            unattended={approval.unattended}
            onUnattended={pickUnattended}
            draft={agent.draft}
            quote={quote}
            models={llm.models}
            onModel={(choice) => llm.pickModel(choice.providerId, choice.model)}
            onLoadModels={llm.loadModels}
            context={agent.context}
            usage={agent.turn.usage}
            onCompact={compactNow}
          />
        </main>

        {viewer.active && (
          <>
            <PanelResizeHandle
              invert
              ariaLabel="Resize the file viewer"
              onResize={panels.resizeViewerBy}
              onResizeEnd={panels.endResize}
            />
            <FileViewer
              files={viewer.files}
              active={viewer.active}
              workspace={workspace.path}
              onActivate={viewer.open}
              onClose={viewer.close}
              onCloseAll={viewer.closeAll}
            />
          </>
        )}

        {(!asideHidden || bottomTab) && (
          <PanelResizeHandle
            invert
            ariaLabel="Resize the side panel"
            onResize={panels.resizeAsideBy}
            onResizeEnd={panels.endResize}
          />
        )}

        {/* The column right of the chat: the pane from the header's button on
            top, the bottom dock under it. Either may be closed; the column
            goes when both are. */}
        <div className="dock-column">
          <AsidePanel
            tab={tab}
            dock="right"
            ctx={{ ...panes, active: !asideHidden }}
            onClose={() => setAsideHidden(true)}
          />
          {bottomTab && (
            <>
              {!asideHidden && (
                <PanelResizeHandle
                  axis="y"
                  invert
                  ariaLabel="Resize the bottom panel"
                  onResize={panels.resizeBottomBy}
                  onResizeEnd={panels.endResize}
                />
              )}
              <AsidePanel
                tab={bottomTab}
                dock="bottom"
                ctx={{ ...panes, active: true }}
                onClose={() => setBottomTab(null)}
              />
            </>
          )}
        </div>
      </div>

      <FolderSwitchDialog
        blocked={folderSwitch.blocked}
        folder={workspace.path}
        onStopAgent={agent.cancel}
        onClose={folderSwitch.close}
      />

      <Modal title="Settings" wide open={settingsOpen} onClose={() => setSettingsOpen(false)}>
        <Settings
          skills={{ view: skillSources.view, error: skillSources.error, onToggle: skillSources.setSourceEnabled }}
          provider={{
            settings: llm.settings,
            busy: llm.busy,
            error: llm.error,
            onSave: llm.save,
            onRemove: llm.remove,
            onSelect: llm.select,
          }}
          remember={approval.remember}
          onRemember={async (next) => {
            const failed = await approval.pickRemember(next);
            if (failed) toast.show(failed);
          }}
          debugLogging={llm.settings?.debugLogging ?? false}
          onDebugLogging={llm.debugLogging}
          theme={theme.preference}
          onTheme={theme.setPreference}
          fontSize={fontSize.size}
          onFontSize={fontSize.setSize}
          onOpenLog={() => {
            setSettingsOpen(false);
            setLogOpen(true);
          }}
          policy={{
            provider: llm.settings?.providers.find((p) => p.id === llm.settings?.activeProviderId) ?? null,
            debugLogging: llm.settings?.debugLogging ?? false,
            mcpServers: mcp.view?.servers ?? null,
            hooks: hooks.view?.hooks ?? null,
          }}
        />
      </Modal>

      <Modal title="Tool calls" wide open={logOpen} onClose={() => setLogOpen(false)}>
        <ToolLog
          rows={toolLog.rows}
          total={toolLog.total}
          filter={toolLog.filter}
          onFilter={toolLog.setFilter}
          onMore={toolLog.more}
          onClear={toolLog.clear}
          enabled={toolLog.enabled}
          onToggle={toolLog.toggle}
          error={toolLog.error}
        />
      </Modal>

      <Modal
        title={mcpDialog === "json" || mcpDialog === null ? "MCP servers" : mcpDialog.entry ?? "Add an MCP server"}
        wide={mcpDialog === "json"}
        open={mcpEditing}
        onClose={() => setMcpDialog(null)}
      >
        {mcpDialog !== null && mcpDialog !== "json" ? (
          <McpServerForm
            key={mcpDialog.entry ?? ""}
            name={mcpDialog.entry}
            text={mcp.view?.text}
            error={mcp.error}
            onSave={mcp.save}
            onClose={() => setMcpDialog(null)}
            onEditJson={() => setMcpDialog("json")}
          />
        ) : (
        <ConfigFileEditor
          label="MCP configuration"
          example={MCP_EXAMPLE}
          merge={mergeMcp}
          text={mcp.view?.text}
          error={mcp.error}
          onSave={mcp.save}
          onClose={() => setMcpDialog(null)}
          note={
            <>
              The <code>mcpServers</code> format of Claude Desktop and Cursor: paste a server's snippet as it is.
              Optional per server: <code>weight</code> (cost of a call in the turn's budget, 3 by default) and{" "}
              <code>timeoutSecs</code> (120). Kept in {mcp.view?.path || "the app directory"}, readable only by you —
              it may hold tokens.
            </>
          }
        />
        )}
      </Modal>

      <Modal
        title={hooksDialog === "json" || hooksDialog === null ? "Hooks" : hooksDialog.entry === null ? "Add a hook" : "Edit hook"}
        wide={hooksDialog === "json"}
        open={hooksEditing}
        onClose={() => setHooksDialog(null)}
      >
        {hooksDialog !== null && hooksDialog !== "json" ? (
          <HookForm
            key={hooksDialog.entry ?? -1}
            index={hooksDialog.entry}
            text={hooks.view?.text}
            error={hooks.error}
            onSave={hooks.save}
            onClose={() => setHooksDialog(null)}
            onEditJson={() => setHooksDialog("json")}
          />
        ) : (
        <ConfigFileEditor
          label="Hooks configuration"
          example={HOOKS_EXAMPLE}
          merge={mergeHooks}
          text={hooks.view?.text}
          error={hooks.error}
          onSave={hooks.save}
          onClose={() => setHooksDialog(null)}
          note={
            <>
              Claude Code's <code>hooks</code> format: <code>PreToolUse</code>, <code>PostToolUse</code> and{" "}
              <code>Stop</code> run here. The command gets the event as JSON on stdin; exit code 2 refuses the call,
              or sends the agent back from Stop, with stderr as the reason. A <code>matcher</code> names this app's
              tools — <code>runCommand</code>, <code>editFile</code> — not Claude Code's. Kept in{" "}
              {hooks.view?.path || "the app directory"}.
            </>
          }
        />
        )}
      </Modal>

      <Toast message={toast.message} />
    </div>
  );
}
