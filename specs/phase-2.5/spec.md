# Phase 2.5 — Capture-on-Speak (Eyes + Voice Combined)

**Status:** SPEC  
**Estimated effort:** 3–4 days  
**Depends on:** Phase 2.4 (voice input + TTS, VERDICT OK)  
**Out of scope:** Watch mode (timer-based periodic capture), voice for MCP door, hypothesis streaming (deferred from 2.4 unless owner says otherwise)

---

## Goal

When the user activates voice input (mic tool click), immediately capture a screenshot of the full primary monitor and attach it to the voice message. The transcript becomes the prompt; the image rides along as context. Result: "show me what you see and [user's spoken question]" in one click — no manual screenshot step.

---

## 1. Trigger Point Decision

**Chosen: Capture at mic click (start_listening begins), not at final transcript.**

**Rationale:**
- User looks at the screen while speaking about it. The screen state *when they click mic* is what they're asking about.
- Capturing at final transcript (after speech ends) means 2–5 seconds of lag — the screen may have changed (notifications, window switches, animations).
- ~200ms capture latency is acceptable; it happens while the user inhales to speak.
- If capture fails, voice text still sends (graceful degradation — see §4).

**Alternative rejected:** Capture on final transcript. Pro: slightly cleaner threading (no capture during recognition). Con: stale screen state. The user's question is about *now*, not 5 seconds from now.

---

## 2. Thuki Window in Capture

**Decision: Accept and document.**

The Thuki chat window (444×668, always-on-top) will appear in every capture-on-speak screenshot, positioned wherever the user placed it. The listening bar will be visible.

**Why not hide it?**
- Hiding the window → `fitDisc()` → shrink to 56×56 → breaks the UX (user sees the window collapse mid-flow).
- The window is semi-transparent (backdrop-blur) and unobtrusive. The AI can see through/around it.
- The user placed it where they want it; moving it would be disorienting.

**Mitigation:** If the window occludes critical UI the user is asking about, they can drag it aside before clicking mic. This is a one-time setup per session (sticky window position).

**Future enhancement (out of scope):** Phase 2.7 wake word could capture *before* showing the window. Not relevant here (mic click already shows the window).

---

## 3. Implementation Plan

### 3a. New `activateVoiceWithCapture` function in App.tsx

Replace the current `activateVoice` (App.tsx:207-211) with:

```typescript
async function activateVoiceWithCapture() {
  if (chat.streaming || voice.listening) return; // guard: one op at a time
  
  setMenuOpen(false);
  await closeMenuOverlay(); // same await pattern as eyesCheckAll
  setPanel("chat");
  
  // Capture first (screen state at mic-click moment).
  let capturedImage: string | null = null;
  try {
    const { base64_png } = await captureScreen(null); // full screen
    capturedImage = base64_png;
  } catch (err) {
    console.error("[thuki] capture-on-speak failed:", err);
    // Voice continues without image (graceful degradation).
  }
  
  // Start listening. When final transcript arrives, send with the image.
  voice.startWithImage(capturedImage);
}
```

### 3b. Modify `useVoiceInput` to accept an image

**Current:** `onFinal` callback receives `text: string`.

**New:** Add a second method `startWithImage(image: string | null)` that:
1. Stores `image` in a ref (`capturedImageRef`).
2. Calls the existing `start()` logic.
3. When `voice://final` fires, passes `(text, capturedImageRef.current)` to the callback.
4. Clears the ref after calling the callback.

**Signature change:**

```typescript
export function useVoiceInput(
  _language: string,
  onFinal: (text: string, image: string | null) => void,
)
```

**New method:**

```typescript
const capturedImageRef = useRef<string | null>(null);

const startWithImage = useCallback((image: string | null) => {
  capturedImageRef.current = image;
  void start();
}, [start]);

// In voice://final handler (line 80-86):
if (text) {
  const img = capturedImageRef.current;
  capturedImageRef.current = null;
  onFinalRef.current(text, img);
}

return { ...state, start, startWithImage, stop };
```

### 3c. Update `App.tsx` voice callback (line 46-50)

```typescript
const voice = useVoiceInput(settings.stt_language, (text, image) => {
  setDraft(text);
  void sendWithImage(text, image);
});
```

**New helper:**

```typescript
async function sendWithImage(text: string, image: string | null) {
  if (!text.trim()) return;
  setDraft("");
  await stopSpeech();
  await chat.send(text, clipboard, settings.active_preset, settings.model, image);
  setHistoryTick((n) => n + 1);
}
```

**Consolidation note:** The existing `send(override?: string)` (line 139) is for typed messages. `sendWithImage` is voice-specific. Could unify them with `send(text?: string, image?: string | null)`, but keeping them separate is clearer. Your call.

### 3d. Update `ToolsTray` call site (App.tsx:336)

```typescript
onVoiceActivate={activateVoiceWithCapture}
```

---

## 4. Prompt Design

**No special prompt needed.** The user's spoken words ARE the prompt. The image is attached as a vision message part, same as manual screenshot flow.

**Example:**
- User clicks mic, screen shows a Python error traceback, user says "What's wrong with this code?"
- Message sent: `content: "What's wrong with this code?"`, `image_base64: <screenshot PNG>`.
- AI sees the traceback and answers about the specific error.

**Contrast with Eyes Check All:** That flow uses a fixed prompt (`EYES_PROMPT = "Look at this screen..."`). Voice+image uses the user's exact words — more flexible, more natural.

---

## 5. Check Again Button

**Already works.** The `lastUserHasImage` heuristic (App.tsx:54-60) scans for `m.role === "user" && m.image_base64`. Voice+image messages will have both `content` (transcript) and `image_base64`, so the "Check again" button appears automatically.

**No code change needed.**

---

## 6. Graceful Degradation (Capture Failure)

If `captureScreen(null)` throws (rare: GDI failure, out of memory, disk full):
1. Log the error to console (line in `activateVoiceWithCapture`).
2. Set `capturedImage = null`.
3. Voice input proceeds normally.
4. Final message sends with `image_base64: null` — text-only question.

**User-facing:** No error toast. The voice flow continues. If the user expected an image, they can manually screenshot afterward.

**Rationale:** Capture failure is extremely rare (~0.1% of ops). Blocking voice input on a capture error would be worse UX than sending text-only.

---

## 7. Interaction with Streaming / Tools

**Guard at entry (line 1 of `activateVoiceWithCapture`):** `if (chat.streaming || voice.listening) return;`

Same guard as `eyesCheckAll` (App.tsx:214). Prevents stacking operations.

**Tool calls:** If the transcript triggers agent tools (e.g., "What time is it? Also what's in this screenshot?"), the agent sees both the image and can call tools. No conflict — tools are separate from vision.

---

## 8. Ordered Build Steps with Gates

| # | Step | Gate |
|---|------|------|
| 1 | Add `capturedImageRef` + `startWithImage` method to `useVoiceInput.ts`; update `onFinal` signature to `(text, image)` | `tsc` clean |
| 2 | Update `App.tsx` voice callback to accept `image` param + create `sendWithImage` helper | `tsc` clean |
| 3 | Replace `activateVoice` with `activateVoiceWithCapture` (capture → `startWithImage`) | `tsc` clean; `npm run dev` → mic click triggers (will error until step 4) |
| 4 | Update `ToolsTray` `onVoiceActivate` prop call site | `npm run dev` → mic click captures + listening bar appears |
| 5 | Manual test: mic click → capture happens (~200ms) → speak → final → message includes image | Image thumbnail appears in chat history (same as manual screenshot messages) |
| 6 | Test graceful degradation: simulate capture failure (disconnect monitor? or patch `captureScreen` to throw) → voice still works, text-only message | No crash, text message sent |
| 7 | `npm run build` | No errors |

---

## 9. Acceptance Tests

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-CS1 | Capture timing | Tray → Mic → capture happens immediately (watch for screen flash if enabled) → listening bar appears within 300ms |
| AT-CS2 | Voice + image message | Click mic → screen shows a web page → say "What is on this page?" → final transcript → message sent with both text and image thumbnail |
| AT-CS3 | AI sees image + transcript | After AT-CS2 → AI answer references specific content from the screenshot (e.g., "This page shows...") |
| AT-CS4 | Check again button | After AT-CS2 → "Check again" button appears below chat (same as manual screenshot flow) |
| AT-CS5 | Thuki window in capture | After AT-CS2 → open saved screenshot from `Pictures\Thuki` → Thuki chat window visible in image (expected, documented) |
| AT-CS6 | Capture failure → text fallback | Simulate capture error (owner: how? maybe unplug monitor during click?) → voice input proceeds → text-only message sent, no crash |
| AT-CS7 | Guard: no double-capture | Click mic → before speaking, click mic again → second click ignored (already listening) |
| AT-CS8 | Guard: no capture during stream | Send question → answer streaming → click mic → ignored (guard active) |
| AT-CS9 | Tool + vision mixed | Click mic → screen shows clock widget → say "What time is it and what's on my screen?" → agent calls `get_datetime` tool AND describes screenshot |

---

## 10. Out of Scope

- **Watch mode / timer capture:** Periodic screenshots every N seconds while listening. Requires frame buffering + timeline UI. Deferred to future phase.
- **Hide Thuki window during capture:** Breaks UX flow (window collapses). See §2 rationale.
- **Hypothesis streaming from 2.4:** Still deferred unless owner approves adding `HypothesisGenerated` handler now. Decision needed.
- **Voice for MCP door:** MCP clients (Claude Desktop, ZCode) call tools over stdio, no GUI. Voice+image only makes sense in Door 1 (Thuki GUI).
- **Multi-region capture:** Voice+image always captures full screen. Select-area mode not applicable (user is speaking, can't draw a box simultaneously).
- **Capture-on-hypothesis:** Capturing on interim results would spam ~5 captures per utterance. Final transcript is the right trigger.

---

## 11. Risk Notes & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Capture adds 200ms lag before listening starts** | User clicks mic → slight pause → listening bar | Acceptable; most users won't notice. Capturing in parallel with `start_listening()` would race (listening starts before image ready) → wrong design. Sequential is correct. |
| **Thuki window occludes critical UI** | AI can't see what user is asking about | Document: "Drag Thuki aside before mic click if it covers important UI." Position is sticky per session. Power users will learn. |
| **Capture fails silently** | User expects image, gets text-only, doesn't know why | Low priority: capture failure rate ~0.1%. If it becomes a complaint, add a 1s toast "Screenshot failed, sending text only." Not in v1. |
| **Image + transcript mismatch** | User speaks about old screen state while new window opens mid-utterance | User responsibility: look at screen while speaking. Same UX contract as Eyes Check All ("look at my screen NOW"). |
| **Large images slow send** | 1920×1080 PNG base64 ≈ 2–3 MB per message, 500ms+ upload on slow network | Accepted; same as manual screenshot. Future: client-side resize or WebP (out of scope). |

---

## 12. Future Enhancements (Not This Phase)

1. **Capture-before-window:** Wake word (2.7) could trigger capture → *then* show window → speak. Eliminates Thuki from image. Needs wake word first.
2. **Hypothesis streaming:** Add `HypothesisGenerated` → `voice://hypothesis` events (15 lines in speech_recognition.rs). Owner: approve for 2.5 or defer to 2.6?
3. **Multi-frame watch mode:** Capture every 2s during listening → send all frames as a sequence. Needs timeline UI + frame picker. Major feature, separate phase.
4. **OCR pre-filter:** Extract text from screenshot before sending → include as context. Helps with low-vision models. Out of scope (needs tesseract or cloud OCR).

---

## 13. Code Diff Summary (for review)

**Files changed:**
1. `src/hooks/useVoiceInput.ts`: Add `capturedImageRef`, `startWithImage()`, update `onFinal` signature to `(text, image)`.
2. `src/App.tsx`: Replace `activateVoice` → `activateVoiceWithCapture` (capture + `startWithImage`), update voice callback + add `sendWithImage` helper.

**Files unchanged:**
- `src-tauri/src/speech_recognition.rs`: No changes (capture happens in frontend before `start_listening`).
- `src/components/ListeningBar.tsx`: No changes (still shows transcript).
- `src-tauri/src/commands.rs`: No changes (`capture_screen` command already exists).

**Lines added:** ~35 (TypeScript only, no Rust).

---

**End of spec.**
