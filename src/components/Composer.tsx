import { useRef, useState } from "react";
import { Paperclip, SendHorizontal, ShieldCheck } from "lucide-react";
import { Dropdown } from "./Dropdown";
import "./Composer.css";

// Permission modes are UI behaviour, not backend data. Models come from the
// provider config once that command exists — empty until then.
const MODES = [
  { value: "Auto", hint: "default" },
  { value: "Ask", hint: "confirm" },
  { value: "Manual", hint: "step" },
];
const MODELS: { value: string }[] = [];

export function Composer({ onNotify }: { onNotify: (msg: string) => void }) {
  const [text, setText] = useState("");
  const [mode, setMode] = useState(MODES[0].value);
  const [model, setModel] = useState<string | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);

  const grow = () => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 120)}px`;
  };

  const send = () => {
    if (!text.trim()) return;
    onNotify("Sending is not wired yet");
    setText("");
    requestAnimationFrame(grow);
  };

  return (
    <section className="composer">
      <textarea
        ref={area}
        rows={2}
        placeholder="Describe you task..."
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          grow();
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            send();
          }
        }}
      />
      <div className="composer-bar">
        <button className="iconbtn" type="button" title="Attach">
          <Paperclip size={15} />
        </button>
        <Dropdown
          title="Permission mode"
          label={
            <span className="mode-label">
              <ShieldCheck size={13} />
              {mode}
            </span>
          }
          value={mode}
          options={MODES}
          onPick={(v) => {
            setMode(v);
            if (v === "Ask") onNotify("Ask mode — agent will request approval");
          }}
        />
        <Dropdown
          title="Model"
          mono
          label={model ?? "no model"}
          value={model ?? ""}
          options={MODELS}
          emptyLabel="No models configured"
          onPick={setModel}
        />
        <button className="send" type="button" title="Send" onClick={send}>
          <SendHorizontal size={16} />
        </button>
      </div>
    </section>
  );
}
