// Pending action confirmation card (specs/phase-2.6 §2c). Yellow card,
// YES/NO buttons, Esc = NO, read-only resolved state after the answer.

import { useEffect, useState } from "react";
import { confirmTool } from "../lib/api";
import type { ToolCall } from "../lib/types";

const ACTION_LABELS: Record<string, string> = {
  click_at: "Click",
  type_text: "Type text",
  press_keys: "Press keys",
  launch_app: "Launch app",
  open_path: "Open",
  find_elements: "Find elements",
};

export function ConfirmCard({ call }: { call: ToolCall }) {
  const [answered, setAnswered] = useState(false);
  const label = ACTION_LABELS[call.tool_name] ?? call.tool_name;

  async function answer(approved: boolean) {
    if (answered) return;
    setAnswered(true);
    await confirmTool(call.call_id, approved).catch((err) =>
      console.error("[jini] confirm failed", err),
    );
  }

  // Esc declines while this card is pending.
  useEffect(() => {
    if (call.status !== "pending" || answered) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        void answer(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (call.status !== "pending") {
    return (
      <div
        className={`confirm-card resolved ${call.status}`}
        role="status"
        aria-label={`${label} — ${call.status}`}
      >
        <span className="confirm-icon">⚡</span>
        <span className="confirm-label">
          {call.status === "declined" ? `Declined: ${label}` : `Approved: ${label}`}
        </span>
        <span className="confirm-desc">{call.description}</span>
      </div>
    );
  }

  return (
    <div
      className="confirm-card pending"
      role="dialog"
      aria-label={`Confirm action: ${call.description}`}
    >
      <span className="confirm-icon">⚡</span>
      <div className="confirm-body">
        <span className="confirm-label">{label}</span>
        <span className="confirm-desc">{call.description}</span>
      </div>
      <div className="confirm-actions" role="group" aria-label="Approve or decline">
        <button
          type="button"
          className="confirm-yes"
          disabled={answered}
          onClick={() => void answer(true)}
          aria-label="Approve this action"
        >
          YES
        </button>
        <button
          type="button"
          className="confirm-no ghost"
          disabled={answered}
          onClick={() => void answer(false)}
          aria-label="Decline this action"
        >
          NO
        </button>
      </div>
    </div>
  );
}
