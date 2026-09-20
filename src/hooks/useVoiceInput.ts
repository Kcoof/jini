// Push-to-talk speech input via Windows SAPI (specs/phase-2.4 STT
// Fallback B). WebView2's webkitSpeechRecognition exists but its cloud
// backend is not wired in WebView2 (always network-errors), so recognition
// runs in Rust: invoke start_listening/stop_listening and react to
// voice:// events. NOTE: SAPI dictation follows the Windows system speech
// language (usually en-US) — the language parameter is not applied.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

type VoiceState = {
  listening: boolean;
  transcript: string;
  error: string | null;
};

const SILENCE_TIMEOUT_MS = 5000;

export function useVoiceInput(
  _language: string,
  onFinal: (text: string, image: string | null) => void,
) {
  const [state, setState] = useState<VoiceState>({
    listening: false,
    transcript: "",
    error: null,
  });
  const timeoutRef = useRef<number | undefined>(undefined);
  // Capture-on-speak (specs/phase-2.5): the screenshot taken at mic-click
  // time rides along with the final transcript.
  const capturedImageRef = useRef<string | null>(null);
  const onFinalRef = useRef(onFinal);
  useEffect(() => {
    onFinalRef.current = onFinal;
  }, [onFinal]);

  const clearTimer = () => window.clearTimeout(timeoutRef.current);

  const armSilenceTimer = useCallback(() => {
    clearTimer();
    timeoutRef.current = window.setTimeout(() => {
      void invoke("stop_listening").catch(() => {});
      setState((prev) =>
        prev.listening
          ? { listening: false, transcript: "", error: "No speech detected. Try again." }
          : prev,
      );
    }, SILENCE_TIMEOUT_MS);
  }, []);

  const start = useCallback(async () => {
    setState({ listening: true, transcript: "", error: null });
    try {
      await invoke("start_listening");
      armSilenceTimer();
    } catch (err) {
      setState({
        listening: false,
        transcript: "",
        error: err instanceof Error ? err.message : String(err),
      });
    }
  }, [armSilenceTimer]);

  const stop = useCallback(async () => {
    clearTimer();
    capturedImageRef.current = null;
    await invoke("stop_listening").catch(() => {});
    setState({ listening: false, transcript: "", error: null });
  }, []);

  /** Start listening with a screenshot captured at mic-click time. */
  const startWithImage = useCallback(
    (image: string | null) => {
      capturedImageRef.current = image;
      void start();
    },
    [start],
  );

  // SAPI events → state. A final transcript auto-sends (spec §2c).
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    async function attach() {
      unlisteners.push(
        await listen<{ text: string }>("voice://hypothesis", (event) => {
          if (cancelled) return;
          setState((prev) => ({ ...prev, transcript: event.payload.text }));
          armSilenceTimer(); // user is speaking — reset the silence timer
        }),
      );
      unlisteners.push(
        await listen<{ text: string }>("voice://final", (event) => {
          if (cancelled) return;
          clearTimer();
          const text = event.payload.text.trim();
          setState({ listening: false, transcript: text, error: null });
          if (text) {
            const image = capturedImageRef.current;
            capturedImageRef.current = null;
            onFinalRef.current(text, image);
          }
        }),
      );
      unlisteners.push(
        await listen<{ message: string }>("voice://error", (event) => {
          if (cancelled) return;
          clearTimer();
          setState({ listening: false, transcript: "", error: event.payload.message });
        }),
      );
      unlisteners.push(
        await listen("voice://end", () => {
          if (cancelled) return;
          clearTimer();
          setState((prev) => (prev.listening ? { ...prev, listening: false } : prev));
        }),
      );
    }

    void attach();
    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, [armSilenceTimer]);

  // Esc cancels listening.
  useEffect(() => {
    if (!state.listening) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void stop();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.listening, stop]);

  useEffect(
    () => () => {
      clearTimer();
      void invoke("stop_listening").catch(() => {});
    },
    [],
  );

  return { ...state, start, startWithImage, stop };
}
