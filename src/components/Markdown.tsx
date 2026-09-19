import { isValidElement, memo, useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ThemedToken } from "shiki";
import { Streamdown, useIsCodeFenceIncomplete, type Components } from "streamdown";
import { highlight, splitLines } from "../lib/highlight";
import { wrapAsciiTrees } from "../lib/wrapAsciiTrees";
import "./Markdown.css";

/** A fenced block's text, back out of the React children Streamdown made of it. */
function textOf(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return textOf(node.props.children);
  return "";
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const label = copied ? "Copied" : "Copy code";
  return (
    <button
      type="button"
      className={`md-copy${copied ? " copied" : ""}`}
      title={label}
      aria-label={label}
      onClick={() => {
        // A clipboard that refuses loses nothing: the code is on screen.
        navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          clearTimeout(timer.current);
          timer.current = setTimeout(() => setCopied(false), 1500);
        }, () => {});
      }}
    >
      {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
    </button>
  );
}

/** A fenced block: numbered lines, colours once the fence is closed — so a
 * streaming block stays plain and synchronous instead of re-highlighting on
 * every delta. Without a language tag it is inline code. */
function Code({ className, children }: { className?: string; children?: ReactNode }) {
  const lang = /language-(\S+)/.exec(className ?? "")?.[1]?.toLowerCase() ?? null;
  const source = lang === null ? "" : textOf(children);
  const incomplete = useIsCodeFenceIncomplete();
  const [tokens, setTokens] = useState<ThemedToken[][] | null>(null);

  useEffect(() => {
    setTokens(null);
    if (lang === null || incomplete) return;
    let live = true;
    void highlight(source, lang).then((result) => live && setTokens(result));
    return () => {
      live = false;
    };
  }, [source, lang, incomplete]);

  if (lang === null) return <code className="md-code-inline">{children}</code>;

  return (
    <div className="md-code" data-lang={lang}>
      <CopyButton text={source} />
      <div className="md-code-scroll">
        <div className="md-code-body">
        {splitLines(source).map((line, index) => (
          <div key={index} className="md-code-line">
            <span className="md-code-gutter" aria-hidden>
              {index + 1}
            </span>
            <code className="md-code-text">
              {tokens?.[index]?.length
                ? tokens[index].map((token, i) => (
                    <span key={i} style={token.htmlStyle}>
                      {token.content}
                    </span>
                  ))
                : line || " "}
            </code>
          </div>
        ))}
        </div>
      </div>
    </div>
  );
}

// Streamdown's own elements lean on Tailwind classes, and this app has no
// Tailwind: every element it draws gets a plain class styled in Markdown.css.
const components: Components = {
  h1: ({ children }) => <h1 className="md-h1">{children}</h1>,
  h2: ({ children }) => <h2 className="md-h2">{children}</h2>,
  h3: ({ children }) => <h3 className="md-h3">{children}</h3>,
  h4: ({ children }) => <h4 className="md-h4">{children}</h4>,
  h5: ({ children }) => <h5 className="md-h4">{children}</h5>,
  h6: ({ children }) => <h6 className="md-h4">{children}</h6>,
  p: ({ children }) => <p className="md-p">{children}</p>,
  strong: ({ children }) => <strong className="md-strong">{children}</strong>,
  em: ({ children }) => <em>{children}</em>,
  ul: ({ children }) => <ul className="md-ul">{children}</ul>,
  ol: ({ children }) => <ol className="md-ol">{children}</ol>,
  li: ({ children }) => <li className="md-li">{children}</li>,
  blockquote: ({ children }) => <blockquote className="md-quote">{children}</blockquote>,
  table: ({ children }) => (
    <div className="md-table-scroll">
      <table className="md-table">{children}</table>
    </div>
  ),
  thead: ({ children }) => <thead>{children}</thead>,
  tbody: ({ children }) => <tbody>{children}</tbody>,
  tr: ({ children }) => <tr>{children}</tr>,
  th: ({ children }) => <th className="md-th">{children}</th>,
  td: ({ children }) => <td className="md-td">{children}</td>,
  hr: () => <hr className="md-hr" />,
  sup: ({ children }) => <sup>{children}</sup>,
  sub: ({ children }) => <sub>{children}</sub>,
  // The CSP lets no outside image load; alt text is what is left of one.
  img: ({ src, alt }) => <img className="md-img" src={typeof src === "string" ? src : undefined} alt={alt} />,
  // Opened in the system browser, never in this window. The address is the
  // tooltip: a link the model wrote can carry anything in its query string,
  // and the user should see where a click sends it.
  a: ({ href, children }) => (
    <a
      href={href}
      title={href}
      className="md-link"
      onClick={(event) => {
        event.preventDefault();
        if (href) void openUrl(href);
      }}
    >
      {children}
    </a>
  ),
  pre: ({ children }) => <>{children}</>,
  code: ({ className, children }) => <Code className={className}>{children}</Code>,
};

/**
 * The model's answer as Markdown.
 *
 * `streaming` turns on Streamdown's repair of half-written markup, and only
 * while the text is still arriving: the repair bets the next delta finishes
 * the markup and drops text to make the bet — everything after an unmatched
 * `![`, the bracket of an unmatched `[`. Mid-stream that costs a frame; on a
 * finished answer it would lose what the model wrote (`arr[0` is ordinary).
 *
 * `memo`: every delta re-renders the transcript, and past answers must not
 * re-parse and re-highlight for a token that changed only the last one.
 */
export const Markdown = memo(function Markdown({ text, streaming }: { text: string; streaming: boolean }) {
  return (
    <Streamdown
      className="md"
      isAnimating={streaming}
      parseIncompleteMarkdown={streaming}
      linkSafety={{ enabled: false }}
      components={components}
    >
      {wrapAsciiTrees(text)}
    </Streamdown>
  );
});
