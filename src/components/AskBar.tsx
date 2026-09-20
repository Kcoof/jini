// Ask bar (specs/phase-1/spec.md 1.4/1.5): Enter sends, Shift+Enter newline,
// Esc back. Shows the clipboard context pill with a Clear button.

import { FormEvent, KeyboardEvent } from "react";

type Props = {
  value: string;
  onChange: (value: string) => void;
  onSend: () => void;
  onDismiss: () => void;
  clipboard: string | null;
  onClearClipboard: () => void;
  disabled?: boolean;
  /** Voice listening locks the text box (specs/phase-2.4 §2a). */
  inputLocked?: boolean;
};

export function AskBar({
  value,
  onChange,
  onSend,
  onDismiss,
  clipboard,
  onClearClipboard,
  disabled,
  inputLocked,
}: Props) {
  function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (!disabled) onSend();
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onDismiss();
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (!disabled) onSend();
    }
  }

  return (
    <form className="ask-bar" onSubmit={onSubmit}>
      {clipboard ? (
        <div className="clipboard-preview">
          <span className="clipboard-label">Context from clipboard</span>
          <p>
            {clipboard.slice(0, 280)}
            {clipboard.length > 280 ? "…" : ""}
          </p>
          <button type="button" className="ghost" onClick={onClearClipboard}>
            Clear
          </button>
        </div>
      ) : null}
      <textarea
        autoFocus
        rows={2}
        disabled={inputLocked}
        placeholder={
          inputLocked
            ? "Listening…"
            : disabled
              ? "Answer streaming — type your next question…"
              : "Ask anything…"
        }
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
      />
      <div className="ask-actions">
        <span className="hint">Enter to send · Shift+Enter newline · Esc back</span>
        <button type="submit" disabled={disabled || !value.trim()}>
          Send
        </button>
      </div>
    </form>
  );
}
