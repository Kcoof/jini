# Phase 2.4 STT Fallback B — Windows SAPI Speech Recognition (ISpRecoContext)

**Status:** SPEC (replaces Primary path from phase-2.4/spec.md §1–§6)  
**Context:** WebView2 `webkitSpeechRecognition` probe FAILED (network error, backend absent). Z.AI ASR requires paid plan (1113 balance error). anymodel Whisper unavailable. Fallback B (Windows SAPI local recognition) is now primary STT path.  
**Scope:** STT implementation only. TTS (speech.rs, speak_text/stop_speech), settings (voice_replies_mode, stt_language), ListeningBar, App integration — all UNCHANGED from original spec.

---

## 1. Architecture Overview

**Rust**: New `speech_recognition.rs` module with two commands:
- `start_listening()` → spawns a dedicated thread with SAPI `ISpRecoContext`, streams events to frontend.
- `stop_listening()` → signals the thread to release audio and terminate cleanly.

**Frontend**: `useVoiceInput.ts` rewritten to invoke Rust commands and listen to Tauri events instead of `webkitSpeechRecognition`. Public API (start/stop/listening/transcript/error) unchanged.

**Threading model**: SAPI recognition runs on a dedicated OS thread (not tokio) with its own COM apartment (`CoInitializeEx(COINIT_APARTMENTTHREADED)` — SAPI SR requires STA, not MTA). Event loop blocks on `WaitForSingleObject` until stop signal or error.

---

## 2. Rust Implementation: `src-tauri/src/speech_recognition.rs`

### 2a. Module structure

```rust
//! Windows SAPI speech recognition (specs/phase-2.4 STT Fallback B).
//! ISpRecoContext with dictation grammar, mic input from default device.
//! Streams hypothesis (interim) + recognition (final) events to frontend.

use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Speech::{
    ISpRecoContext, ISpRecoGrammar, ISpRecognizer, SpInprocRecognizer, SpSharedRecoContext,
    SPEI_HYPOTHESIS, SPEI_RECOGNITION, SPEI_END_SR_STREAM, SPEVENTENUM,
    SPLOADOPTIONS, SPSTATEHANDLE, SPDKL_DefaultLocation, SPLO_STATIC, SPRS_ACTIVE,
    SPCS_ENABLED, ISpeechRecoContext, ISpEventSource,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, WAIT_OBJECT_0, INFINITE};

// Shared state: the stop event handle + thread join handle.
static LISTENER_STATE: Mutex<Option<ListenerState>> = Mutex::new(None);

struct ListenerState {
    stop_event: HANDLE,
    thread: Option<thread::JoinHandle<()>>,
}
```

### 2b. `start_listening` command

**Signature:**
```rust
#[tauri::command]
pub fn start_listening(app: AppHandle) -> Result<(), String>
```

**Logic:**
1. Check if already listening → return `Err("Already listening")`.
2. Create a Win32 manual-reset event (`CreateEventW(None, true, false, None)`) for the stop signal.
3. Clone `app` handle for the thread.
4. Spawn `std::thread::spawn(move || listen_thread(app_clone, stop_event_handle))`.
5. Store `ListenerState { stop_event, thread }` in the global mutex.
6. Return `Ok(())` immediately (non-blocking).

### 2c. `listen_thread` function

**Signature:**
```rust
fn listen_thread(app: AppHandle, stop_event: HANDLE)
```

**Steps:**

1. **COM init (STA):**
   ```rust
   unsafe {
       CoInitializeEx(None, COINIT_APARTMENTTHREADED)
           .map_err(|e| eprintln!("[SR] CoInitializeEx failed: {e}"));
   }
   ```

2. **Create recognizer + context:**
   ```rust
   let recognizer: ISpRecognizer = CoCreateInstance(&SpSharedRecoContext, None, CLSCTX_ALL)
       .map_err(|e| { emit_error(&app, &format!("SAPI recognizer init: {e}")); return; })?;
   let context: ISpRecoContext = recognizer.CreateRecoContext()
       .map_err(|e| { emit_error(&app, &format!("CreateRecoContext: {e}")); return; })?;
   ```

3. **Set input to default microphone:**
   ```rust
   recognizer.SetInput(None, true)  // None = default audio input, true = allow format changes
       .map_err(|e| { emit_error(&app, &format!("SetInput (mic): {e}")); return; })?;
   ```

4. **Load dictation grammar:**
   ```rust
   let grammar: ISpRecoGrammar = context.CreateGrammar(0)
       .map_err(|e| { emit_error(&app, &format!("CreateGrammar: {e}")); return; })?;
   grammar.LoadDictation(None, SPLO_STATIC)  // None = default topic
       .map_err(|e| { emit_error(&app, &format!("LoadDictation: {e}")); return; })?;
   grammar.SetDictationState(SPRS_ACTIVE)
       .map_err(|e| { emit_error(&app, &format!("SetDictationState: {e}")); return; })?;
   ```

5. **Set event interest + notification:**
   ```rust
   let events = SPEI_HYPOTHESIS | SPEI_RECOGNITION | SPEI_END_SR_STREAM;
   let event_source: ISpEventSource = context.cast()
       .map_err(|e| { emit_error(&app, &format!("Cast to ISpEventSource: {e}")); return; })?;
   event_source.SetInterest(events.0 as u64, events.0 as u64)
       .map_err(|e| { emit_error(&app, &format!("SetInterest: {e}")); return; })?;
   
   let notify_event = CreateEventW(None, false, false, None)  // auto-reset event
       .map_err(|e| { emit_error(&app, &format!("CreateEventW (notify): {e}")); return; })?;
   event_source.SetNotifyWindowMessage(0, 0, 0, notify_event)  // hwnd=0 → event-based
       .map_err(|e| { emit_error(&app, &format!("SetNotifyWindowMessage: {e}")); return; })?;
   ```

6. **Event loop:**
   ```rust
   let handles = [stop_event, notify_event];
   loop {
       let wait_result = WaitForSingleObject(handles.as_ptr() as HANDLE, INFINITE);
       match wait_result {
           WAIT_OBJECT_0 => {
               // stop_event signaled
               break;
           }
           WAIT_OBJECT_0 + 1 => {
               // notify_event → speech event available
               loop {
                   let mut event = SPEVENT::default();
                   let hr = event_source.GetEvents(1, &mut event, std::ptr::null_mut());
                   if hr.is_err() || event.eEventId == 0 {
                       break;  // no more events
                   }
                   match SPEVENTENUM(event.eEventId as i32) {
                       SPEI_HYPOTHESIS => {
                           if let Ok(text) = extract_result_text(&context, event) {
                               let _ = app.emit("voice://hypothesis", json!({ "text": text }));
                           }
                       }
                       SPEI_RECOGNITION => {
                           if let Ok(text) = extract_result_text(&context, event) {
                               let _ = app.emit("voice://final", json!({ "text": text }));
                           }
                       }
                       SPEI_END_SR_STREAM => {
                           let _ = app.emit("voice://end", json!({}));
                           break;  // stream ended (mic lost, etc.)
                       }
                       _ => {}
                   }
                   event_source.FreeEvent(event);
               }
           }
           _ => {
               emit_error(&app, "WaitForSingleObject failed");
               break;
           }
       }
   }
   ```

7. **Cleanup:**
   ```rust
   let _ = grammar.SetDictationState(SPRS_INACTIVE);
   let _ = CloseHandle(notify_event);
   CoUninitialize();
   let _ = app.emit("voice://end", json!({}));
   ```

### 2d. `extract_result_text` helper

```rust
fn extract_result_text(context: &ISpRecoContext, event: SPEVENT) -> Result<String, String> {
    unsafe {
        let result = context.GetRecognitionResult(event.lParam as usize)
            .map_err(|e| format!("GetRecognitionResult: {e}"))?;
        let mut text_ptr: PWSTR = PWSTR::null();
        result.GetText(SP_GETWHOLEPHRASE, SP_GETWHOLEPHRASE, true, &mut text_ptr, None)
            .map_err(|e| format!("GetText: {e}"))?;
        if text_ptr.is_null() {
            return Err("GetText returned null".into());
        }
        let len = (0..).take_while(|&i| *text_ptr.0.offset(i) != 0).count();
        let slice = std::slice::from_raw_parts(text_ptr.0, len);
        let text = String::from_utf16_lossy(slice);
        CoTaskMemFree(text_ptr.0 as *mut _);
        Ok(text)
    }
}
```

### 2e. `stop_listening` command

**Signature:**
```rust
#[tauri::command]
pub fn stop_listening() -> Result<(), String>
```

**Logic:**
1. Lock `LISTENER_STATE` mutex.
2. If `None` → return `Ok(())` (not listening).
3. Signal `stop_event` with `SetEvent`.
4. Take the `thread` handle, drop the mutex lock, then `thread.join()`.
5. Close `stop_event` handle with `CloseHandle`.
6. Set `LISTENER_STATE` back to `None`.

### 2f. `emit_error` helper

```rust
fn emit_error(app: &AppHandle, message: &str) {
    eprintln!("[SR] {}", message);
    let _ = app.emit("voice://error", json!({ "message": message }));
}
```

---

## 3. Cargo.toml Changes

**Add to `windows` features** (line 32):

```toml
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_Graphics_Gdi",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Threading",
  "Win32_System_ProcessStatus",
  "Win32_Media_Speech",
  "Win32_System_Com",
  "Win32_System_Memory",  # NEW: for CoTaskMemFree
] }
```

**No new dependencies needed.** All types exist in `windows` 0.58.

---

## 4. Commands Registration

In `src-tauri/src/lib.rs`:

```rust
pub mod speech_recognition;

// In invoke_handler! (line 82+):
commands::start_listening,
commands::stop_listening,
```

In `src-tauri/src/commands.rs`:

```rust
// Re-export from speech_recognition module:
pub use crate::speech_recognition::{start_listening, stop_listening};
```

---

## 5. Frontend: `useVoiceInput.ts` Rewrite

**File:** `src/hooks/useVoiceInput.ts`

Replace lines 1–139 with:

```typescript
// Push-to-talk speech input via Windows SAPI (specs/phase-2.4 STT Fallback B).
// Single utterance, hypothesis (interim) + final events, 5s silence timeout.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

type VoiceState = {
  listening: boolean;
  transcript: string;
  error: string | null;
};

const SILENCE_TIMEOUT_MS = 5000;

export function useVoiceInput(_language: string, onFinal: (text: string) => void) {
  const [state, setState] = useState<VoiceState>({
    listening: false,
    transcript: "",
    error: null,
  });
  const timeoutRef = useRef<number | undefined>(undefined);
  const onFinalRef = useRef(onFinal);
  
  useEffect(() => {
    onFinalRef.current = onFinal;
  }, [onFinal]);

  const clearTimer = () => window.clearTimeout(timeoutRef.current);

  const armSilenceTimer = useCallback(() => {
    clearTimer();
    timeoutRef.current = window.setTimeout(() => {
      void invoke("stop_listening");
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
    await invoke("stop_listening");
    setState({ listening: false, transcript: "", error: null });
  }, []);

  // Listen to SAPI events
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    async function attach() {
      unlisteners.push(
        await listen<{ text: string }>("voice://hypothesis", (event) => {
          if (cancelled) return;
          setState((prev) => ({ ...prev, transcript: event.payload.text }));
          armSilenceTimer();  // reset timeout on each hypothesis
        }),
      );
      unlisteners.push(
        await listen<{ text: string }>("voice://final", (event) => {
          if (cancelled) return;
          clearTimer();
          const text = event.payload.text.trim();
          setState({ listening: false, transcript: text, error: null });
          onFinalRef.current(text);
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
          setState((prev) => ({ ...prev, listening: false }));
        }),
      );
    }

    void attach();
    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, [armSilenceTimer]);

  // Esc cancels listening
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

  return { ...state, start, stop };
}
```

**Note:** `language` parameter is now ignored (prefixed with `_`). SAPI dictation uses the Windows system recognizer language (typically en-US unless the user installed another language pack). Document this limitation in a comment.

---

## 6. Event Payloads (TypeScript types)

Add to `src/lib/types.ts`:

```typescript
export type VoiceHypothesis = { text: string };  // interim
export type VoiceFinal = { text: string };       // final transcript
export type VoiceError = { message: string };
export type VoiceEnd = {};                       // stream ended
```

---

## 7. Silence Timeout Behavior

**Frontend timer (unchanged from original spec):** `useVoiceInput` arms a 5-second timer on `start()` and resets it on every `voice://hypothesis` event. If the timer fires → call `stop_listening()` + show "No speech detected" error.

**Why frontend, not SAPI?** SAPI's `SPEI_END_SR_STREAM` fires on mic loss or explicit stop, not on silence. The silence heuristic is application logic, not engine behavior.

---

## 8. Acceptance Tests (Revised)

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-STT1 | SAPI availability | `cargo build` succeeds; `start_listening` command exists |
| AT-STT2 | Mic tool → listening bar | Tray → Mic → chat panel opens, listening bar shows "Listening…" + purple pulse |
| AT-STT3 | Hypothesis updates | Click mic → speak slowly "what time is it" → interim text updates in real-time in listening bar |
| AT-STT4 | Final transcript + auto-send | Continue AT-STT3 → pause → final transcript appears in ask box → auto-sends |
| AT-STT5 | Tool answer spoken aloud | After AT-STT4 → agent calls `get_datetime` → answer streams → done → SAPI TTS speaks "The current time is..." |
| AT-STT6 | Cancel listening | Click mic → say nothing → click Cancel → listening bar closes, no send |
| AT-STT7 | Esc during listening | Click mic → press Esc → `stop_listening` called, bar closes |
| AT-STT8 | Silence timeout | Click mic → wait 5 seconds → error toast "No speech detected", bar closes |
| AT-STT9 | Microphone permission | If no mic connected or disabled → SAPI `SetInput` fails → `voice://error` event → error message displayed |
| AT-STT10 | Stop cleans up thread | Click mic → speak → final result → wait 2s → click mic again → second session starts (no "Already listening" error) |

**Unchanged from original spec:** AT-V9 (TTS never), AT-V10 (always), AT-V11 (stop speech on new send), AT-V12 (stop on cancel), AT-V13 (code block stripping).

---

## 9. Ordered Build Steps with Gates

| # | Step | Gate |
|---|------|------|
| 1 | Add `Win32_System_Memory` to Cargo.toml windows features | `cargo check --lib` clean |
| 2 | Create `src-tauri/src/speech_recognition.rs` skeleton (empty `start_listening`/`stop_listening` stubs returning `Ok(())`) | `cargo check` clean |
| 3 | Add `pub mod speech_recognition;` to `lib.rs` + register commands | `cargo check` clean |
| 4 | Implement `start_listening` → thread spawn → COM init → recognizer/context creation (no event loop yet) → emit `voice://end` → cleanup | `cargo build` succeeds; manual `invoke("start_listening")` from dev console → no panic, `voice://end` event fires |
| 5 | Add dictation grammar loading + `SetInput(None, true)` | `cargo build`; call starts without error (mic LED may light up) |
| 6 | Add event interest + notification setup + event loop (log events to stderr, no emits yet) | `cargo build`; speak into mic → stderr shows `[SR] SPEI_HYPOTHESIS` / `SPEI_RECOGNITION` breadcrumbs |
| 7 | Implement `extract_result_text` + emit `voice://hypothesis` / `voice://final` | Speak → frontend console (via `listen()` test) shows hypothesis + final text |
| 8 | Implement `stop_listening` (signal + join) | Call `stop_listening()` during recognition → thread exits cleanly, no hang |
| 9 | Rewrite `useVoiceInput.ts` per §5 | `tsc` clean; `npm run dev` → mic tool click → listening bar appears |
| 10 | Test full flow: mic click → speak → interim updates → final → auto-send | AT-STT3 + AT-STT4 pass |
| 11 | Test cancel + Esc | AT-STT6 + AT-STT7 pass |
| 12 | Test silence timeout | AT-STT8 passes |

---

## 10. Risk Notes & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **SAPI dictation quality lower than cloud STT** | Transcription errors on accents, background noise, fast speech | Accept as limitation of local STT; user can type corrections. Document that cloud STT (future phase) will improve accuracy. |
| **System recognizer language mismatch** | User sets `stt_language: "zh-CN"` in settings but Windows has only en-US recognizer installed → recognition still runs in English | Document that SAPI uses the Windows system language. Add a warning in Settings UI: "Speech language setting is ignored; Windows SAPI uses your system recognizer language (typically en-US)." Remove the input field or make it read-only displaying "System default". |
| **Microphone not connected or disabled** | `SetInput` fails → thread emits error and exits | Handled by `voice://error` event → error message shown to user. Clear UX. |
| **Thread doesn't join on app exit** | If user quits app while listening, thread may leak or COM cleanup incomplete | Add `drop()` impl for `LISTENER_STATE` or app shutdown hook that calls `stop_listening()` before exit. Low priority (OS cleans up process resources anyway). |
| **`SpSharedRecoContext` vs `SpInprocRecognizer`** | Shared engine may conflict with other apps using SAPI recognition | Start with `SpSharedRecoContext` (lighter weight). If conflicts arise (e.g., mic busy), switch to `SpInprocRecognizer` in a follow-up. Both have identical API. |
| **Long pauses mid-sentence → premature final result** | SAPI may finalize after 1–2s silence, cutting off multi-sentence input | Accept as SAPI behavior; single-utterance mode is intentional for fast one-question flow. User can click mic again for follow-up. |
| **Event loop blocks app exit** | `WaitForSingleObject(INFINITE)` never returns if stop_event not signaled | Always call `stop_listening()` in `useVoiceInput` cleanup (`useEffect` return). Belt-and-suspenders: add 5s timeout to `WaitForSingleObject` and check a separate `running` flag. |

---

## 11. Out of Scope (Unchanged from Original Spec)

- Wake word (Phase 2.7)
- Voice activity detection (VAD)
- Streaming TTS mid-answer
- Custom TTS/STT voices (uses Windows defaults)
- Language selection for STT (SAPI uses system language)
- Transcript editing before send (auto-send is the feature)
- Voice commands ("cancel", "stop")
- Cloud STT fallback (Whisper, Azure, Google) — future enhancement

---

## 12. Known Limitations (Document in Code)

1. **Language:** SAPI dictation ignores the `stt_language` setting and uses the Windows system recognizer language (typically en-US). Users must install additional language packs via Windows Settings → Time & Language → Speech to use other languages.

2. **Quality:** Local SAPI recognition is less accurate than cloud STT (WebView2, Whisper, Google). Expect ~85–90% accuracy in quiet environments with clear speech. Background noise, accents, and fast speech reduce accuracy.

3. **Single utterance:** One continuous phrase per activation. Long pauses (>1–2s) trigger finalization. For multi-sentence input, click mic multiple times.

4. **Mic exclusivity:** While listening, the microphone may be unavailable to other apps (depends on audio driver). This is normal for SAPI in-process mode.

---

**End of spec.**
