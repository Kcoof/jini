// Conversation history list (specs/phase-1/spec.md 1.4).

import { useEffect, useState } from "react";
import { deleteConversation, getConversation, getConversations } from "../lib/api";
import type { ChatMessage, ConversationSummary } from "../lib/types";

type Props = {
  refreshKey: number;
  onOpen: (id: string, messages: ChatMessage[]) => void;
};

export function ConversationHistory({ refreshKey, onOpen }: Props) {
  const [items, setItems] = useState<ConversationSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getConversations()
      .then(setItems)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [refreshKey]);

  if (error) return <p className="status err">{error}</p>;
  if (!items) return <p className="hint">Loading history…</p>;
  if (items.length === 0) return <p className="hint">No conversations yet.</p>;

  return (
    <ul className="history-list">
      {items.map((item) => (
        <li key={item.id} className="history-item">
          <button
            type="button"
            className="history-open"
            onClick={() => {
              getConversation(item.id)
                .then((conv) => onOpen(conv.id, conv.messages))
                .catch((err) => console.error("[jini] open history failed", err));
            }}
          >
            <span className="history-title">{item.title}</span>
          </button>
          <button
            type="button"
            className="ghost history-delete"
            aria-label="Delete conversation"
            onClick={() => {
              deleteConversation(item.id)
                .then(() => getConversations())
                .then(setItems)
                .catch((err) => console.error("[jini] delete failed", err));
            }}
          >
            ✕
          </button>
        </li>
      ))}
    </ul>
  );
}
