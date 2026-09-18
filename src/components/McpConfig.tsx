import { useEffect, useState } from "react";
import type { McpView } from "../lib/chat";
import "./McpConfig.css";

// The whole file in one box, because the format is the one every MCP server's
// README already gives: pasting that snippet is the fastest way to add one,
// and a field-by-field form would have to be translated back into it.

type Props = {
  view: McpView | null;
  error: string | null;
  /** Resolves to whether it was stored. */
  onSave: (text: string) => Promise<boolean>;
  onClose: () => void;
};

export function McpConfig({ view, error, onSave, onClose }: Props) {
  const [draft, setDraft] = useState(view?.text ?? "");
  // Follows the file until the user starts typing, so the box is never a
  // stale copy of what is on disk.
  const [touched, setTouched] = useState(false);
  useEffect(() => {
    if (!touched) setDraft(view?.text ?? "");
  }, [view, touched]);

  const save = async () => {
    if (await onSave(draft)) {
      setTouched(false);
      onClose();
    }
  };

  return (
    <div className="mcp-config">
      <textarea
        className="mcp-config-text"
        aria-label="MCP configuration"
        spellCheck={false}
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          setTouched(true);
        }}
      />
      {error && <p className="modal-note mcp-config-error">{error}</p>}
      <p className="modal-note">
        The <code>mcpServers</code> format of Claude Desktop and Cursor: paste a server's snippet as it is.
        Optional per server: <code>weight</code> (cost of a call in the turn's budget, 3 by default) and{" "}
        <code>timeoutSecs</code> (120). Kept in {view?.path || "the app directory"}, readable only by you —
        it may hold tokens.
      </p>
      <div className="mcp-config-actions">
        <button className="btn btn-primary" type="button" onClick={save}>
          Save
        </button>
        <button className="btn btn-ghost" type="button" onClick={onClose}>
          Cancel
        </button>
      </div>
    </div>
  );
}
