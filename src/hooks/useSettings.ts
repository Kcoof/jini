import { useCallback, useEffect, useState } from "react";
import { getSettings, saveSettings, setApiKey, setHotkey } from "../lib/api";
import type { PresetId, Settings } from "../lib/types";

const fallback: Settings = {
  active_preset: "glm",
  model: "glm-5.3",
  hotkey: "Alt+Space",
  theme: "dark",
  models: {},
  base_urls: {},
  api_key_set: {},
  tools_enabled: true,
  voice_replies_mode: "auto",
  stt_language: "en-US",
  actions_enabled: false,
};

export function useSettings() {
  const [settings, setSettings] = useState<Settings>(fallback);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((err) => console.error("[jini] settings load failed", err));
  }, []);

  /** Save the non-secret core; returns the fresh settings. */
  const update = useCallback(
    async (partial: Partial<Settings> & { base_url?: string | null }) => {
      // Send the FULL merged settings: the old field-by-field rebuild
      // silently dropped every field added after Phase 1 (tools, voice,
      // actions toggles were never persisted until this fix).
      const next = await saveSettings({
        ...settings,
        ...partial,
        base_url: partial.base_url ?? null,
      });
      setSettings(next);
      return next;
    },
    [settings],
  );

  const saveKey = useCallback(async (preset: PresetId, key: string) => {
    await setApiKey(preset, key);
    setSettings(await getSettings());
  }, []);

  const changeHotkey = useCallback(async (hotkey: string) => {
    await setHotkey(hotkey);
  }, []);

  return { settings, update, saveKey, changeHotkey };
}
