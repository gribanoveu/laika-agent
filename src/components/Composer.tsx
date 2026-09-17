import { useRef, useState } from "react";
import { Paperclip, SendHorizontal, Square, ShieldCheck, Bot } from "lucide-react";
import { Dropdown } from "./Dropdown";
import type { ConversationMode } from "../lib/chat";
import "./Composer.css";

// Permission modes are UI behaviour, not backend data. Models come from the
// provider config once that command exists — empty until then.
const MODES = [
  { value: "Auto", hint: "default" },
  { value: "Ask", hint: "confirm" },
  { value: "Manual", hint: "step" },
];
const MODELS: { value: string }[] = [];

// What the agent may be this turn. The labels are the user's words for it;
// the values are what `domain::conversation_mode` deserializes.
const CONVERSATIONS: { value: ConversationMode; label: string; hint: string }[] = [
  { value: "agent", label: "Agent", hint: "read, edit, run" },
  { value: "plan", label: "Plan", hint: "read only" },
  { value: "ask", label: "Ask", hint: "answer only" },
];

type Props = {
  /** Sends the box. While a turn is running the same box steers it instead. */
  onSend: (text: string) => void;
  onStop: () => void;
  running: boolean;
  onNotify: (msg: string) => void;
  /** Lives above this component: the backend has to be told, and a component
      does not call commands. */
  conversation: ConversationMode;
  onConversation: (mode: ConversationMode) => void;
};

export function Composer({
  onSend,
  onStop,
  running,
  onNotify,
  conversation,
  onConversation,
}: Props) {
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
    onSend(text);
    setText("");
    requestAnimationFrame(grow);
  };

  return (
    <section className="composer">
      <textarea
        ref={area}
        rows={2}
        placeholder={running ? "Add something while it works…" : "Describe you task..."}
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
        {/* Two chips because these are two questions. This one is what the
            agent is for; the one beside it is who has to agree before it
            acts — a plan-mode turn still asks, and an unattended agent still
            cannot write in Ask. */}
        <Dropdown
          title="What the agent may do"
          label={
            <span className="mode-label">
              <Bot size={13} />
              {CONVERSATIONS.find((c) => c.value === conversation)?.label ?? "Agent"}
            </span>
          }
          value={conversation}
          options={CONVERSATIONS.map((c) => ({ value: c.value, label: c.label, hint: c.hint }))}
          onPick={(v) => onConversation(v as ConversationMode)}
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
        {/* One button, two jobs: while a turn runs the only thing worth doing
            with it is stopping — and a send button that does nothing during a
            turn is worse than no button. */}
        {running ? (
          <button className="send stop" type="button" title="Stop" onClick={onStop}>
            <Square size={14} />
          </button>
        ) : (
          <button className="send" type="button" title="Send" onClick={send}>
            <SendHorizontal size={16} />
          </button>
        )}
      </div>
    </section>
  );
}
