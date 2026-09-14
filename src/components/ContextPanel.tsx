import { useState } from "react";
import { Sparkles } from "lucide-react";
import { useStaging } from "../hooks/useStaging";
import { DIFFS, GENERATED_COMMIT_MESSAGE, SESSION } from "../mock/data";
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
  const { unstaged, staged, stage, unstage, stageAll, clearStaged } = useStaging();
  const [diff, setDiff] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [generating, setGenerating] = useState(false);
  const [committed, setCommitted] = useState<{ hash: string; subject: string } | null>(null);

  const canCommit = staged.length > 0 && message.trim().length > 0;

  const generate = () => {
    setGenerating(true);
    setCommitted(null);
    // ponytail: fake latency stands in for the LLM call; replace with the invoke wrapper.
    setTimeout(() => {
      setMessage(GENERATED_COMMIT_MESSAGE);
      setGenerating(false);
    }, 700);
  };

  const commit = () => {
    if (!canCommit) return;
    setCommitted({ hash: "a3f9c2e", subject: message.trim().split("\n")[0] });
    clearStaged();
    setMessage("");
    setDiff(null);
    onNotify("Commit created");
  };

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
            onShowDiff={() => setDiff(f.name)}
          />
        ))}
        {unstaged.length === 0 && <div className="stage-empty">No unstaged changes</div>}
        {diff && DIFFS[diff] && (
          <div>
            <div className="diff-preview-head">
              <span className="diff-preview-name">{diff}</span>
              <button className="link-btn" type="button" onClick={() => setDiff(null)}>
                Close
              </button>
            </div>
            <pre className="diff-preview">
              {DIFFS[diff].split("\n").map((line, i) => (
                <div key={i} className={line.startsWith("+") ? "ln-add" : line.startsWith("-") ? "ln-del" : ""}>
                  {line}
                </div>
              ))}
            </pre>
          </div>
        )}
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
            onShowDiff={() => setDiff(f.name)}
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
            className={`btn btn-ghost${generating ? " loading" : ""}`}
            type="button"
            disabled={staged.length === 0 || generating}
            onClick={generate}
          >
            <Sparkles size={13} />
            Generate description
          </button>
          <button className="btn btn-primary" type="button" disabled={!canCommit} onClick={commit}>
            Commit
          </button>
        </div>
        <div className="commit-status">
          {committed && (
            <>
              Committed {committed.hash} · {committed.subject}
              <button
                className="push-link"
                type="button"
                onClick={() => onNotify(`Pushed to origin/${SESSION.branch}`)}
              >
                Push
              </button>
            </>
          )}
        </div>
      </div>

      <div className="panel-section">
        <div className="section-label">
          <span>Session context</span>
        </div>
        <div className="empty" style={{ paddingTop: 0 }}>
          Branch <code className="mono">{SESSION.branch}</code>, repo root indexed.
        </div>
      </div>
    </>
  );
}
