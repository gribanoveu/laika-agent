import { useCallback, useEffect, useState } from "react";
import { mcpConfig, saveMcpConfig, setMcpServerEnabled, type McpView } from "../lib/chat";

/**
 * The MCP configuration, re-read whenever the tab or the editor opens: the
 * file is the user's, and may have been edited outside the app.
 */
export function useMcp(visible: boolean) {
  const [view, setView] = useState<McpView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setView(await mcpConfig());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (visible) reload();
  }, [visible, reload]);

  /** Resolves to whether it was stored; a refusal is left in `error` for the editor to show. */
  const save = useCallback(async (text: string) => {
    try {
      setView(await saveMcpConfig(text));
      setError(null);
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  }, []);

  const setEnabled = useCallback(async (name: string, enabled: boolean) => {
    setView((v) => v && { ...v, servers: v.servers.map((s) => (s.name === name ? { ...s, enabled } : s)) });
    try {
      setView(await setMcpServerEnabled(name, enabled));
      setError(null);
    } catch (e) {
      setError(String(e));
      await reload();
    }
  }, [reload]);

  return { view, error, save, setEnabled };
}
