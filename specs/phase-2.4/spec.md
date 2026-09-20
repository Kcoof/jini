# Phase 2.4 — Voice (push-to-talk STT + spoken replies TTS)

**Status:** SPEC  
**Estimated effort:** 1 week  
**Depends on:** Phase 2.3 (agent loop + tools, VERDICT OK)  
**Out of scope:** Wake word (2.7), streaming TTS mid-answer, voice for MCP door, voice activity detection (VAD), custom wake phrases

---

## Goal

Add voice input (push-to-talk speech-to-text) and voice output (TTS for final answers). The mic tool in the tray becomes live. User clicks mic → speaks → transcript appears in the ask box → auto-sends. The final answer is spoken aloud via SAPI. Fast, single-click flow optimized for hands-free operation after initial activation.

---

## 1. STT Technology Probe (Step 0 — must complete before implementation)

**Decision ladder:**

1. **Primary: WebView2 `webkitSpeechRecognition`** — Check if available in our WebView2 runtime (Windows 11, Edge WebView2). Probe: `typeof (window as any).webkitSpeechRecognition !== 'undefined'` in frontend + one test recognition attempt with a 5-second timeout. If available → use it (zero Rust dependencies, proven UX in Chrome/Edge).

2. **Fallback A: Provider's Whisper-compatible audio endpoint** — If WebView2 STT unavailable, check if the active provider (currently GLM/ZhipuAI via anymodel.org or zai.pub) offers a `/v1/audio/transcriptions` endpoint compatible with OpenAI's Whisper API (POST with `multipart/form-data`, file field, model field). Probe: attempt a test request with a 1-second silent WAV file. If 200 OK or recognized error format → use it.

3. **Fallback B: Windows SAPI recognition (ISpRecoContext)** — Rust implementation using `windows` crate, `Win32::Media::Speech::ISpRecoContext`. Streams partial transcript via Tauri events. Only if A and B both fail.

**Spec assumes Primary (WebView2) is available.** If the probe fails, the orchestrator will adapt the spec based on which fallback succeeds. Document the probe result in a comment at the top of the voice implementation file.

---

## 2. Mic Tool UX Flow

### 2a. Activation (tray → mic click)

**Before click:**
- Tray is open (444×224 tools tray + disc).
- Mic tool shows `live: true` (tools.ts line 40).

**On click:**
1. Close tray immediately (same flow as eyesCheckAll).
2. Open chat panel (workspace mode, 444×668).
3. Show **listening bar** above the AskBar, below ResponsePanel.
4. Listening bar contains:
   - Purple glowing mic icon (Lucide `Mic`, 18px, color `#a78bfa` with pulse animation).
   - Live interim transcript text (gray, updates as speech recognition emits interim results).
   - "Cancel" button (ghost style) + Esc handler.
5. AskBar textarea is **disabled and dimmed** during listening (placeholder: "Listening…").
6. Focus remains on the window so Esc works.

### 2b. During listening

- WebView2 `SpeechRecognition` instance: `interimResults: true`, `continuous: false` (single utterance), `lang` from settings (default `"en-US"`).
- Interim results update the listening bar text in real-time.
- User can click "Cancel" or press Esc → stop recognition, hide listening bar, restore AskBar, no send.
- Timeout: if **5 seconds pass with no speech detected** (no interim or final results), auto-cancel with a transient error message: "No speech detected. Try again." (2s toast, then fade).

### 2c. End of utterance (final result)

When `SpeechRecognition` fires `result` event with `isFinal: true`:
1. Stop recognition immediately.
2. Hide listening bar.
3. **Auto-send** the final transcript:
   - Insert transcript into AskBar value (so user sees it for ~100ms before the answer starts streaming).
   - Call `chat.send(transcript, clipboard, preset, model)` immediately (no confirm step — owner wants FAST).
4. Chat panel remains open, answer streams as normal.

**Rationale for auto-send:** Owner wants voice to be a fast, hands-free flow. Confirmation adds friction. User can cancel the stream if the transcript was wrong, then type a correction.

### 2d. Error states (user-facing messages)

- **No mic permission:** "Microphone access denied. Allow microphone access in your browser settings and try again."
- **Network error during recognition:** "Speech recognition failed. Check your network and try again."
- **No speech detected (5s timeout):** "No speech detected. Try again."
- **Generic error:** "Voice input failed: [error message]."

All errors appear as a 2-second toast below the listening bar (or in the chat error slot if listening bar is hidden).

---

## 3. TTS: Spoken Replies

### 3a. When to speak

**Trigger:** After `chat://done` event, if `tools_enabled` is true AND a new setting `voice_replies_mode` allows it.

**New setting: `voice_replies_mode`** (string, default `"auto"`):
- `"always"` — Speak every assistant reply.
- `"auto"` — Speak replies that are SHORT (≤300 chars after stripping markdown code blocks) OR replies where the turn used tools. Skip long reasoning-heavy answers.
- `"never"` — Never speak (TTS off).

Rationale: GLM-5.3-flash (current active model) produces long reasoning preambles ("The user asked for X, so I will Y…"). Speaking those aloud is terrible UX. The `"auto"` heuristic skips them but speaks concise answers and tool-driven answers (like "The time is 3:42 PM"). User can override to `"always"` if they want everything spoken, or `"never"` to disable TTS entirely.

### 3b. What to speak

Speak the **final assistant message content** as-is, with one filter: strip markdown code blocks (triple-backtick blocks) before speaking. Rationale: code read aloud is unintelligible; the user can read it on screen.

Strip pattern (regex): `` `{3}[^\n]*\n[\s\S]*?`{3} `` (greedy match of fenced code blocks). Apply once before passing to TTS.

### 3c. TTS implementation

**Rust command:** `speak_text(text: String) -> Result<(), String>`

Signature:
```rust
#[tauri::command]
pub fn speak_text(text: String) -> Result<(), String>
```

Implementation: Extract the SAPI speech logic from `tools/speak.rs` into a shared private function `fn sapi_speak(text: &str) -> Result<(), String>` in a new file `src-tauri/src/speech.rs`. Both `tools/speak.rs` (the agent tool) and `commands::speak_text` (the command) call it.

File structure:
- `src-tauri/src/speech.rs`: `pub fn sapi_speak(text: &str) -> Result<(), String>` — single source of truth for SAPI TTS. Uses `CoInitializeEx(COINIT_MULTITHREADED)`, `CoCreateInstance::<SpVoice>`, `Speak(PCWSTR, SPF_ASYNC, None)`. 500-char truncation applied here (safety limit; long answers are filtered by the frontend before calling).
- `src-tauri/src/tools/speak.rs`: `pub async fn run(args: &Value)` calls `speech::sapi_speak(text)` wrapped in `spawn_blocking`.
- `src-tauri/src/commands.rs`: `pub fn speak_text(text: String)` calls `speech::sapi_speak(&text)` directly (synchronous command, no async needed — SPF_ASYNC makes SAPI non-blocking).

**Frontend integration:**

In `useChat.ts`, add a new effect that listens to `chat://done`. When fired:
1. Check `settings.voice_replies_mode` (passed as a prop or fetched from context).
2. Apply the mode logic:
   - `"never"` → skip.
   - `"always"` → speak.
   - `"auto"` → check: if `activeCalls.length > 0` (tool turn) OR final message content ≤ 300 chars (after stripping code blocks) → speak, else skip.
3. Strip markdown code blocks from `messages[last].content`.
4. Call `invoke("speak_text", { text: strippedContent })`.

**Stop/queue behavior:**

- **New send while speaking:** Before calling `chat.send(...)`, call a new command `stop_speech()` that cancels any queued SAPI speech. SAPI `ISpVoice::Skip` skips all queued text. Signature: `#[tauri::command] pub fn stop_speech() -> Result<(), String>`. Implementation: `CoCreateInstance::<SpVoice>` + `voice.Skip("Sentence", 999999, None)` (skip all queued items).
- **Cancel button during stream:** The existing `chat.cancel()` already stops the stream. Add a `stop_speech()` call before `cancelMessage()` in the Cancel button handler (App.tsx line 267).

---

## 4. Settings Schema & UI

### 4a. Database schema (src-tauri/src/db.rs)

Add three new settings keys (stored in `settings` table as key-value pairs):

```rust
// In load_settings():
let voice_replies_mode = get_value(conn, "voice_replies_mode")?.unwrap_or_else(|| "auto".into());
let stt_language = get_value(conn, "stt_language")?.unwrap_or_else(|| "en-US".into());

// In Settings struct (add fields):
pub voice_replies_mode: String,  // "always" | "auto" | "never"
pub stt_language: String,         // BCP-47 language tag (e.g., "en-US", "zh-CN")
```

### 4b. SaveSettingsInput (commands.rs)

Add optional fields:
```rust
pub voice_replies_mode: Option<String>,
pub stt_language: Option<String>,
```

Update `save_core_settings` to persist them (or leave unchanged if `None`).

### 4c. Settings.tsx UI

Add two new rows below the "AI Tools" toggle:

**Row 1: Voice replies**
```tsx
<label>
  Voice replies
  <select value={voiceMode} onChange={(e) => setVoiceMode(e.target.value)}>
    <option value="auto">Auto (short answers + tool results)</option>
    <option value="always">Always</option>
    <option value="never">Never</option>
  </select>
</label>
```

**Row 2: Speech language**
```tsx
<label>
  Speech language
  <input 
    value={sttLang} 
    onChange={(e) => setSttLang(e.target.value)} 
    placeholder="en-US" 
  />
  <span className="hint">BCP-47 tag (e.g., en-US, zh-CN, es-ES)</span>
</label>
```

---

## 5. New Frontend Component: ListeningBar

File: `src/components/ListeningBar.tsx`

```tsx
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
```

**CSS** (add to existing stylesheet):

```css
.listening-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background: rgba(167, 139, 250, 0.1);
  border-bottom: 1px solid rgba(167, 139, 250, 0.3);
}

.listening-icon {
  color: #a78bfa;
  animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

.listening-transcript {
  flex: 1;
  margin: 0;
  color: #94a3b8;
  font-size: 14px;
}
```

---

## 6. Voice Hook: useVoiceInput

File: `src/hooks/useVoiceInput.ts`

```typescript
import { useCallback, useEffect, useRef, useState } from "react";

type VoiceState = {
  listening: boolean;
  transcript: string;
  error: string | null;
};

export function useVoiceInput(language: string, onFinal: (text: string) => void) {
  const [state, setState] = useState<VoiceState>({
    listening: false,
    transcript: "",
    error: null,
  });
  const recognitionRef = useRef<any>(null);
  const timeoutRef = useRef<number | undefined>(undefined);

  const start = useCallback(() => {
    const SpeechRecognition = (window as any).webkitSpeechRecognition || (window as any).SpeechRecognition;
    if (!SpeechRecognition) {
      setState({ listening: false, transcript: "", error: "Speech recognition not available." });
      return;
    }

    const recognition = new SpeechRecognition();
    recognition.lang = language;
    recognition.interimResults = true;
    recognition.continuous = false;

    recognition.onstart = () => {
      setState({ listening: true, transcript: "", error: null });
      // 5-second no-speech timeout
      timeoutRef.current = window.setTimeout(() => {
        recognition.stop();
        setState((prev) => ({
          ...prev,
          listening: false,
          error: "No speech detected. Try again.",
        }));
      }, 5000);
    };

    recognition.onresult = (event: any) => {
      window.clearTimeout(timeoutRef.current);
      let interim = "";
      let final = "";
      for (let i = 0; i < event.results.length; i++) {
        const transcript = event.results[i][0].transcript;
        if (event.results[i].isFinal) {
          final += transcript;
        } else {
          interim += transcript;
        }
      }
      if (final) {
        setState({ listening: false, transcript: final, error: null });
        onFinal(final.trim());
      } else {
        setState((prev) => ({ ...prev, transcript: interim }));
        // Reset timeout on interim result (user is speaking)
        timeoutRef.current = window.setTimeout(() => {
          recognition.stop();
          setState((prev) => ({
            ...prev,
            listening: false,
            error: "No speech detected. Try again.",
          }));
        }, 5000);
      }
    };

    recognition.onerror = (event: any) => {
      window.clearTimeout(timeoutRef.current);
      let msg = "Voice input failed.";
      if (event.error === "not-allowed") {
        msg = "Microphone access denied. Allow microphone access and try again.";
      } else if (event.error === "network") {
        msg = "Speech recognition failed. Check your network and try again.";
      } else if (event.error === "no-speech") {
        msg = "No speech detected. Try again.";
      }
      setState({ listening: false, transcript: "", error: msg });
    };

    recognition.onend = () => {
      window.clearTimeout(timeoutRef.current);
      setState((prev) => ({ ...prev, listening: false }));
    };

    recognitionRef.current = recognition;
    recognition.start();
  }, [language, onFinal]);

  const stop = useCallback(() => {
    window.clearTimeout(timeoutRef.current);
    recognitionRef.current?.stop();
    setState({ listening: false, transcript: "", error: null });
  }, []);

  useEffect(() => {
    return () => {
      window.clearTimeout(timeoutRef.current);
      recognitionRef.current?.stop();
    };
  }, []);

  return { ...state, start, stop };
}
```

---

## 7. Integration Points

### 7a. App.tsx changes

1. Import `ListeningBar` and `useVoiceInput`.
2. Add `const voice = useVoiceInput(settings.stt_language, (text) => { setDraft(text); void send(); });` after `const chat = useChat();`.
3. Add a `listening` state flag (derived from `voice.listening`).
4. In `ToolsTray` `onVoiceActivate` prop, call:
   ```tsx
   function activateVoice() {
     setMenuOpen(false);
     setPanel("chat");
     voice.start();
   }
   ```
5. Render `ListeningBar` between `ResponsePanel` and `AskBar` when `voice.listening` is true:
   ```tsx
   {voice.listening ? (
     <ListeningBar transcript={voice.transcript} onCancel={voice.stop} />
   ) : null}
   ```
6. Disable `AskBar` when `voice.listening` (pass `disabled={chat.streaming || voice.listening}`).
7. Add Esc handler in workspace mode: if `voice.listening`, call `voice.stop()` before `goHome()`.

### 7b. ToolsTray.tsx changes

1. Update `tools.ts` line 40: `live: false` → `live: true`.
2. Add `onVoiceActivate: () => void` prop to `ToolsTray`.
3. In `activate(tool)` switch, case `"voice"`: call `onVoiceActivate()` if `tool.live`.

### 7c. useChat.ts changes

Add TTS effect after the `chat://done` listener (inside the same `attach()` function):

```typescript
unlisteners.push(
  await listen<ChatDone>("chat://done", (event) => {
    // ... existing done logic ...
    
    // TTS: speak the final answer if voice_replies_mode allows
    if (!event.payload.cancelled && settings.voice_replies_mode !== "never") {
      const lastMsg = state.messages[state.messages.length - 1];
      if (lastMsg?.role === "assistant") {
        const shouldSpeak = 
          settings.voice_replies_mode === "always" ||
          (settings.voice_replies_mode === "auto" && (
            state.activeCalls.length > 0 || 
            stripCodeBlocks(lastMsg.content).length <= 300
          ));
        if (shouldSpeak) {
          const text = stripCodeBlocks(lastMsg.content);
          void invoke("speak_text", { text });
        }
      }
    }
  }),
);

function stripCodeBlocks(text: string): string {
  return text.replace(/```[^\n]*\n[\s\S]*?```/g, "");
}
```

**Note:** `settings` must be passed as a prop to `useChat` or accessed via context. Update `useChat` signature to accept `settings: Settings`.

### 7d. New Tauri commands

Add to `src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub fn speak_text(text: String) -> Result<(), String> {
    crate::speech::sapi_speak(&text)
}

#[tauri::command]
pub fn stop_speech() -> Result<(), String> {
    crate::speech::stop_sapi_speech()
}
```

Register in `lib.rs` `invoke_handler!`:
```rust
commands::speak_text,
commands::stop_speech,
```

### 7e. New speech.rs module

File: `src-tauri/src/speech.rs`

```rust
//! Shared SAPI text-to-speech logic (specs/phase-2.4). Used by both the
//! `speak` agent tool and the `speak_text` command for voice replies.

use windows::core::PCWSTR;
use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPF_ASYNC, SPF_PURGEBEFORESPEAK};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

/// Speak text aloud via SAPI (fire-and-forget, 500-char limit).
pub fn sapi_speak(text: &str) -> Result<(), String> {
    let truncated: String = text.chars().take(500).collect();
    if truncated.is_empty() {
        return Err("speak: text must not be empty".into());
    }
    
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)
            .map_err(|e| format!("SAPI init: {e}"))?;
        let wide: Vec<u16> = truncated.encode_utf16().chain(std::iter::once(0u16)).collect();
        voice
            .Speak(PCWSTR(wide.as_ptr()), SPF_ASYNC.0 as u32, None)
            .map_err(|e| format!("SAPI speak: {e}"))?;
    }
    Ok(())
}

/// Stop all queued SAPI speech immediately.
pub fn stop_sapi_speech() -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)
            .map_err(|e| format!("SAPI init: {e}"))?;
        // SPF_PURGEBEFORESPEAK clears the queue; speak empty string with it.
        voice
            .Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None)
            .map_err(|e| format!("SAPI stop: {e}"))?;
    }
    Ok(())
}
```

Update `src-tauri/src/lib.rs`:
```rust
pub mod speech;
```

Update `src-tauri/src/tools/speak.rs`:
```rust
pub async fn run(args: &serde_json::Value) -> super::ToolResult {
    let text: String = args["text"].as_str().unwrap_or("").chars().take(500).collect();
    if text.is_empty() {
        return Err("speak: text must not be empty".into());
    }
    tokio::task::spawn_blocking(move || crate::speech::sapi_speak(&text))
        .await
        .map_err(|e| format!("speak task: {e}"))??;
    Ok("Speaking.".into())
}
```

---

## 8. Ordered Build Steps with Gates

| # | Step | Gate (must pass before next step) |
|---|------|----------------------------------|
| 0 | **STT probe:** Add a dev-only Settings button "Test Voice" that attempts `new webkitSpeechRecognition()` and logs result to console | Button click → console shows `"webkitSpeechRecognition available"` or error; if unavailable, STOP and discuss fallback with owner |
| 1 | Add `voice_replies_mode`, `stt_language` to `db.rs` Settings struct + schema | `cargo check --lib` clean |
| 2 | Create `src-tauri/src/speech.rs` with `sapi_speak` and `stop_sapi_speech` | `cargo check --lib` clean |
| 3 | Refactor `tools/speak.rs` to call `speech::sapi_speak` | `cargo test --lib` — speak tool test still passes |
| 4 | Add `speak_text` and `stop_speech` commands to `commands.rs` + register in `lib.rs` | `cargo check` clean |
| 5 | Update Settings.tsx with voice_replies_mode + stt_language inputs | `tsc` clean |
| 6 | Create `useVoiceInput.ts` hook | `tsc` clean |
| 7 | Create `ListeningBar.tsx` component + CSS | `tsc` clean; `npm run dev` → component renders (manually trigger listening state) |
| 8 | Integrate voice into App.tsx: `useVoiceInput` + `activateVoice` + `ListeningBar` render + Esc handler | `npm run dev` → mic tool click opens chat with listening bar; cancel works |
| 9 | Update `tools.ts` mic tool `live: true` | Tray shows active mic tool |
| 10 | Add TTS effect to `useChat.ts` (pass settings prop) | `tsc` clean |
| 11 | Add `stop_speech()` calls before send/cancel | `tsc` clean |
| 12 | `npm run build` + `cargo build --release` | No errors |

---

## 9. Acceptance Tests

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-V1 | Probe: Test Voice button in Settings | Console logs `"webkitSpeechRecognition available"` (if fails, fallback discussion required before continuing) |
| AT-V2 | Save voice settings | Settings → voice_replies_mode "Auto", stt_language "en-US" → Save → reload app → settings persist |
| AT-V3 | Mic tool activation | Tray → Mic → chat panel opens with listening bar showing "Listening…" + purple pulsing icon |
| AT-V4 | STT transcript capture | Click mic → say "What time is it" → interim text updates → final transcript appears in ask box → auto-sends |
| AT-V5 | Tool answer spoken aloud | After AT-V4 auto-send → agent calls `get_datetime` → answer streams → done → SAPI speaks "The current time is..." |
| AT-V6 | Cancel listening | Click mic → say nothing → click Cancel → listening bar closes, no send, AskBar enabled |
| AT-V7 | Esc during listening | Click mic → press Esc → listening bar closes, no send |
| AT-V8 | No-speech timeout | Click mic → wait 5 seconds in silence → error toast "No speech detected. Try again." → listening bar closes |
| AT-V9 | TTS mode "never" | Settings → voice_replies "Never" → Save → send any question → answer streams → done → no speech |
| AT-V10 | TTS mode "always" | Settings → voice_replies "Always" → Save → send "explain async/await in 3 paragraphs" → long answer streams → done → SAPI speaks entire answer |
| AT-V11 | Stop speech on new send | Send question → answer starts speaking → type new question and send → previous speech stops immediately, new answer starts |
| AT-V12 | Stop speech on cancel | Send question → stream starts → click Cancel → streaming stops AND speech stops |
| AT-V13 | Code block stripping | Send "write a hello world in python" → answer includes ```python block → TTS speaks explanation but NOT the code |

---

## 10. Out of Scope

- Wake word / hotword activation (Phase 2.7)
- Voice activity detection (VAD) for automatic start/stop (requires always-on mic, privacy concern)
- Streaming TTS (speak partial answers before `done`) — SAPI doesn't support streaming, would need cloud TTS
- Custom TTS voices (uses Windows default voice)
- Voice input for MCP clients (Door 2) — only Door 1 (GUI) gets voice
- Language auto-detection for STT (user sets language manually)
- Transcript editing before send (auto-send is the feature; user can type corrections after if wrong)
- Voice commands ("cancel", "stop", "clear") — only push-to-talk for questions
- Noise cancellation / audio preprocessing (relies on OS/hardware)

---

## 11. Risk Notes & Mitigations

1. **WebView2 STT availability**: If Step 0 probe fails, the spec's primary path is invalid. Mitigation: Probe first (gate 0); if unavailable, the orchestrator will write a Rust SAPI recognition impl or provider Whisper client before Step 1.

2. **Interim results lag**: WebView2 STT may batch interim results with 200–500ms lag. Mitigation: Accept the lag; interim display is a UX nicety, not critical. Final result is accurate.

3. **Accidental auto-send on bad transcript**: No confirm step means wrong transcripts auto-send. Mitigation: User can cancel the stream and type a correction. This is a known tradeoff for speed.

4. **Long answer TTS is annoying**: Even with `"auto"` mode, some answers may be ~250 chars but still verbose. Mitigation: Owner can toggle to `"never"` if TTS is more annoying than useful. The 300-char threshold is a starting heuristic; can be tuned post-launch.

5. **SAPI voice quality**: Windows default voice (David/Zira) is robotic. Mitigation: Out of scope for 2.4; owner can change voice in Windows Settings → Time & Language → Speech. Cloud TTS (Azure, ElevenLabs) is a future phase.

6. **No mic permission flow in WebView2**: If the user denies mic permission, WebView2 fires `onerror` with `"not-allowed"`, but there's no OS-level permission prompt like in a browser. Mitigation: Error message instructs user to check "browser settings" (WebView2 inherits Edge settings). If this is insufficient, we'll need a Rust command to open Windows Settings → Privacy → Microphone programmatically (future enhancement).

---

## 12. TypeScript Types to Add

File: `src/lib/types.ts`

```typescript
export type Settings = {
  // ... existing fields ...
  voice_replies_mode: "always" | "auto" | "never";
  stt_language: string;  // BCP-47 (e.g., "en-US")
};
```

---

**End of spec.**
