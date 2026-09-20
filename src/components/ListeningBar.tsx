// Listening bar (specs/phase-2.4 §5): live transcript while the mic is
// capturing, with a pulsing icon and Cancel.

import { Mic } from "lucide-react";

type Props = {
  transcript: string;
  onCancel: () => void;
};

export function ListeningBar({ transcript, onCancel }: Props) {
  return (
    <div className="listening-bar">
      <Mic size={18} strokeWidth={2.2} className="listening-icon" />
      <p className="listening-transcript">{transcript || "Listening…"}</p>
      <button type="button" className="ghost" onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
