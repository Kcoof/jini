// Settings panel (specs/phase-1/spec.md 1.3). The key field is write-only;
// saved keys live in Windows Credential Manager and show only as flags.

import { useState } from "react";
import { testProvider } from "../lib/api";
import type { PresetId, Settings as SettingsType } from "../lib/types";

const PRESETS: { id: PresetId; label: string }[] = [
  { id: "glm", label: "GLM" },
  { id: "minimax", label: "MiniMax" },
  { id: "openai", label: "OpenAI" },
  { id: "anymodel", label: "AnyModel" },
  { id: "custom", label: "Custom" },
];

type Props = {
  settings: SettingsType;
  onSave: (partial: Partial<SettingsType> & { base_url?: string | null }) => Promise<unknown>;
  onSaveKey: (preset: PresetId, key: string) => Promise<unknown>;
  onHotkey: (hotkey: string) => Promise<unknown>;
};

export function Settings({ settings, onSave, onSaveKey, onHotkey }: Props) {
  const [preset, setPreset] = useState<PresetId>(settings.active_preset);
  const [model, setModel] = useState(settings.models[settings.active_preset] || settings.model);
  const [baseUrl, setBaseUrl] = useState(settings.base_urls[settings.active_preset] || "");
  const [hotkey, setHotkey] = useState(settings.hotkey);
  const [toolsOn, setToolsOn] = useState(settings.tools_enabled);
  const [actionsOn, setActionsOn] = useState(settings.actions_enabled);
  const [voiceMode, setVoiceMode] = useState(settings.voice_replies_mode);
  const [sttLang, setSttLang] = useState(settings.stt_language);
  const [apiKey, setApiKey] = useState("");
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [testResult, setTestResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);

  function switchPreset(id: PresetId) {
    setPreset(id);
    setModel(settings.models[id] || "");
    setBaseUrl(settings.base_urls[id] || "");
    setApiKey("");
    setTestResult(null);
  }

  async function save() {
    setStatus(null);
    try {
      await onSave({
        active_preset: preset,
        model,
        hotkey,
        theme: settings.theme,
        base_url: baseUrl,
        tools_enabled: toolsOn,
        voice_replies_mode: voiceMode,
        stt_language: sttLang,
        actions_enabled: actionsOn,
      });
      if (hotkey !== settings.hotkey) {
        await onHotkey(hotkey);
      }
      if (apiKey.trim()) {
        await onSaveKey(preset, apiKey.trim());
        setApiKey("");
      }
      setStatus({ ok: true, text: "Saved." });
    } catch (err) {
      setStatus({ ok: false, text: err instanceof Error ? err.message : String(err) });
    }
  }

  async function test() {
    setTesting(true);
    setTestResult(null);
    try {
      const reply = await testProvider({
        preset_id: preset,
        base_url: baseUrl || null,
        model: model || null,
        api_key: apiKey.trim() || null,
      });
      setTestResult({ ok: true, text: reply });
    } catch (err) {
      setTestResult({ ok: false, text: err instanceof Error ? err.message : String(err) });
    } finally {
      setTesting(false);
    }
  }

  return (
    <section className="settings">
      <label>
        Provider
        <select value={preset} onChange={(e) => switchPreset(e.target.value as PresetId)}>
          {PRESETS.map((item) => (
            <option key={item.id} value={item.id}>
              {item.label}
              {settings.api_key_set[item.id] ? " · key saved" : ""}
            </option>
          ))}
        </select>
      </label>
      <label>
        Endpoint
        <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://…" />
      </label>
      <label>
        Model
        <input value={model} onChange={(e) => setModel(e.target.value)} />
      </label>
      <label>
        API key
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          placeholder={settings.api_key_set[preset] ? "•••• saved in Credential Manager" : "Paste key"}
        />
      </label>
      <label>
        Hotkey
        <input value={hotkey} onChange={(e) => setHotkey(e.target.value)} placeholder="Alt+Space" />
      </label>
      <label>
        Voice replies
        <select value={voiceMode} onChange={(e) => setVoiceMode(e.target.value)}>
          <option value="auto">Auto (short answers + tool results)</option>
          <option value="always">Always speak replies</option>
          <option value="never">Never speak</option>
        </select>
      </label>
      <label>
        Speech language
        <input value={sttLang} onChange={(e) => setSttLang(e.target.value)} placeholder="en-US" />
        <span className="hint">BCP-47 tag (e.g., en-US, ar-SA)</span>
      </label>
      <div className="settings-toggle">
        <div className="settings-toggle-text">
          <strong>Action tools</strong>
          <span>
            Allow the AI to click, type, open files, and launch apps — with your YES/NO
            approval for each action. Default off.
          </span>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={actionsOn}
          className={`switch${actionsOn ? " on" : ""}`}
          onClick={() => setActionsOn((v) => !v)}
        >
          <span className="knob" />
        </button>
      </div>
      <div className="settings-toggle">
        <div className="settings-toggle-text">
          <strong>AI Tools (agent)</strong>
          <span>
            When on, the AI can call tools on its own — look at your screen, list open windows,
            read the clipboard, and speak.
          </span>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={toolsOn}
          className={`switch${toolsOn ? " on" : ""}`}
          onClick={() => setToolsOn((v) => !v)}
        >
          <span className="knob" />
        </button>
      </div>
      {status ? <p className={`status ${status.ok ? "ok" : "err"}`}>{status.text}</p> : null}
      {testResult ? (
        <p className={`status ${testResult.ok ? "ok" : "err"}`}>{testResult.text}</p>
      ) : null}
      <div className="settings-actions">
        <button type="button" className="ghost" onClick={() => void test()} disabled={testing}>
          {testing ? "Testing…" : "Test connection"}
        </button>
        <button type="button" onClick={() => void save()}>
          Save
        </button>
      </div>
    </section>
  );
}
