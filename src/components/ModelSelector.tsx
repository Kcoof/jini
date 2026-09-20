// Provider chip in the chat header (specs/phase-1/spec.md 1.4). Switches
// the active preset; the model comes from the saved per-preset model.

import type { PresetId, Settings } from "../lib/types";

const PRESETS: PresetId[] = ["glm", "minimax", "openai", "anymodel", "custom"];

type Props = {
  settings: Settings;
  onChange: (preset: PresetId) => void;
};

export function ModelSelector({ settings, onChange }: Props) {
  return (
    <label className="model-selector">
      <span>{settings.active_preset.toUpperCase()}</span>
      <select
        value={settings.active_preset}
        onChange={(e) => onChange(e.target.value as PresetId)}
        aria-label="Provider preset"
      >
        {PRESETS.map((id) => (
          <option key={id} value={id}>
            {id} · {settings.models[id] || "unset"}
          </option>
        ))}
      </select>
    </label>
  );
}
