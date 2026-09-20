# Live Assistant Roadmap — sorted by Claude (Sonnet 4.6), 2026-09-17

> STATUS: AWAITING OWNER APPROVAL. Rules: Claude plans → orchestrator
> implements → Claude reviews → owner tests and approves each phase.

## The owner's idea, sorted into phases

**What you are building:** A voice-driven live screen assistant. You call its name, it starts watching your screen, you talk to it, it talks back, and it can do things on your computer. This is a big idea — maybe 4-5 months of work. The plan below breaks it into pieces you can approve and test one at a time. Each piece gives you something you can actually use.

---

### Phase 2.2 — Eyes Check All *(ship this week, already planned)*

**What you get:** Click the Eye tool → the assistant takes one screenshot of your whole screen → sends it to the AI → the answer streams into chat. A "Check again" button takes a fresh screenshot in the same conversation. That's it. Fast, simple, already in the plan.

**Why ship this now:** It gives you a working "see my screen" button today, with zero new infrastructure. Everything it needs (screenshot, vision chat, streaming) is already built and tested. It is the foundation every later phase builds on.

**Effort: S** (a few days). Unlock the Eye tool, wire it like the Screenshot tool but without the chooser: one button, one capture, straight to chat.

---

### Phase 2.3 — Window Switcher *(already planned, ship after 2.2)*

**What you get:** Click the Window Switcher tool → a list of your open apps → click one → that app comes to the front, the overlay hides.

**Effort: S.** Needed for the agent phase later (the assistant needs to know which windows are open).

---

### Phase 2.4 — Voice: Talk and Listen (no wake word yet)

**What you get:** Click the Mic tool → it listens to you → your words appear in the Ask box → send as normal. The assistant answers in text. Push-to-talk (click to start, click to stop or release). No always-on listening yet.

**Technical approach:**
- **Speech-to-text:** Use the browser's built-in `webkitSpeechRecognition` (available in WebView2 on Windows 11 — no new dependencies, no API key). It calls Windows Speech Recognition / Azure under the hood. Alternatively, call the provider's Whisper-compatible endpoint (POST audio → text) — same API key already in Credential Manager.
- **Text-to-speech (AI speaks back):** Windows SAPI via a small Rust command (`ISpVoice::Speak`). No internet, no extra key, always available. The assistant's reply text is passed to it after the stream completes.
- **Recommendation:** Start with `webkitSpeechRecognition` for STT (zero new deps, works offline for common speech) + Windows SAPI for TTS. Both are built into Windows. If the speech recognition quality is poor for the owner's accent/language, add the Whisper API endpoint as a setting.

**What can break:** `webkitSpeechRecognition` requires a microphone permission grant in WebView2 (a browser dialog appears once). SAPI voices sound robotic on older Windows — acceptable for now.

**Effort: M** (1-2 weeks).

---

### Phase 2.5 — Live Screen Watcher ("like a video record")

**What you get:** A "Watch my screen" button (or voice command after Phase 2.4). The assistant starts watching. Every time something on your screen changes, or every few seconds, it takes a screenshot and remembers what it saw. You talk to it: "what's on screen now?" → it answers from what it just saw. You can say "look again" and it captures a fresh shot. Press Esc or say "stop watching" to end.

**The honest technical answer about "like video":**
The AI provider (anymodel.org / any OpenAI-compatible endpoint) has **no live video input**. You cannot stream a video at it. What you CAN do is send it screenshots, and do so often enough that it feels continuous. Three realistic patterns:

| Pattern | How it works | Token cost | Recommendation |
|---|---|---|---|
| Capture-on-speak | Take a screenshot only when the user speaks a command | Lowest — one image per command | **This one** |
| Timed capture | Take a screenshot every N seconds while watching | Medium — adds up fast at $0.01+/image | Offer as a setting, default OFF |
| Change-detect then capture | Compare pixels; only capture when something moved | Low — captures only meaningful changes | Phase 3 enhancement |

**Recommended approach for Phase 2.5:** Capture-on-speak. When the user starts talking, take a screenshot immediately, attach it to the voice message, send both. The AI sees what was on screen at the exact moment the user spoke. This is the most useful and cheapest pattern. It is also the easiest to build: it reuses the existing screenshot → vision chat pipeline exactly.

Optional: add a "Watch mode" toggle that takes a screenshot every 5 seconds and keeps the last one in memory, so the AI can answer "what is on screen?" without a new capture. This is one extra timer + one stored base64 string.

**Effort: M** (2 weeks, after Phase 2.4).

---

### Phase 2.6 — The Assistant Speaks Your Name (Wake Word)

**What you get:** Say "Thuki" (or a chosen word) and the assistant activates — same as pressing Alt+Space, but with your voice. No button press needed.

**The honest technical answer:**
Wake word detection needs to run **always**, listening in the background. This is genuinely hard to do well.

| Option | How | Quality | Cost | Effort |
|---|---|---|---|---|
| Push-to-talk (existing hotkey) | Alt+Space already works | Perfect | Free | Zero (already built) |
| Windows Speech Recognition / SAPI keyword | `ISpRecoContext` with a custom grammar — "Thuki" → trigger | OK for English, unreliable for non-English | Free | M |
| Local Whisper (whisper.cpp) | Run a small model on-device, always recording, detect keyword | Good, any language | Needs a 150MB+ model file | L |
| Tauri plugin for OS hotword API | No mature plugin exists for Tauri v2 yet | — | — | L |

**Recommendation:** Keep Alt+Space as the primary activation for now. After Phase 2.4 (voice) is working and the owner uses it daily, decide whether the push-to-talk feel is good enough. If not, add SAPI keyword detection as a Rust background thread. Skip Whisper-local for now — it is a large download and adds build complexity.

**Effort: M** (if using SAPI keyword) or **defer indefinitely** (keep Alt+Space).

---

### Phase 3 — The Assistant Does Things (Agent Actions)

**What you get:** You say "open that file" or "click that button" → the assistant does it. This is the most powerful and most dangerous part of the idea.

**The honest danger:** An AI that can click things on your computer CAN delete files, send emails, or close important work if it misunderstands. This must be built carefully with confirmation steps.

**The safe ladder — build one rung at a time, owner approves each:**

| Rung | What the AI can do | Confirmation needed | Effort |
|---|---|---|---|
| A — Describe | "The Save button is in the top-left" | None | S |
| B — Show me where | Draws a highlight box on the screen over the element | None | M |
| C — Single approved action | "Click Save? YES/NO" → owner says yes → one click happens | Owner says YES every time | M |
| D — Short sequence | "Open File > Save As > type the name > click OK" — shows each step before doing it | Owner approves the list before it starts | L |
| E — Autonomous | Runs a whole task without asking | NEVER without rung D working perfectly for months | — |

**Technical pieces needed for Rung C+:**
- **Read the screen:** Windows UI Automation (UIA) via the `uiautomation` crate — reads element names, roles, and bounding boxes from running apps without taking a screenshot. This is what Inspect (Phase 2.5 stub) uses. Required for reliable clicking.
- **Send input:** Win32 `SendInput` (keyboard) and `SetCursorPos` + `mouse_event` (mouse) from Rust. This is how the OS simulates real input. Already partly used in the test scripts.
- **Confirmation UX:** Every proposed action shows in the chat as a "card" — the owner clicks YES or NO. The action only fires after YES. No exceptions.

**Effort: L** (1-2 months for Rungs A-C). Rung D is another L on top. Rung E is never recommended.

---

## Recommended order (overall)

```
NOW:     2.2 Eyes Check All          (small win, ships the screen-watching foundation)
NEXT:    2.3 Window Switcher         (small win, needed for agent later)
THEN:    2.4 Voice — push-to-talk    (talk + listen, no wake word)
THEN:    2.5 Live Screen Watcher     (capture-on-speak, optional timed mode)
THEN:    2.6 Wake Word (if wanted)   (SAPI keyword, Alt+Space stays as fallback)
FUTURE:  3.A-C Agent actions         (one rung at a time, owner approves each)
```

At the end of Phase 2.5 the owner has: say something → it sees your screen → it answers by voice. That is the core of the idea, working. Phase 3 adds the ability to act, carefully.

---

## Technical notes (for the implementer)

- **No new dependencies for Phase 2.2:** pure reuse of existing capture + vision pipeline.
- **Phase 2.4 STT:** `webkitSpeechRecognition` is a JS Web API, works in WebView2, zero Rust. Tauri's `tauri-plugin-process` is NOT needed. One new Rust command for SAPI TTS (`ISpVoice`) using the `windows` crate already in the project.
- **Phase 2.5 watcher timer:** a `setInterval` in JS (or a Tokio interval in Rust emitting `screen://tick` events) — either works. JS interval is simpler.
- **Phase 2.6 SAPI keyword:** a dedicated background Rust thread (not async — SAPI is COM, needs a thread with a message pump) that calls `ISpRecoContext::CreateGrammar`, adds "Thuki" as a rule, and emits a Tauri event when recognized. This thread must not block the main window.
- **Phase 3 UIA:** the `uiautomation` crate (already planned for the Inspect stub). `SendInput` is in the `windows` crate already referenced.
- **Token cost warning:** at standard vision model pricing, sending a full-screen PNG every 5 seconds costs roughly $2-5/hour depending on provider. Capture-on-speak costs near zero unless the user is very talkative. Surface this clearly in the Settings page when timed capture is added.
- **One chat connection rule:** the existing single-connection SSE client handles vision already. The watcher mode reuses it — no second connection, no parallel streams.
--- end report ---
