// Streamed responses (specs/phase-1/spec.md 1.4): tokens render as they
// arrive; Cancel aborts mid-stream; errors show in plain language.
// Phase 2.3: agent tool calls render as gray cards (specs/phase-2.3 §12.3).

import { useState } from "react";
import { ConfirmCard } from "./ConfirmCard";
import type { ChatMessage, ToolCall } from "../lib/types";

const TOOL_LABELS: Record<string, string> = {
  capture_screen: "Screenshot",
  list_windows: "List windows",
  get_active_window: "Active window",
  read_clipboard: "Read clipboard",
  get_datetime: "Date & time",
  speak: "Speak",
};

function formatArgs(toolName: string, argsJson: string): string {
  try {
    const obj = JSON.parse(argsJson);
    if (toolName === "speak" && typeof obj.text === "string") return `"${obj.text}"`;
    return JSON.stringify(obj, null, 2);
  } catch {
    return argsJson;
  }
}

function ToolCard({ call }: { call: ToolCall }) {
  const [expanded, setExpanded] = useState(false);
  const label = TOOL_LABELS[call.tool_name] ?? call.tool_name;
  return (
    <div className={`tool-card ${call.status}`}>
      <button
        type="button"
        className="tool-card-header"
        onClick={() => setExpanded((e) => !e)}
      >
        <span className="tool-icon">⚙</span>
        <span className="tool-label">
          {call.status === "running" ? `Calling ${label}…` : `Used ${label}`}
        </span>
        <span className="tool-chevron">{expanded ? "▲" : "▾"}</span>
      </button>
      {expanded ? (
        <div className="tool-card-body">
          {call.arguments && call.arguments !== "{}" ? (
            <pre className="tool-args">{formatArgs(call.tool_name, call.arguments)}</pre>
          ) : null}
          {call.result ? <pre className="tool-result">{call.result}</pre> : null}
        </div>
      ) : null}
    </div>
  );
}

type Props = {
  messages: ChatMessage[];
  streaming: boolean;
  error: string | null;
  onCancel: () => void;
  activeCalls?: ToolCall[];
};

export function ResponsePanel({ messages, streaming, error, onCancel, activeCalls }: Props) {
  return (
    <section className="response-panel">
      <div className="messages">
        {messages.map((m) => (
          <div key={m.id} className={`bubble ${m.role}`}>
            {m.role === "user" && m.clipboard_context ? (
              <span className="ctx-flag">clipboard attached</span>
            ) : null}
            {m.image_base64 ? (
              <img
                className="bubble-image"
                src={`data:image/png;base64,${m.image_base64}`}
                alt="attached screenshot"
              />
            ) : null}
            {m.role === "tool" ? (
              <span className="ctx-flag">tool: {m.name ?? "unknown"}</span>
            ) : null}
            <p>{m.content}</p>
          </div>
        ))}
        {activeCalls && activeCalls.length > 0 ? (
          <div className="tool-cards">
            {activeCalls.map((call) =>
              call.status === "pending" || call.status === "declined" ? (
                <ConfirmCard key={call.call_id} call={call} />
              ) : (
                <ToolCard key={call.call_id} call={call} />
              ),
            )}
          </div>
        ) : null}
        {streaming ? <div className="caret" aria-hidden /> : null}
      </div>
      {error ? <p className="status err">{error}</p> : null}
      {streaming ? (
        <div className="stream-actions">
          <button type="button" className="ghost" onClick={onCancel}>
            Cancel
          </button>
        </div>
      ) : null}
    </section>
  );
}
