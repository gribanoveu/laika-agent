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

/** A row's text as it takes up columns: a tab is TAB wide, as drawn. */
const width = (row: DiffRow) =>
  ("parts" in row ? row.parts.map((p) => p.text).join("") : row.text).replace(/\t/g, " ".repeat(TAB)).length;

/**
 * Where each row starts, in lines from the top, and — last — how many lines
 * all of them take. Unwrapped every row is one line. Wrapped, a row of code
 * takes as many lines as `cols` characters fit into its text, and a hunk
 * header as many as `headerCols` do: the font is monospaced and lines break
 * at any character, so this is arithmetic, not a measurement.
 */
export function rowStarts(rows: DiffRow[], wrap: { cols: number; headerCols: number } | null): number[] {
  const starts = new Array<number>(rows.length + 1);
  starts[0] = 0;
  rows.forEach((row, i) => {
    const cols = "parts" in row ? wrap?.cols : wrap?.headerCols;
    starts[i + 1] = starts[i] + (cols ? Math.max(1, Math.ceil(width(row) / cols)) : 1);
  });
  return starts;
}

/** The last row starting at or before `line`. */
function rowAt(starts: number[], line: number) {
  let [lo, hi] = [0, starts.length - 2];
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= line) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/**
 * Only the rows in view, and a margin either side, placed where they would
 * be. Unwrapped, every row is one line — long lines scroll sideways — and,
 * wrapped, how many lines each takes is worked out from the width, so where
 * each row goes is known before it is drawn, and nothing is guessed and
 * corrected while scrolling.
 */
function VirtualRows({ rows, wrap, digits, plain }: { rows: DiffRow[]; wrap: boolean; digits: number; plain: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const [top, setTop] = useState(0);
  const [view, setView] = useState({ height: 0, width: 0, row: 0, char: 0 });

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    // Measured, not assumed: the line height and the character width follow
    // the font-size setting, and the font may arrive after the first draw.
    const measure = () => {
      const probe = document.createElement("span");
      probe.textContent = "0".repeat(64);
      probe.style.cssText = "position:absolute;visibility:hidden;white-space:pre";
      el.append(probe);
      const char = probe.getBoundingClientRect().width / 64;
      probe.remove();
      setView({
        height: el.clientHeight,
        width: el.clientWidth,
        row: Math.ceil(parseFloat(getComputedStyle(el).lineHeight)) || 18,
        char,
      });
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    void document.fonts?.ready.then(measure);
    return () => watch.disconnect();
  }, []);

  // The columns a wrapped line gets: the width less the number columns, the
  // sign and the text's own padding, as DiffView.css lays them out.
  const cols = useMemo(() => {
    if (!wrap || !view.char) return null;
    const gutter = plain ? digits * view.char + 12 + 1.5 * view.char : 2 * (digits * view.char + 12) + 1.5 * view.char;
    return {
      cols: Math.max(1, Math.floor((view.width - gutter - 9) / view.char)),
      headerCols: Math.max(1, Math.floor((view.width - 18) / view.char)),
    };
  }, [wrap, view.char, view.width, digits, plain]);
  const starts = useMemo(() => rowStarts(rows, cols), [rows, cols]);
  // Unwrapped, wide enough for the longest line, so the tint of a changed row
  // reaches across however far the view is scrolled sideways.
  const longest = useMemo(() => (wrap ? 0 : rows.reduce((most, row) => Math.max(most, width(row)), 0)), [rows, wrap]);

  const line = view.row ? top / view.row : 0;
  const first = view.row ? Math.max(0, rowAt(starts, line) - OVERSCAN) : 0;
  const last = view.row ? Math.min(rows.length, rowAt(starts, line + view.height / view.row) + 1 + OVERSCAN) : 0;
  return (
    <div
      ref={box}
      className="diff-scroll"
      style={cols ? ({ "--wrap-cols": cols.cols } as CSSProperties) : undefined}
      onScroll={(e) => setTop(e.currentTarget.scrollTop)}
    >
      <div
        className="diff-rows"
        style={{
          height: starts[rows.length] * view.row,
          minWidth: wrap ? undefined : `calc(${longest}ch + 2 * var(--diff-digits) + 7ch)`,
        }}
      >
        {rows.slice(first, last).map((row, i) => {
          const at = first + i;
          return (
            <Row
              key={at}
              row={row}
              style={{ top: starts[at] * view.row, height: (starts[at + 1] - starts[at]) * view.row }}
            />
          );
        })}
      </div>
    </div>
  );
}

/**
 * A unified diff drawn as lines with both line numbers, and within an edited
 * line the words that changed — or rows already made, as the file viewer has.
 * Scrolls inside itself, so a long diff does not push away whatever sits
 * under it. `virtual` draws only the rows in view, for a file thousands of
 * lines long; its lines wrap only when asked.
 */
export function DiffView({
  unified,
  rows: given,
  virtual,
  wrap = false,
}: {
  unified?: string;
  rows?: DiffRow[];
  virtual?: boolean;
  /** With `virtual`: long lines wrap, breaking at any character. */
  wrap?: boolean;
}) {
  const parsed = useMemo(() => (given ? null : diffRows(unified ?? "")), [unified, given]);
  const rows = given ?? parsed ?? [];

  // Nothing the parser could read: the text as it came, still better than blank.
  if (!given && rows.length === 0) return <pre className="diff-view diff-view-raw">{unified}</pre>;

  // Every row's numbers as wide as the longest, so the text lines up.
  const digits = Math.max(3, String(rows.reduce((most, row) => ("parts" in row ? Math.max(most, row.oldNo ?? 0, row.newNo ?? 0) : most), 0)).length);
  // Nothing added or removed — a file shown whole, unchanged: the two number
  // columns would repeat each other, and the sign column would stay empty.
  const plain = rows.every((row) => row.kind !== "add" && row.kind !== "del");
  return (
    <div
      className={`diff-view${plain ? " diff-view-plain" : ""}${virtual ? " diff-view-virtual" : ""}${virtual && wrap ? " diff-view-wrap" : ""}`}
      style={{ "--diff-digits": `${digits}ch` } as CSSProperties}
    >
      {virtual ? (
        <VirtualRows rows={rows} wrap={wrap} digits={digits} plain={plain} />
      ) : (
        rows.map((row, i) => <Row key={i} row={row} />)
      )}
    </div>
  );
}
