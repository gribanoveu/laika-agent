import { useState } from "react";
import type { AsideTab } from "../types";
import "./GettingStarted.css";

const KEY = "atlas-cli-getting-started-skipped";

// Onboarding copy is UI text, not backend data — it lives with the component.
// `target` is a pane to open, or the settings dialog — it opens on Models.
const CARDS: { title: string; text: string; action: string; target: AsideTab | "settings" }[] = [
  {
    title: "Add a provider",
    text: "Connect an LLM provider and pick a model: the agent has nothing to answer with until then.",
    action: "Add a provider →",
    target: "settings",
  },
  {
    title: "Connect MCP",
    text: "Connect MCP servers for docs, git and whatever else the agent should reach.",
    action: "Open MCP settings →",
    target: "mcp",
  },
  {
    title: "Stage and commit",
    text: "Stage what the agent changed and commit it from the panel on the right.",
    action: "Open Changes panel →",
    target: "changes",
  },
];

export function GettingStarted({
  onAction,
  onOpenSettings,
}: {
  onAction: (tab: AsideTab) => void;
  onOpenSettings: () => void;
}) {
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
          onClick={() => (card.target === "settings" ? onOpenSettings() : onAction(card.target))}
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
