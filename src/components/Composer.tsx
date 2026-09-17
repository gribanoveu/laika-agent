import { useRef, useState } from "react";
import { Paperclip, SendHorizontal, Square, ShieldCheck, Bot } from "lucide-react";
import { Dropdown } from "./Dropdown";
import type { ConversationMode } from "../lib/chat";
import "./Composer.css";

// Models come from the provider config once that command exists — empty
// until then.
//
// Two permission states, not the prototype's three, because two is what
// `ApprovalPolicy` has: ask before anything that changes the tree, or run the
// whole turn without asking. A third label would be a control that moves and
// changes nothing. "Always allow this tool" is the third real state and it is
// not a chip — it is answered on the card, about one tool, in the moment.
const MODES: { value: string; label: string; hint: string }[] = [
  { value: "ask", label: "Ask", hint: "confirm changes" },
  { value: "auto", label: "Auto", hint: "never ask" },
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
  /** Both chips live above this component: the backend has to be told, and a
      component does not call commands. */
  conversation: ConversationMode;
  onConversation: (mode: ConversationMode) => void;
  /** `true` when the turn runs without asking — `ApprovalPolicy::skip_all`. */
  unattended: boolean;
  onUnattended: (unattended: boolean) => void;
};

export function Composer({
  onSend,
  onStop,
  running,
  conversation,
  onConversation,
  unattended,
  onUnattended,
}: Props) {
  const [text, setText] = useState("");
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
              {unattended ? "Auto" : "Ask"}
            </span>
          }
          value={unattended ? "auto" : "ask"}
          options={MODES}
          onPick={(v) => onUnattended(v === "auto")}
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
