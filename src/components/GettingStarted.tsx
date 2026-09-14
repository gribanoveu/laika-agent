import { useState } from "react";
import type { AsideTab } from "../types";
import "./GettingStarted.css";

const KEY = "atlas-cli-getting-started-skipped";

// Onboarding copy is UI text, not backend data — it lives with the component.
const CARDS: { title: string; text: string; action: string; tab: AsideTab | null }[] = [
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
    action: "Open Changes panel →",
    tab: "changes",
  },
];

export function GettingStarted({ onAction }: { onAction: (tab: AsideTab) => void }) {
  const [index, setIndex] = useState(0);
  const [skipped, setSkipped] = useState(() => localStorage.getItem(KEY) === "1");

  if (skipped) return null;

  const card = CARDS[index];
  const last = index === CARDS.length - 1;

  const dismiss = () => {
    setSkipped(true);
    localStorage.setItem(KEY, "1");
  };

  return (
    <div className="getting-started">
      <div className="gs-head">
        <span className="getting-started-title">Getting started</span>
        <button className="gs-skip" type="button" onClick={dismiss}>
          Skip
        </button>
      </div>
      <div className="gs-cards">
        <div className="gs-card-title">{card.title}</div>
        <div className="gs-card-text">{card.text}</div>
        <button
          className="gs-card-action"
          type="button"
          onClick={() => card.tab && onAction(card.tab)}
        >
          {card.action}
        </button>
      </div>
      <div className="gs-foot">
        <div className="gs-dots">
          {CARDS.map((_, i) => (
            <button
              key={i}
              type="button"
              aria-label={`Card ${i + 1}`}
              className={`gs-dot${i === index ? " active" : ""}`}
              onClick={() => setIndex(i)}
            />
          ))}
        </div>
        <div className="gs-nav">
          <button
            className="gs-nav-btn"
            type="button"
            aria-label="Previous"
            disabled={index === 0}
            onClick={() => setIndex((i) => i - 1)}
          >
            ←
          </button>
          <button
            className="gs-nav-btn next"
            type="button"
            onClick={() => (last ? dismiss() : setIndex((i) => i + 1))}
          >
            {last ? "Done" : "Next"}
          </button>
        </div>
      </div>
    </div>
  );
}
