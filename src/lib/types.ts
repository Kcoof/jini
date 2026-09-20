export type PresetId = "glm" | "minimax" | "openai" | "anymodel" | "custom";

export type Settings = {
  active_preset: PresetId;
  model: string;
  hotkey: string;
  theme: "dark" | "light";
  models: Partial<Record<PresetId, string>>;
  base_urls: Partial<Record<PresetId, string>>;
  api_key_set: Partial<Record<PresetId, boolean>>;
  /** Agent tools master switch (specs/phase-2.3). Default true. */
  tools_enabled: boolean;
  /** Spoken replies: "always" | "auto" | "never" (specs/phase-2.4). */
  voice_replies_mode: string;
  /** BCP-47 speech recognition language (default en-US). */
  stt_language: string;
  /** Action tools master switch (specs/phase-2.6). Default off. */
  actions_enabled: boolean;
};

export type ChatMessage = {
  id: string;
  role: "user" | "assistant" | "tool";
  content: string;
  created_at: string;
  clipboard_context?: string | null;
  /** Base64 PNG attached to this message (screenshot analysis). */
  image_base64?: string | null;
  /** On assistant messages: the tool calls it requested. */
  tool_calls?: unknown;
  /** On tool messages: which call this answers. */
  tool_call_id?: string | null;
  /** On tool messages: the tool name. */
  name?: string | null;
};

/** One agent tool call shown as a gray card in the chat. */
export type ToolCall = {
  call_id: string;
  tool_name: string;
  /** Raw JSON arguments string as sent by the model. */
  arguments: string;
  result?: string;
  status: "pending" | "running" | "done" | "error" | "declined";
  /** Human-readable action description for confirm cards (2.6). */
  description?: string;
};

export type ChatToolPending = {
  conversation_id: string;
  call_id: string;
  tool_name: string;
  arguments: string;
  description: string;
};

export type ChatToolStart = {
  conversation_id: string;
  call_id: string;
  tool_name: string;
  arguments: string;
};

export type ChatToolResult = {
  conversation_id: string;
  call_id: string;
  tool_name: string;
  result: string;
};

export type Conversation = {
  id: string;
  title: string;
  messages: ChatMessage[];
  created_at: string;
  updated_at: string;
};

export type ConversationSummary = {
  id: string;
  title: string;
  updated_at: string;
};

export type OverlayShown = { clipboard: string | null };

export type ChatChunk = { conversation_id: string; text: string };
export type ChatDone = { conversation_id: string; cancelled: boolean };
export type ChatError = { conversation_id: string; message: string };
