import { useState } from "react";
import { Minus, Plus, Sparkles } from "lucide-react";
import { useStaging } from "../hooks/useStaging";
import type { ChangedFile } from "../lib/chat";
import "./ChangesPanel.css";

type Props = {
  /** On screen: the status is read only while it is. */
  active: boolean;
  /** The open folder, as the backend's events name it. */
  workspace: string | null;
  onNotify: (msg: string) => void;
  /** The commit message, held above the panel: it outlives the panel being closed or moved. */
  message: string;
  onMessage: (message: string) => void;
};

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
  // The name leads; the folder from the repository root sits under it.
  const cut = file.path.lastIndexOf("/");
  const name = file.path.slice(cut + 1);
  const dir = cut > 0 ? file.path.slice(0, cut) : null;
  return (
    <div className="stage-file">
      <button
        type="button"
        className={`stage-btn${staged ? " unstage" : ""}`}
        title={staged ? "Unstage" : "Stage"}
        onClick={onToggle}
      >
        {staged ? <Minus size={12} /> : <Plus size={12} />}
      </button>
      <span className="stage-label" title={`${file.path} — show diff`} onClick={onShowDiff}>
        <span className="stage-name">{name}</span>
        {dir && (
          <span className="stage-dir">
            <bdi>{dir}</bdi>
          </span>
        )}
      </span>
      <span className="stat">
        {file.add > 0 && <span className="add">+{file.add}</span>}
        {file.del > 0 && <span className="del">-{file.del}</span>}
      </span>
    </div>
  );
}

export function ChangesPanel({ active, workspace, onNotify, message, onMessage }: Props) {
  const { unstaged, staged, error, stage, unstage, commit } = useStaging(active, workspace);
  const [committing, setCommitting] = useState(false);

  const canCommit = staged.length > 0 && message.trim().length > 0 && !committing;
  // A failed action is said once, in a toast; the lists are read back either way.
  const run = (op: Promise<unknown>) => op.catch((e) => onNotify(String(e)));
  const commitNow = async () => {
    setCommitting(true);
    try {
      const id = await commit(message);
      onMessage("");
      onNotify(`Committed ${id}`);
    } catch (e) {
      onNotify(String(e));
    } finally {
      setCommitting(false);
    }
  };

  if (error)
    return (
      <div className="panel-section">
        <div className="stage-empty">{error}</div>
      </div>
    );

  return (
    <div className="changes-panel">
      <div className="panel-section stage-section">
        <div className="section-label">
          <span>Changes</span>
          {unstaged.length > 0 && (
            <button className="link-btn" type="button" onClick={() => run(stage(unstaged.map((f) => f.path)))}>
              Stage all
            </button>
          )}
        </div>
        <div className="stage-list">
          {unstaged.map((f) => (
            <StageRow
              key={f.path}
              file={f}
              staged={false}
              onToggle={() => run(stage([f.path]))}
              onShowDiff={() => onNotify("Diff view is not wired yet")}
            />
          ))}
          {unstaged.length === 0 && <div className="stage-empty">No unstaged changes</div>}
        </div>
      </div>

      <div className="panel-section stage-section">
        <div className="section-label">
          <span>Staged</span>
          <span className="count">{staged.length}</span>
        </div>
        <div className="stage-list">
          {staged.map((f) => (
            <StageRow
              key={f.path}
              file={f}
              staged
              onToggle={() => run(unstage([f.path]))}
              onShowDiff={() => onNotify("Diff view is not wired yet")}
            />
          ))}
          {staged.length === 0 && <div className="stage-empty">Stage files to commit</div>}
        </div>
      </div>

      <div className="panel-section commit-section">
        <div className="section-label">
          <span>Commit message</span>
        </div>
        <textarea
          className="commit-msg"
          rows={4}
          placeholder="Describe the commit…"
          value={message}
          onChange={(e) => onMessage(e.target.value)}
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
            onClick={commitNow}
          >
            Commit
          </button>
        </div>
      </div>

    </div>
  );
}
