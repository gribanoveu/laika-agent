import { memo, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { diffRows, type DiffRow } from "../lib/diffRows";
import "./DiffView.css";

const SIGN = { context: " ", add: "+", del: "-" } as const;

/** Rows drawn beyond each edge of the view, so a fast scroll finds them already there. */
const OVERSCAN = 60;
/** A tab as the viewer draws it, for how wide the longest line is. */
const TAB = 4;

const Row = memo(function Row({ row, style }: { row: DiffRow; style?: CSSProperties }) {
  if (!("parts" in row)) {
    return (
      <div className={`diff-row diff-${row.kind}`} style={style}>
        {row.text}
      </div>
    );
  }
  return (
    <div className={`diff-row diff-${row.kind}`} style={style}>
      <span className="diff-no">{row.oldNo ?? ""}</span>
      <span className="diff-no">{row.newNo ?? ""}</span>
      <span className="diff-sign">{SIGN[row.kind]}</span>
      <span className="diff-text">
        {row.parts.map((part, j) => {
          const colour = part.style as CSSProperties | undefined;
          if (part.changed) return <mark key={j} style={colour}>{part.text}</mark>;
          return colour ? <span key={j} style={colour}>{part.text}</span> : part.text;
        })}
      </span>
    </div>
  );
});

/**
 * Only the rows in view, and a margin either side, placed where they would
 * be: every row is one line tall — long lines scroll sideways rather than
 * wrap — so where each one goes is arithmetic, and nothing is guessed and
 * corrected while scrolling.
 */
function VirtualRows({ rows }: { rows: DiffRow[] }) {
  const box = useRef<HTMLDivElement>(null);
  const [top, setTop] = useState(0);
  const [view, setView] = useState({ height: 0, row: 0 });

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    // Measured, not assumed: the line height follows the font-size setting.
    const measure = () => setView({ height: el.clientHeight, row: Math.ceil(parseFloat(getComputedStyle(el).lineHeight)) || 18 });
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, []);

  // Wide enough for the longest line, so the tint of a changed row reaches
  // across however far the view is scrolled sideways.
  const longest = useMemo(
    () => rows.reduce((most, row) => Math.max(most, ("parts" in row ? row.parts.map((p) => p.text).join("") : row.text).replace(/\t/g, " ".repeat(TAB)).length), 0),
    [rows],
  );

  const first = view.row ? Math.max(0, Math.floor(top / view.row) - OVERSCAN) : 0;
  const last = view.row ? Math.min(rows.length, Math.ceil((top + view.height) / view.row) + OVERSCAN) : 0;
  return (
    <div ref={box} className="diff-scroll" onScroll={(e) => setTop(e.currentTarget.scrollTop)}>
      <div
        className="diff-rows"
        style={{ height: rows.length * view.row, minWidth: `calc(${longest}ch + 2 * var(--diff-digits) + 7ch)` }}
      >
        {rows.slice(first, last).map((row, i) => (
          <Row key={first + i} row={row} style={{ top: (first + i) * view.row, height: view.row }} />
        ))}
      </div>
    </div>
  );
}

/**
 * A unified diff drawn as lines with both line numbers, and within an edited
 * line the words that changed — or rows already made, as the file viewer has.
 * Scrolls inside itself, so a long diff does not push away whatever sits
 * under it. `virtual` draws only the rows in view, for a file thousands of
 * lines long; its lines do not wrap.
 */
export function DiffView({ unified, rows: given, virtual }: { unified?: string; rows?: DiffRow[]; virtual?: boolean }) {
  const parsed = useMemo(() => (given ? null : diffRows(unified ?? "")), [unified, given]);
  const rows = given ?? parsed ?? [];

  // Nothing the parser could read: the text as it came, still better than blank.
  if (!given && rows.length === 0) return <pre className="diff-view diff-view-raw">{unified}</pre>;

  // Every row's numbers as wide as the longest, so the text lines up.
  const digits = String(rows.reduce((most, row) => ("parts" in row ? Math.max(most, row.oldNo ?? 0, row.newNo ?? 0) : most), 0)).length;
  // Nothing added or removed — a file shown whole, unchanged: the two number
  // columns would repeat each other, and the sign column would stay empty.
  const plain = rows.every((row) => row.kind !== "add" && row.kind !== "del");
  return (
    <div
      className={`diff-view${plain ? " diff-view-plain" : ""}${virtual ? " diff-view-virtual" : ""}`}
      style={{ "--diff-digits": `${Math.max(3, digits)}ch` } as CSSProperties}
    >
      {virtual ? <VirtualRows rows={rows} /> : rows.map((row, i) => <Row key={i} row={row} />)}
    </div>
  );
}
