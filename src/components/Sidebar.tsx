import { useEffect, useRef, useState } from "react";
import {
  ChevronRight,
  Clock,
  FolderOpen,
  GitBranch,
  Keyboard,
  MessageSquare,
  PanelLeft,
  Plus,
  Settings,
  UserRound,
} from "lucide-react";
import { Dropdown } from "./Dropdown";
import { GettingStarted } from "./GettingStarted";
import type { ChatSummary } from "../lib/chat";
import type { AsideTab } from "../types";
import "./Sidebar.css";

type Props = {
  chats: ChatSummary[];
  repo: string | null;
  /** Folders opened lately, the last one first. */
  recent: string[];
  onOpenFolder: (path: string) => void;
  onPickFolder: () => void;
  activeChat: string | null;
  onSelectChat: (id: string) => void;
  onNewChat: () => void;
  onToggleCollapse: () => void;
  onOpenSettings: () => void;
  onOnboardingAction: (tab: AsideTab) => void;
};

/** Not a path: no folder is called this. */
const PICK = "\u0000pick";

const folderName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

export function Sidebar({
  chats,
  repo,
  recent,
  onOpenFolder,
  onPickFolder,
  activeChat,
  onSelectChat,
  onNewChat,
  onToggleCollapse,
  onOpenSettings,
  onOnboardingAction,
}: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const userWrap = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: PointerEvent) => {
      if (!userWrap.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setMenuOpen(false);
    document.addEventListener("pointerdown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);

  return (
    <nav className="sidebar">
      <div className="sidebar-top">
        <button className="newchat" type="button" onClick={onNewChat}>
          <Plus size={14} />
          <span className="label">New chat</span>
        </button>
        {/* One folder at a time: switching is here, and the list below is
            that folder's chats. */}
        <Dropdown
          below
          right
          label={<FolderOpen size={15} />}
          title="Switch folder"
          value={repo ?? ""}
          options={[
            ...recent.map((path) => ({ value: path, label: folderName(path), hint: path })),
            { value: PICK, label: "Open folder…" },
          ]}
          onPick={(value) => (value === PICK ? onPickFolder() : value !== repo && onOpenFolder(value))}
        />
        <button className="iconbtn" type="button" title="Collapse panel" onClick={onToggleCollapse}>
          <PanelLeft size={15} />
        </button>
      </div>

      <div className="group">
        {chats.length === 0 ? (
          <div className="empty">No chats yet.</div>
        ) : (
          chats.map((chat) => (
            <button
              key={chat.id}
              type="button"
              className={`chat${chat.id === activeChat ? " active" : ""}`}
              onClick={() => onSelectChat(chat.id)}
              title={chat.branchedFrom ? "A branch of an earlier chat" : undefined}
            >
              {chat.branchedFrom ? <GitBranch size={14} /> : <MessageSquare size={14} />}
              <span>{chat.title}</span>
            </button>
          ))
        )}
      </div>

      <div className="sidebar-bottom">
        <GettingStarted onAction={onOnboardingAction} />
        <div className="user-wrap" ref={userWrap}>
          <button
            type="button"
            className={`user${menuOpen ? " open" : ""}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((v) => !v)}
          >
            <UserRound size={16} />
            <span className="meta">Account</span>
            <ChevronRight className="chev" size={12} />
          </button>
          {menuOpen && (
            <div className="user-menu" role="menu">
              <button
                className="user-menu-item"
                role="menuitem"
                type="button"
                onClick={() => {
                  setMenuOpen(false);
                  onOpenSettings();
                }}
              >
                <span className="ico">
                  <Settings size={14} />
                </span>
                Settings
              </button>
              <button
                className="user-menu-item"
                role="menuitem"
                type="button"
                onClick={() => setMenuOpen(false)}
              >
                <span className="ico">
                  <Keyboard size={14} />
                </span>
                Keyboard shortcuts
              </button>
              <button
                className="user-menu-item"
                role="menuitem"
                type="button"
                onClick={() => setMenuOpen(false)}
              >
                <span className="ico">
                  <Clock size={14} />
                </span>
                About Laika Agent
              </button>
            </div>
          )}
        </div>
      </div>
    </nav>
  );
}
