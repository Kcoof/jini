import { invoke } from "@tauri-apps/api/core";
import type {
  Conversation,
  ConversationSummary,
  PresetId,
  Settings,
} from "./types";

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function saveSettings(settings: {
  active_preset: PresetId;
  model: string;
  hotkey: string;
  theme: "dark" | "light";
  base_url?: string | null;
  tools_enabled?: boolean;
  voice_replies_mode?: string;
  stt_language?: string;
  actions_enabled?: boolean;
}): Promise<Settings> {
  return invoke("save_settings", { settings });
}

export function setApiKey(preset: PresetId, apiKey: string): Promise<void> {
  return invoke("set_api_key", { presetId: preset, apiKey });
}

export function hasApiKey(preset: PresetId): Promise<boolean> {
  return invoke("has_api_key", { presetId: preset });
}

export function testProvider(input: {
  preset_id: PresetId;
  base_url: string | null;
  model: string | null;
  api_key: string | null;
}): Promise<string> {
  return invoke("test_provider", { input });
}

export function setHotkey(hotkey: string): Promise<void> {
  return invoke("set_hotkey", { hotkey });
}

export function getClipboard(): Promise<{ text: string | null }> {
  return invoke("get_clipboard");
}

export function sendMessage(input: {
  conversation_id: string | null;
  content: string;
  clipboard_context: string | null;
  preset_id: PresetId | null;
  model: string | null;
  image_base64?: string | null;
}): Promise<{ conversation_id: string }> {
  return invoke("send_message", { input });
}

export function cancelMessage(): Promise<void> {
  return invoke("cancel_message");
}

export function getConversations(): Promise<ConversationSummary[]> {
  return invoke("get_conversations");
}

export function getConversation(id: string): Promise<Conversation> {
  return invoke("get_conversation", { id });
}

export function deleteConversation(id: string): Promise<void> {
  return invoke("delete_conversation", { id });
}

export function hideWindow(): Promise<void> {
  return invoke("hide_window");
}

/** Phase 2.1 — capture (optionally cropped) → path + base64 PNG.
 * The region comes from the UI in CSS px; scale to physical px here. */
export function captureScreen(
  region?: { x: number; y: number; w: number; h: number } | null,
): Promise<{ path: string; base64_png: string }> {
  const scale = window.devicePixelRatio || 1;
  const physical = region
    ? {
        x: Math.round(region.x * scale),
        y: Math.round(region.y * scale),
        w: Math.round(region.w * scale),
        h: Math.round(region.h * scale),
      }
    : null;
  return invoke("capture_screen", { region: physical });
}

/** Snipping-Tool mode: expand the window over the primary monitor. */
export function beginSnip(): Promise<{
  restore_x: number;
  restore_y: number;
  mon_x: number;
  mon_y: number;
  mon_w: number;
  mon_h: number;
}> {
  return invoke("begin_snip");
}

export function endSnip(restoreX: number, restoreY: number): Promise<void> {
  return invoke("end_snip", { restoreX, restoreY });
}

export function speakText(text: string): Promise<void> {
  return invoke("speak_text", { text });
}

export function stopSpeech(): Promise<void> {
  return invoke("stop_speech");
}

export function confirmTool(callId: string, approved: boolean): Promise<void> {
  return invoke("confirm_tool", { callId, approved });
}
