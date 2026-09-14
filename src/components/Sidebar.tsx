import { useEffect, useRef, useState } from "react";
import {
  ChevronRight,
  ChevronUp,
  Clock,
  Keyboard,
  LogOut,
  MessageSquare,
  PanelLeft,
  Plus,
  Settings,
  SlidersHorizontal,
  UserRound,
} from "lucide-react";
import { GettingStarted } from "./GettingStarted";
import { CHATS, SESSION } from "../mock/data";
import type { AsideTab } from "../types";
import "./Sidebar.css";

type Props = {
  activeChat: string;
  onSelectChat: (id: string) => void;
  onToggleCollapse: () => void;
  onOpenSettings: () => void;
  onOnboardingAction: (tab: AsideTab) => void;
};

export function Sidebar({
  activeChat,
  onSelectChat,
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
        <button className="newchat" type="button">
          <Plus size={14} />
          <span className="label">New chat</span>
        </button>
        <button className="iconbtn" type="button" title="Сортировка">
          <SlidersHorizontal size={15} />
        </button>
        <button className="iconbtn" type="button" title="Свернуть панель" onClick={onToggleCollapse}>
          <PanelLeft size={15} />
        </button>
      </div>

      <div className="group">
        <div className="group-head">
          <span>{SESSION.repo}</span>
          <ChevronUp size={12} />
        </div>
        {CHATS.map((chat) => (
          <button
            key={chat.id}
            type="button"
            className={`chat${chat.id === activeChat ? " active" : ""}`}
            onClick={() => onSelectChat(chat.id)}
          >
            <MessageSquare size={14} />
            <span>{chat.title}</span>
          </button>
        ))}
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
            <span className="meta">Eugene</span>
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
                About atlas-cli
              </button>
              <div className="user-menu-sep" />
              <button
                className="user-menu-item danger"
                role="menuitem"
                type="button"
                onClick={() => setMenuOpen(false)}
              >
                <span className="ico">
                  <LogOut size={14} />
                </span>
                Sign out
              </button>
            </div>
          )}
        </div>
      </div>
    </nav>
  );
}
