import { useRef, useState } from "react";
import { Paperclip, SendHorizontal, ShieldCheck } from "lucide-react";
import { Dropdown } from "./Dropdown";
import { MODELS, MODES } from "../mock/data";
import "./Composer.css";

export function Composer({ onNotify }: { onNotify: (msg: string) => void }) {
  const [text, setText] = useState("");
  const [mode, setMode] = useState<string>(MODES[0]);
  const [model, setModel] = useState(MODELS[0]);
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
        <button className="iconbtn" type="button" title="Прикрепить">
          <Paperclip size={15} />
        </button>
        <Dropdown
          title="Режим разрешений"
          label={
            <span className="mode-label">
              <ShieldCheck size={13} />
              {mode}
            </span>
          }
          value={mode}
          options={[
            { value: "Auto", hint: "default" },
            { value: "Ask", hint: "confirm" },
            { value: "Manual", hint: "step" },
          ]}
          onPick={(v) => {
            setMode(v);
            if (v === "Ask") onNotify("Ask mode — agent will request approval");
          }}
        />
        <Dropdown
          title="Модель"
          mono
          label={model}
          value={model}
          options={MODELS.map((value) => ({ value }))}
          onPick={setModel}
        />
        <button className="send" type="button" title="Отправить" onClick={send}>
          <SendHorizontal size={16} />
        </button>
      </div>
    </section>
  );
}
