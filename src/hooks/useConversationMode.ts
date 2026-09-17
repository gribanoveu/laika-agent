import { useCallback, useState } from "react";
import { setConversationMode, type ConversationMode } from "../lib/chat";

/**
 * Which mode the chip shows, and the one rule about changing it.
 *
 * The backend is where the mode is enforced — it decides which tools reach the
 * model and refuses the rest. This holds only what the chip reads back, and it
 * moves *after* the backend has agreed: a chip saying "Plan" over a turn that
 * is still armed is the one failure worth guarding here.
 *
 * Returns the reason on failure rather than raising or toasting: the hook does
 * not know where this app puts its errors.
 */
export function useConversationMode() {
  const [mode, setMode] = useState<ConversationMode>("agent");

  const pick = useCallback(async (next: ConversationMode): Promise<string | null> => {
    try {
      await setConversationMode(next);
      setMode(next);
      return null;
    } catch (e) {
      return String(e);
    }
  }, []);

  return { mode, pick };
}
