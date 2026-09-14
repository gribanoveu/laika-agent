import { useState } from "react";
import { Sparkles } from "lucide-react";
import { useStaging } from "../hooks/useStaging";
import type { ChangedFile } from "../types";
import "./ContextPanel.css";

type Props = { onNotify: (msg: string) => void };

function StageRow({
  file,
  staged,
  onToggle,
  onShowDiff,
}: {
  file: ChangedFile;
  staged: boolean;
  onToggle: () => void;
  onShowDiff: () => void;
}) {
  return (
    <div className="stage-file">
      <button
        type="button"
        className={`stage-btn${staged ? " unstage" : ""}`}
        title={staged ? "Unstage" : "Stage"}
        onClick={onToggle}
      >
        {staged ? "−" : "+"}
      </button>
      <span className="stage-name" title="Show diff" onClick={onShowDiff}>
        {file.name}
      </span>
      <span className="stat">
        {file.add > 0 && <span className="add">+{file.add}</span>}
        {file.del > 0 && <span className="del">-{file.del}</span>}
      </span>
    </div>
  );
}

export function ContextPanel({ onNotify }: Props) {
  const { unstaged, staged, stage, unstage, stageAll } = useStaging();
  const [message, setMessage] = useState("");

  const canCommit = staged.length > 0 && message.trim().length > 0;

  return (
    <>
      <div className="panel-section">
        <div className="section-label">
          <span>Changes</span>
          {unstaged.length > 0 && (
            <button className="link-btn" type="button" onClick={stageAll}>
              Stage all
            </button>
          )}
        </div>
        {unstaged.map((f) => (
          <StageRow
            key={f.name}
            file={f}
            staged={false}
            onToggle={() => stage(f.name)}
            onShowDiff={() => onNotify("Diff view is not wired yet")}
          />
        ))}
        {unstaged.length === 0 && <div className="stage-empty">No unstaged changes</div>}
      </div>

      <div className="panel-section">
        <div className="section-label">
          <span>Staged</span>
          <span className="count">{staged.length}</span>
        </div>
        {staged.map((f) => (
          <StageRow
            key={f.name}
            file={f}
            staged
            onToggle={() => unstage(f.name)}
            onShowDiff={() => onNotify("Diff view is not wired yet")}
          />
        ))}
        {staged.length === 0 && <div className="stage-empty">Stage files to commit</div>}
      </div>

      <div className="panel-section">
        <div className="section-label">
          <span>Commit message</span>
        </div>
        <textarea
          className="commit-msg"
          rows={4}
          placeholder="Describe the commit…"
          value={message}
          onChange={(e) => setMessage(e.target.value)}
        />
        <div className="commit-actions">
          <button
            className="btn btn-ghost"
            type="button"
            disabled={staged.length === 0}
            onClick={() => onNotify("Message generation is not wired yet")}
          >
            <Sparkles size={13} />
            Generate description
          </button>
          <button
            className="btn btn-primary"
            type="button"
            disabled={!canCommit}
            onClick={() => onNotify("Commit is not wired yet")}
          >
            Commit
          </button>
        </div>
      </div>

      <div className="panel-section">
        <div className="section-label">
          <span>Session context</span>
        </div>
        <div className="empty" style={{ paddingTop: 0 }}>
          No repository open.
        </div>
      </div>
    </>
  );
}
