import { useCallback, useEffect, useState } from "react";
import {
  llmSettings,
  removeProvider,
  saveApiKey,
  saveProvider,
  setActiveProvider,
  setDebugLogging,
  type LlmSettings,
  type ProviderConfig,
} from "../lib/chat";

/** Which provider a turn talks to, and what it needs to get there. */
export function useLlmSettings() {
  const [settings, setSettings] = useState<LlmSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    try {
      setSettings(await llmSettings());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const guard = useCallback(
    async (work: () => Promise<void>) => {
      setBusy(true);
      setError(null);
      try {
        await work();
        await reload();
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [reload],
  );

  const save = useCallback(
    (provider: ProviderConfig, apiKey: string | null) =>
      guard(async () => {
        await saveProvider(provider);
        // Only when the user typed one: an untouched field means "leave the
        // stored key alone", and sending its empty value would delete it.
        if (apiKey !== null) await saveApiKey(provider.id, apiKey);
        await setActiveProvider(provider.id);
      }),
    [guard],
  );

  const remove = useCallback((id: string) => guard(() => removeProvider(id)), [guard]);
  const select = useCallback((id: string) => guard(() => setActiveProvider(id)), [guard]);
  const debugLogging = useCallback(
    (enabled: boolean) => guard(() => setDebugLogging(enabled)),
    [guard],
  );

  return { settings, error, busy, save, remove, select, debugLogging };
}
