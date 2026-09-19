import { useEffect, useRef, useState } from "react";
import { SendHorizontal, Square, ShieldCheck, Bot } from "lucide-react";
import { Dropdown } from "./Dropdown";
import { ContextMeter } from "./ContextMeter";
import type { ChatUsage, ContextUsage, ConversationMode } from "../lib/chat";
import { choiceKey, type ModelChoice } from "../hooks/useLlmSettings";
import "./Composer.css";

// Two permission states, not the prototype's three, because two is what
// `ApprovalPolicy` has: ask before anything that changes the tree, or run the
// whole turn without asking. A third label would be a control that moves and
// changes nothing. "Always allow this tool" is the third real state and it is
// not a chip — it is answered on the card, about one tool, in the moment.
const MODES: { value: string; label: string; hint: string }[] = [
  { value: "ask", label: "Ask", hint: "Confirm edits and commands" },
  { value: "auto", label: "Auto", hint: "Run the whole turn without asking" },
];

// What the agent may be this turn. The labels are the user's words for it;
// the values are what `domain::conversation_mode` deserializes.
const CONVERSATIONS: { value: ConversationMode; label: string; hint: string }[] = [
  { value: "agent", label: "Agent", hint: "Reads, edits files and runs commands" },
  { value: "plan", label: "Plan", hint: "Reads only and writes a plan" },
  { value: "ask", label: "Ask", hint: "Answers questions, changes nothing" },
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
  /** Text put into the box from outside — a branch hands back its message.
      Replaces what was typed; `seq` makes the same text twice land twice. */
  draft?: { text: string; seq: number } | null;
  /** Every configured `provider/model`, and the one turns go to now. */
  models: { choices: ModelChoice[]; current: ModelChoice | null };
  onModel: (choice: ModelChoice) => void;
  /** Asks the providers what they serve; called when the model menu opens. */
  onLoadModels: () => void;
  /** What the next request will cost, as the backend's own estimate — the one
      that decides when a conversation is folded. */
  context: ContextUsage | null;
  usage: ChatUsage | null;
  onCompact: () => void;
};

export function Composer({
  onSend,
  onStop,
  running,
  conversation,
  onConversation,
  unattended,
  onUnattended,
  draft = null,
  models,
  onModel,
  onLoadModels,
  context,
  usage,
  onCompact,
}: Props) {
  const [text, setText] = useState("");
  const area = useRef<HTMLTextAreaElement>(null);

  const grow = () => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 120)}px`;
  };

  useEffect(() => {
    if (!draft) return;
    setText(draft.text);
    area.current?.focus();
    requestAnimationFrame(grow);
  }, [draft]);

  const send = () => {
    if (!text.trim()) return;
    onSend(text);
    setText("");
    requestAnimationFrame(grow);
  };

  return (
    <section className="composer">
      <textarea
        className="chat-text"
        ref={area}
        rows={2}
        placeholder={running ? "Add something while it works…" : "Describe your task…"}
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
        <Dropdown
          title="Permission mode"
          heading="Permissions"
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
          heading="Mode"
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
          heading="Model"
          label={<span className="model-label">{models.current?.label ?? "no model"}</span>}
          value={models.current ? choiceKey(models.current) : ""}
          options={models.choices.map((choice) => ({
            value: choiceKey(choice),
            label: choice.label,
            hint: choice.model ? undefined : "The first model the provider lists",
          }))}
          emptyLabel="No provider yet — add one in Settings → Models"
          onOpen={onLoadModels}
          onPick={(key) => {
            const choice = models.choices.find((c) => choiceKey(c) === key);
            if (choice) onModel(choice);
          }}
        />
        {/* Shown from the first render, before anything has been said: an
            empty conversation already costs the prompt and the schemas. */}
        {context && (
          <span className="composer-meter">
            <ContextMeter context={context} usage={usage} running={running} onCompact={onCompact} up />
          </span>
        )}
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
