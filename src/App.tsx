import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AskBar } from "./components/AskBar";
import { ConversationHistory } from "./components/ConversationHistory";
import { Disc } from "./components/Disc";
import { ModelSelector } from "./components/ModelSelector";
import { ResponsePanel } from "./components/ResponsePanel";
import { Settings } from "./components/Settings";
import { SnipOverlay } from "./components/SnipOverlay";
import type { Region } from "./components/SnipOverlay";
import { ToolsTray } from "./components/ToolsTray";
import { ListeningBar } from "./components/ListeningBar";
import { useChat } from "./hooks/useChat";
import { useVoiceInput } from "./hooks/useVoiceInput";
import { useSettings } from "./hooks/useSettings";
import { captureScreen, hideWindow, stopSpeech } from "./lib/api";
import {
  closeMenuOverlay,
  fitDisc,
  fitSnip,
  fitWorkspace,
  openMenuOverlay,
  type MenuAnchor,
} from "./lib/windowFit";
import type { OverlayShown, PresetId } from "./lib/types";

type Panel = "home" | "chat" | "settings" | "history";

const CAPTURE_PROMPT =
  "What is in this picture? Tell me anything important, wrong, or noteworthy.";
/** Eyes Check All asks for a review, not a neutral description. */
const EYES_PROMPT =
  "Look at this screen. Describe what is on it, then point out anything important, wrong, unusual, or that needs attention. Be brief and clear.";

export default function App() {
  const { settings, update, saveKey, changeHotkey } = useSettings();
  const [draft, setDraft] = useState("");
  const [clipboard, setClipboard] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel>("home");
  const [historyTick, setHistoryTick] = useState(0);
  const [menuOpen, setMenuOpen] = useState(false);
  const [anchor, setAnchor] = useState<MenuAnchor | null>(null);
  const [snipping, setSnipping] = useState(false);
  const restoreFromSnip = useRef<(() => Promise<void>) | null>(null);
  const chat = useChat(settings.voice_replies_mode);
  const voice = useVoiceInput(settings.stt_language, (text, image) => {
    // Voice flow (2.4 §2c + 2.5): auto-send the transcript, with the
    // mic-click screenshot riding along as vision context.
    setDraft(text);
    void send(text, image);
  });

  const workspace = panel !== "home";
  /** A picture-check conversation shows the Check again button. */
  const lastUserHasImage = (() => {
    for (let i = chat.messages.length - 1; i >= 0; i--) {
      const m = chat.messages[i];
      if (m.role === "user") return !!m.image_base64;
    }
    return false;
  })();

  // Smart Clipboard: every summon reports the clipboard text and resets to a
  // clean disc-sized window (self-heal for any wrong-size state).
  useEffect(() => {
    const unlisten = listen<OverlayShown>("overlay://shown", (event) => {
      setClipboard(event.payload.clipboard);
      setPanel("home");
      setMenuOpen(false);
      void fitDisc();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Panel switches drive the window size: workspace sheets vs the disc.
  useEffect(() => {
    if (panel !== "home") {
      void fitWorkspace();
      return;
    }
    if (menuOpen) return;
    void closeMenuOverlay();
  }, [panel, menuOpen]);

  // When a stream ends, put the cursor back in the ask box so the next
  // question can be typed immediately (the user may have typed it while
  // the answer was still streaming).
  const wasStreaming = useRef(false);
  useEffect(() => {
    if (wasStreaming.current && !chat.streaming && panel === "chat") focusAsk();
    wasStreaming.current = chat.streaming;
  }, [chat.streaming, panel]);

  async function toggleMenu() {
    if (menuOpen) {
      setMenuOpen(false);
      if (panel === "home") await closeMenuOverlay();
      return;
    }
    if (panel !== "home") return;
    const spot = await openMenuOverlay();
    setAnchor(spot);
    setMenuOpen(true);
  }

  function focusAsk() {
    requestAnimationFrame(() => {
      const el = document.querySelector("textarea");
      if (el instanceof HTMLTextAreaElement) el.focus();
    });
  }

  function openChat() {
    setMenuOpen(false);
    setPanel("chat");
    focusAsk();
  }

  function goHome() {
    setMenuOpen(false);
    setPanel("home");
  }

  /** Cancel stops the stream AND any spoken reply (specs/phase-2.4 §3c). */
  function cancelStream() {
    void stopSpeech();
    void chat.cancel();
  }

  async function dismiss() {
    if (workspace || menuOpen) {
      goHome();
      return;
    }
    await hideWindow();
  }

  async function send(override?: string, image?: string | null) {
    const text = (override ?? draft).trim();
    if (!text) return;
    setDraft("");
    await stopSpeech(); // a new question interrupts the spoken answer
    await chat.send(
      text,
      clipboard,
      settings.active_preset,
      settings.model,
      image ?? null,
    );
    setHistoryTick((n) => n + 1);
  }

  async function takeScreenshot(mode: "full" | "select") {
    setMenuOpen(false);
    // CRITICAL: the [panel, menuOpen] effect queues its own closeMenuOverlay
    // when the tray closes. Await the close here so every shrink op leaves
    // the layout queue BEFORE fitSnip queues the full-screen expand —
    // otherwise the shrink lands AFTER the expand and re-shrinks the window.
    if (panel === "home") {
      await closeMenuOverlay();
    }
    try {
      if (mode === "full") {
        await analyzeCapture(null);
      } else {
        // Expand over the screen, let the user draw the region, then
        // capture just that part.
        const restore = await fitSnip();
        restoreFromSnip.current = restore;
        setSnipping(true);
      }
    } catch (err) {
      console.error("[jini] screenshot failed", err);
      await restoreFromSnip.current?.();
      restoreFromSnip.current = null;
      setSnipping(false);
    }
  }

  async function onSnipRegion(region: Region) {
    setSnipping(false);
    const restore = restoreFromSnip.current;
    restoreFromSnip.current = null;
    await restore?.();
    try {
      await analyzeCapture(region);
    } catch (err) {
      console.error("[jini] region capture failed", err);
    }
  }

  async function onSnipCancel() {
    setSnipping(false);
    const restore = restoreFromSnip.current;
    restoreFromSnip.current = null;
    await restore?.();
  }

  /** Capture → straight into the chat with the picture attached. */
  async function analyzeCapture(region: Region | null, prompt: string = CAPTURE_PROMPT) {
    if (chat.streaming) return; // never stack a second stream
    const { base64_png } = await captureScreen(region);
    setPanel("chat");
    focusAsk();
    await chat.send(prompt, clipboard, settings.active_preset, settings.model, base64_png);
    setHistoryTick((n) => n + 1);
  }

  /** Eyes Check All (specs/phase-2.2): one click, no chooser — the whole
   *  screen goes straight to the AI as a check, answer streams in chat. */
  /** Voice (specs/phase-2.4 + 2.5): mic click → capture the screen at that
   *  moment → chat opens → mic starts; the transcript sends WITH the shot. */
  async function activateVoice() {
    if (chat.streaming || voice.listening) return; // one operation at a time
    setMenuOpen(false);
    await closeMenuOverlay();
    setPanel("chat");
    let captured: string | null = null;
    try {
      const { base64_png } = await captureScreen(null);
      captured = base64_png;
    } catch (err) {
      // Capture failed — voice still works, just without the picture.
      console.error("[jini] capture-on-speak failed:", err);
    }
    voice.startWithImage(captured);
  }

  async function eyesCheckAll() {
    if (chat.streaming) return; // ignore while an answer is streaming
    setMenuOpen(false);
    // Same awaited close as takeScreenshot: the shrink must leave the
    // layout queue before any later grow, or the window ends up wrong.
    await closeMenuOverlay();
    try {
      await analyzeCapture(null, EYES_PROMPT);
    } catch (err) {
      console.error("[jini] eyes check failed", err);
    }
  }

  /** Check again: fresh capture as a NEW turn in the SAME conversation. */
  async function checkAgain() {
    if (chat.streaming) return;
    try {
      await analyzeCapture(null, EYES_PROMPT);
    } catch (err) {
      console.error("[jini] check again failed", err);
    }
  }

  return (
    <main className={`shell theme-${settings.theme}${workspace ? " workspace" : " home"}`}>
      {workspace ? (
        <section className="sheet">
          <header className="sheet-bar">
            <button type="button" className="ghost" onClick={() => void dismiss()}>
              Back
            </button>
            {panel === "chat" ? (
              <ModelSelector
                settings={settings}
                onChange={(preset: PresetId) =>
                  void update({
                    active_preset: preset,
                    model: settings.models[preset] || settings.model,
                  })
                }
              />
            ) : (
              <span className="sheet-title">
                {panel === "settings" ? "Settings" : "History"}
              </span>
            )}
            {panel === "chat" ? (
              <button type="button" className="ghost" onClick={() => setPanel("history")}>
                History
              </button>
            ) : (
              <span />
            )}
          </header>

          {panel === "settings" ? (
            <Settings settings={settings} onSave={update} onSaveKey={saveKey} onHotkey={changeHotkey} />
          ) : null}

          {panel === "history" ? (
            <ConversationHistory
              refreshKey={historyTick}
              onOpen={(id, messages) => {
                chat.loadMessages(id, messages);
                setPanel("chat");
              }}
            />
          ) : null}

          {panel === "chat" ? (
            <>
              <ResponsePanel
                messages={chat.messages}
                streaming={chat.streaming}
                error={chat.error}
                onCancel={cancelStream}
                activeCalls={chat.activeCalls}
              />
              {voice.listening ? (
                <ListeningBar transcript={voice.transcript} onCancel={voice.stop} />
              ) : null}
              {voice.error ? <p className="status err">{voice.error}</p> : null}
              {lastUserHasImage ? (
                <div className="check-again-bar">
                  <button
                    type="button"
                    className="ghost"
                    disabled={chat.streaming}
                    onClick={() => void checkAgain()}
                  >
                    ⟳ Check again — look at my screen now
                  </button>
                </div>
              ) : null}
              <AskBar
                value={draft}
                onChange={setDraft}
                onSend={() => void send()}
                onDismiss={() => void dismiss()}
                clipboard={clipboard}
                onClearClipboard={() => setClipboard(null)}
                disabled={chat.streaming || voice.listening}
                inputLocked={voice.listening}
              />
            </>
          ) : null}
        </section>
      ) : null}

      <Disc onToggle={() => void toggleMenu()} discWindow={menuOpen ? anchor : null} />

      {menuOpen ? (
        <ToolsTray
          anchor={anchor}
          onAskAi={openChat}
          onClipboard={openChat}
          onSettings={() => {
            setMenuOpen(false);
            setPanel("settings");
          }}
          onClose={() => setMenuOpen(false)}
          onScreenshot={(mode) => void takeScreenshot(mode)}
          onEyesCheck={() => void eyesCheckAll()}
          onVoiceActivate={() => void activateVoice()}
        />
      ) : null}

      {snipping ? (
        <SnipOverlay onRegion={(r) => void onSnipRegion(r)} onCancel={() => void onSnipCancel()} />
      ) : null}
    </main>
  );
}
