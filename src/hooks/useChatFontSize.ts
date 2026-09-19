import { useEffect, useState } from "react";

/** The chat's font size, as a factor on the type scale inside `.chat-text` (src/styles/tokens.css). */
export const FONT_SIZES = { small: 0.92, medium: 1, large: 1.1, larger: 1.2 } as const;
export type FontSize = keyof typeof FONT_SIZES;

const KEY = "atlas-cli-chat-font-size";

export function useChatFontSize() {
  const [size, setSize] = useState<FontSize>(() => {
    const stored = localStorage.getItem(KEY);
    return stored && stored in FONT_SIZES ? (stored as FontSize) : "large";
  });

  useEffect(() => {
    localStorage.setItem(KEY, size);
    document.documentElement.style.setProperty("--chat-font-scale", String(FONT_SIZES[size]));
  }, [size]);

  return { size, setSize };
}
