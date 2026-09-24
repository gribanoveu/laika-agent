import { useEffect, useMemo, useState } from "react";
import { X } from "lucide-react";
import { useFileView } from "../hooks/useFileView";
import { fileRows, paintRows, type Painted } from "../lib/diffRows";
import { highlight, languageOf } from "../lib/highlight";
import type { FileSide, FileTarget } from "../lib/chat";
import { DiffView } from "./DiffView";
import { Tabs } from "./Tabs";
import "./FileViewer.css";

const SIDE: Record<FileSide, string | null> = { unstaged: "Unstaged", staged: "Staged", worktree: null };

// ponytail: past this the file stays uncoloured — Shiki's JavaScript engine
// takes seconds on a huge file; colour in chunks or a worker if that matters.
const MAX_COLOURED_CHARS = 300_000;

/** Both versions of a file coloured by its language, once Shiki has them; `null` until then or without one. */
function useColours(old: string | null, next: string | null, path: string) {
  const [colours, setColours] = useState<{ old: Painted | null; next: Painted | null } | null>(null);
  const lang = languageOf(path);
  useEffect(() => {
    setColours(null);
    if (!lang || (old?.length ?? 0) + (next?.length ?? 0) > MAX_COLOURED_CHARS) return;
    let live = true;
    const paint = async (text: string | null): Promise<Painted | null> => {
      const lines = text ? await highlight(text, lang) : null;
      return lines?.map((tokens) => tokens.map((t) => ({ text: t.content, style: t.htmlStyle as Record<string, string> }))) ?? null;
    };
    void Promise.all([paint(old), paint(next)]).then(([o, n]) => live && setColours({ old: o, next: n }));
    return () => {
      live = false;
    };
  }, [old, next, lang]);
  return colours;
}

/**
 * The column beside the chat that shows one file: what changed in it, or the
 * whole of it with the changes in place. Opened from Changes and Files; the
 * next file opened replaces it.
 */
export function FileViewer({
  target,
  workspace,
  onClose,
}: {
  target: FileTarget;
  workspace: string | null;
  onClose: () => void;
}) {
  const { view, error } = useFileView(target, workspace);
  const [mode, setMode] = useState<"diff" | "file">("diff");
  const text = view && !view.unviewable ? view : null;
  const changed = !!text && text.old !== text.new;
  // A file with nothing changed has only one way to be shown.
  const shown = changed ? mode : "file";
  const colours = useColours(text?.old ?? null, text?.new ?? null, target.path);
  const plain = useMemo(() => (text ? fileRows(text.old ?? "", text.new ?? "", shown === "file") : []), [text, shown]);
  const rows = useMemo(() => (colours ? paintRows(plain, colours.old, colours.next) : plain), [plain, colours]);
  const add = rows.filter((row) => row.kind === "add").length;
  const del = rows.filter((row) => row.kind === "del").length;

  const cut = target.path.lastIndexOf("/");
  const dir = cut > 0 ? target.path.slice(0, cut) : null;
  return (
    <section
      className="file-viewer"
      aria-label={target.path}
      onKeyDown={(e) => e.key === "Escape" && !e.defaultPrevented && onClose()}
    >
      <div className="file-viewer-head">
        <div className="file-viewer-title" title={target.path}>
          <span className="file-viewer-name">{target.path.slice(cut + 1)}</span>
          {dir && (
            <span className="file-viewer-dir">
              <bdi>{dir}</bdi>
            </span>
          )}
        </div>
        {SIDE[target.side] && <span className="file-viewer-side">{SIDE[target.side]}</span>}
        <span className="file-viewer-stat">
          {add > 0 && <span className="add">+{add}</span>}
          {del > 0 && <span className="del">-{del}</span>}
        </span>
        <Tabs
          label="Show"
          value={shown}
          onChange={setMode}
          tabs={[
            { id: "diff", label: "Diff", disabled: !changed, title: changed ? undefined : "No changes" },
            { id: "file", label: "File" },
          ]}
        />
        <button type="button" className="iconbtn" title="Close" onClick={onClose}>
          <X size={14} />
        </button>
      </div>
      <div className="file-viewer-body">
        {error ? (
          <div className="file-viewer-note">{error}</div>
        ) : !view ? null : view.unviewable === "binary" ? (
          <div className="file-viewer-note">A binary file — not shown.</div>
        ) : view.unviewable === "tooLarge" ? (
          <div className="file-viewer-note">Too large to show here.</div>
        ) : view.old === null && view.new === null ? (
          <div className="file-viewer-note">Not on disk.</div>
        ) : rows.length === 0 ? (
          <div className="file-viewer-note">Empty file.</div>
        ) : (
          <DiffView rows={rows} />
        )}
      </div>
    </section>
  );
}
