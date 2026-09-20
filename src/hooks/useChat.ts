// Streaming chat state (specs/phase-1/spec.md 1.4): listens to the Rust
// events, assembles tokens as they arrive, exposes send/cancel/load.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { cancelMessage, sendMessage, speakText } from "../lib/api";
import type {
  ChatChunk,
  ChatDone,
  ChatError,
  ChatMessage,
  ChatToolPending,
  ChatToolResult,
  ChatToolStart,
  PresetId,
  ToolCall,
} from "../lib/types";

type ChatState = {
  conversationId: string | null;
  messages: ChatMessage[];
  streaming: boolean;
  error: string | null;
  /** Agent tool calls this turn, shown as gray cards (specs/phase-2.3). */
  activeCalls: ToolCall[];
};

const empty: ChatState = {
  conversationId: null,
  messages: [],
  streaming: false,
  error: null,
  activeCalls: [],
};

function stripCodeBlocks(text: string): string {
  // Remove fenced code blocks (``` ... ```) — code read aloud is noise.
  return text.replace(/```[\s\S]*?```/g, "");
}

export function useChat(voiceRepliesMode: string) {
  const [state, setState] = useState<ChatState>(empty);
  const streamBuf = useRef("");
  // The done listener attaches once; these refs keep it reading fresh
  // values instead of stale closures (same pattern as conversationIdRef).
  const voiceModeRef = useRef(voiceRepliesMode);
  useEffect(() => {
    voiceModeRef.current = voiceRepliesMode;
  }, [voiceRepliesMode]);
  const messagesRef = useRef<ChatMessage[]>([]);
  const callsRef = useRef<ToolCall[]>([]);
  useEffect(() => {
    messagesRef.current = state.messages;
    callsRef.current = state.activeCalls;
  }, [state.messages, state.activeCalls]);
  // Mirror of state.conversationId so `send` never closes over a stale id
  // (the screenshot flow calls send programmatically right after renders).
  const conversationIdRef = useRef<string | null>(null);

  useEffect(() => {
    conversationIdRef.current = state.conversationId;
  }, [state.conversationId]);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    async function attach() {
      unlisteners.push(
        await listen<ChatChunk>("chat://chunk", (event) => {
          if (cancelled) return;
          const { conversation_id, text } = event.payload;
          streamBuf.current += text;
          setState((prev) => {
            const msgs = [...prev.messages];
            const last = msgs[msgs.length - 1];
            if (last && last.role === "assistant" && last.id.startsWith("stream-")) {
              msgs[msgs.length - 1] = { ...last, content: streamBuf.current };
            } else {
              msgs.push({
                id: `stream-${conversation_id}`,
                role: "assistant",
                content: streamBuf.current,
                created_at: new Date().toISOString(),
              });
            }
            return { ...prev, conversationId: conversation_id, messages: msgs, streaming: true, error: null };
          });
        }),
      );
      unlisteners.push(
        await listen<ChatDone>("chat://done", (event) => {
          if (cancelled) return;
          // A late done from a PREVIOUS turn must not clobber the current
          // one (e.g. its empty buffer would read as "no text").
          if (conversationIdRef.current && event.payload.conversation_id && event.payload.conversation_id !== conversationIdRef.current) {
            return;
          }
          const gotText = streamBuf.current !== "";
          streamBuf.current = "";
          setState((prev) => ({
            ...prev,
            streaming: false,
            // activeCalls stay so the finished cards remain visible until
            // the next send clears them (AT-1: "Used List windows" persists).
            conversationId: event.payload.conversation_id || prev.conversationId,
            error:
              !event.payload.cancelled && !gotText
                ? "The provider connected but returned no text. Try again, or check the model name in Settings."
                : prev.error,
          }));
          // Spoken replies (specs/phase-2.4 §3): after a completed turn,
          // speak the final answer when the mode allows. "auto" speaks
          // tool turns and short answers, skipping long reasoning rambles.
          if (!event.payload.cancelled) {
            const mode = voiceModeRef.current;
            if (mode !== "never") {
              const msgs = messagesRef.current;
              const last = msgs[msgs.length - 1];
              const usedTools = callsRef.current.length > 0;
              if (last?.role === "assistant" && last.content.trim()) {
                const stripped = stripCodeBlocks(last.content).trim();
                const shouldSpeak =
                  mode === "always" || (mode === "auto" && (usedTools || stripped.length <= 300));
                if (shouldSpeak && stripped) {
                  void speakText(stripped);
                }
              }
            }
          }
        }),
      );
      unlisteners.push(
        await listen<ChatError>("chat://error", (event) => {
          if (cancelled) return;
          if (conversationIdRef.current && event.payload.conversation_id && event.payload.conversation_id !== conversationIdRef.current) {
            return;
          }
          streamBuf.current = "";
          setState((prev) => ({
            ...prev,
            streaming: false,
            error: event.payload.message,
          }));
        }),
      );
      unlisteners.push(
        await listen<ChatToolStart>("chat://tool-start", (event) => {
          if (cancelled) return;
          const { call_id, tool_name, arguments: args } = event.payload;
          setState((prev) => {
            const exists = prev.activeCalls.some((c) => c.call_id === call_id);
            return {
              ...prev,
              activeCalls: exists
                ? prev.activeCalls.map((c) =>
                    c.call_id === call_id ? { ...c, status: "running" as const } : c,
                  )
                : [
                    ...prev.activeCalls,
                    { call_id, tool_name, arguments: args, status: "running" as const },
                  ],
            };
          });
        }),
      );
      unlisteners.push(
        await listen<ChatToolPending>("chat://tool-pending", (event) => {
          if (cancelled) return;
          const { call_id, tool_name, arguments: args, description } = event.payload;
          setState((prev) => ({
            ...prev,
            activeCalls: [
              ...prev.activeCalls.filter((c) => c.call_id !== call_id),
              { call_id, tool_name, arguments: args, description, status: "pending" as const },
            ],
          }));
        }),
      );
      unlisteners.push(
        await listen<ChatToolResult>("chat://tool-result", (event) => {
          if (cancelled) return;
          const { call_id, result } = event.payload;
          setState((prev) => ({
            ...prev,
            activeCalls: prev.activeCalls.map((c) =>
              c.call_id === call_id
                ? {
                    ...c,
                    result,
                    status: result.includes("Action declined") ? ("declined" as const) : ("done" as const),
                  }
                : c,
            ),
          }));
        }),
      );
    }

    void attach();
    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);

  const send = useCallback(
    async (
      content: string,
      clipboardContext: string | null,
      preset: PresetId | null,
      model: string | null,
      imageBase64?: string | null,
    ) => {
      streamBuf.current = "";
      const userMsg: ChatMessage = {
        id: `user-${Date.now()}`,
        role: "user",
        content,
        created_at: new Date().toISOString(),
        clipboard_context: clipboardContext,
        image_base64: imageBase64 ?? null,
      };
      setState((prev) => ({
        ...prev,
        messages: [...prev.messages, userMsg],
        streaming: true,
        error: null,
        activeCalls: [],
      }));
      try {
        const { conversation_id } = await sendMessage({
          conversation_id: conversationIdRef.current,
          content,
          clipboard_context: clipboardContext,
          preset_id: preset,
          model,
          image_base64: imageBase64 ?? null,
        });
        conversationIdRef.current = conversation_id;
        setState((prev) => ({ ...prev, conversationId: conversation_id }));
      } catch (err) {
        setState((prev) => ({
          ...prev,
          streaming: false,
          error: err instanceof Error ? err.message : String(err),
        }));
      }
    },
    [],
  );

  const cancel = useCallback(async () => {
    await cancelMessage();
  }, []);

  const loadMessages = useCallback((conversationId: string, messages: ChatMessage[]) => {
    streamBuf.current = "";
    setState({ conversationId, messages, streaming: false, error: null, activeCalls: [] });
  }, []);

  const reset = useCallback(() => {
    streamBuf.current = "";
    setState(empty);
  }, []);

  return { ...state, send, cancel, loadMessages, reset };
}
