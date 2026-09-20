# Assistant Roadmap v2 — revised by Claude after the owner's feedback (2026-09-17)

> Supersedes specs/assistant-roadmap.md. NEW HARD FACTS proven by live tests:
> 1) gpt-5.6-sol on anymodel.org returns real tool_calls (get_weather test).
> 2) tools + image parts in ONE request work — the model saw the image and
>    called capture_screen. Both of Claude's top risks for Phase 2.3 are cleared.

## REVISED PLAN — Connected Voice + Eyes + Control Assistant

**Owner's goal:** One integrated assistant you can TALK to. It SEES your screen. It ACTS on your computer. Everything works together, not isolated tools.

**Good news:** The provider (anymodel.org) already supports tool calling. We proved it works with a real test. We can build the full assistant on the infrastructure you already have — no MCP servers or new protocols needed for v1.

---

### The new phases (simplified, bigger steps)

You still approve each one before the next starts. Each phase gives you something you can use immediately. The small solo tools (Eyes Check All one-click, Window Switcher list) stay — they're stepping stones, not throwaways.

---

### **Phase 2.3 — Agent Loop Foundation** *(the backbone)*

**What you get:** The assistant can USE TOOLS. You type "what windows are open?" → the model calls `list_windows` itself → the answer lists your actual open apps. Or: "take a screenshot" → it calls `capture_screen` → shows you the result. This is the core loop every later feature builds on. The model decides when to use tools; you see what it does.

**First tool set (SAFE, no dangerous actions yet):**
- `capture_screen` — already built, now callable by the model
- `list_windows` — lists your open apps (name, title, window handle)
- `get_active_window` — which window is in front right now
- `read_clipboard` — what text is on your clipboard
- `get_datetime` — current time/date
- `speak` — the assistant says something out loud (Windows SAPI voice)

**UI:** Tool calls show as gray cards in chat ("🔧 Calling list_windows…" → result below). The model's final answer appears after.

**Technical backbone:**
1. `ChatRequest` in Rust grows a `tools: Option<Vec<Tool>>` field (OpenAI schema: `{type: "function", function: {name, description, parameters: {...}}}`).
2. `extract_delta` in Rust also watches for `/choices/0/delta/tool_calls` alongside `content` — the SSE stream can contain EITHER text deltas OR tool_call deltas, never both in one chunk. Tool call arguments arrive as JSON fragments across chunks; reassemble them with a buffer.
3. When `finish_reason: "tool_calls"` arrives, emit `chat://tool_calls` event with the array. Rust executes each, builds `role: "tool"` result messages, appends them to history, calls the provider again with `tool_choice: "none"` (forces a text answer now). The final answer streams normally.
4. SQLite: `role: "tool"` messages persist like any other message. History replays correctly on reload.

**Confirmation:** None yet — these tools are all read-only. They describe what they see; they don't change anything.

**Effort: M** (2 weeks). Most of the work is the SSE tool_calls parser + the execute-and-continue loop. The tools themselves are trivial Rust functions.

---

### **Phase 2.4 — Voice: Push-to-Talk** *(talk and listen)*

**What you get:** Click the Mic tool (or press a key) → talk → your words appear in the Ask box → the assistant answers OUT LOUD (voice) AND in text (chat). You hear the answer while it types. Push-to-talk: hold the mic button, release when done.

**Technical:**
- **Speech-to-text:** `webkitSpeechRecognition` (browser Web Speech API, available in WebView2). Zero new deps, works offline for English, calls Windows Speech Recognition / Azure under the hood. Returns text; you send it as a normal chat message.
- **Text-to-speech:** Windows SAPI (`ISpVoice::Speak` via the `windows` crate) in Rust. A new `speak(text: String)` Tauri command. The frontend calls it when the assistant's answer finishes streaming. The voice speaks while the text is already visible in chat.
- **Integration with Phase 2.3:** The `speak` TOOL (callable by the model) is the same as the command. The model can decide to speak its own answers, or you can auto-speak every assistant reply.

**Mic tool behavior:**
1. Click mic → `webkitSpeechRecognition.start()` → "Listening…" indicator in the tray name bar.
2. Speak → words stream into the Ask box live.
3. Release mic (or silence timeout) → `stop()` → auto-send the message.
4. Answer streams back → `speak(assistant_text)` → you hear it.

**No wake word yet.** Alt+Space or click the mic. Wake word comes later (it needs an always-on listener, which is heavier).

**Effort: S-M** (1 week). `webkitSpeechRecognition` is JS-only, no Rust. SAPI is one Rust command wrapping `ISpVoice`. The hard part is the UX polish (canceling mid-speech, showing the live transcript).

---

### **Phase 2.5 — Eyes + Voice Together** *(capture-on-speak)*

**What you get:** You click mic and start talking → the moment you speak, a screenshot is taken and attached to your voice message. You say "what's on screen?" → the model sees the screenshot and answers. Or: you say "click that button" → the model sees the screen, knows where the button is (from the image), and can describe its location. **Still no real clicking yet** — that's Phase 2.6.

**Technical:**
- When `webkitSpeechRecognition` fires its first result (the user's voice is detected), call `captureScreen(null)` immediately. Attach the base64 PNG to the message alongside the transcript.
- The `capture_screen` TOOL (Phase 2.3) also stays — the model can call it anytime during a multi-turn conversation to "look again."
- Optional: a "Watch mode" toggle in the tray: take a screenshot every 5 seconds while the mic is active. Token warning shown in Settings ("continuous watching costs $X/hour"). Off by default.

**Integration:** This is just wiring Phase 2.3 tools + Phase 2.4 voice. The screenshot happens automatically on speak; no new commands needed.

**Effort: S** (3-4 days). Glue code only.

---

### **Phase 2.6 — Actions (Careful, One Step at a Time)**

**What you get:** The assistant can DO things. You say "click that button" → it clicks. You say "type this into the field" → it types. Every real action shows a **yellow confirmation card** with YES/NO buttons in chat. You click YES → the action happens. You click NO or ignore it → nothing happens, it stays safe.

**New tools (ALL require confirmation):**
- `click_at(x, y)` — click the mouse at screen coordinates (physical px)
- `type_text(text)` — type a string into the active window
- `press_keys(keys)` — press a key combo (e.g., "Ctrl+S" to save)
- `launch_app(path)` — open an .exe (e.g., "C:\\Windows\\notepad.exe")
- `open_path(path)` — open a file or URL in the default app

**UI automation (UIA) for "click that button":**
- New Rust command: `find_elements(query)` → searches the active window's UI tree (Windows UI Automation API via the `uiautomation` Rust crate). Returns element name, role, bounding box. The model calls this, sees "Save button at (450, 320)", then calls `click_at(450, 320)`.
- Highlight mode (optional, nice-to-have): when the model identifies an element, draw a yellow box around it for 2 seconds before asking YES/NO.

**Confirmation flow:**
1. Model calls an action tool (e.g., `click_at(450, 320)`).
2. Frontend receives `chat://tool_calls` with `requires_confirm: true`.
3. Shows a card: "🟡 Click at (450, 320)? YES / NO"
4. User clicks YES → frontend emits `confirm-tool://yes` → Rust executes → result appears in chat.
5. User clicks NO → Rust returns `{"error": "user declined"}` → model sees this, says "OK, I won't click."

**Safety:**
- Read-only tools (Phase 2.3) run instantly, no confirm.
- ALL action tools require one YES click per action.
- NO batch approvals in v1 ("click 5 things" = 5 YES clicks).
- Dangerous paths (`C:\Windows\System32`, any .exe in system folders) are blocked with a Rust guard.

**Effort: L** (3 weeks). UIA integration is new. Win32 `SendInput` for typing/clicking is straightforward but needs careful testing. The confirmation UX is non-trivial (embedding YES/NO buttons in a streamed chat).

---

### **Phase 2.7 — Wake Word (Optional, Later)**

**What you get:** Say "Thuki" (or your chosen word) → the assistant activates, same as clicking mic or pressing Alt+Space. No button needed.

**Technical:** Windows Speech Recognition SAPI keyword grammar (`ISpRecoContext`), running in a background Rust thread. Emits a Tauri event when heard. Adds ~30-50ms latency to all audio on the system (always-on mic). English works well; other languages are hit-or-miss with SAPI.

**Recommendation:** Build this AFTER Phase 2.4-2.6 work well. Alt+Space + click-to-talk is already fast. If the owner uses it daily and wants hands-free, add wake word then. It's a nice-to-have, not a must-have.

**Effort: M** (1 week) if built later. **Defer** for now.

---

### **What about Window Switcher (old Phase 2.3)?**

**Answer:** It becomes just the `list_windows` + `focus_window(handle)` tools in Phase 2.3. No separate UI needed. You say "show Firefox" → model calls `focus_window` → Firefox comes to front. The tiny UI (a list you click) can still exist if you want — takes 2 days — but it's optional once the agent can switch windows itself.

---

## Revised phase order (final)

```
NOW:     Phase 2.3 — Agent loop + read-only tools     (2 weeks)
NEXT:    Phase 2.4 — Voice push-to-talk               (1 week)
THEN:    Phase 2.5 — Eyes + Voice (capture-on-speak)  (3-4 days)
THEN:    Phase 2.6 — Actions with confirmation         (3 weeks)
LATER:   Phase 2.7 — Wake word (if wanted)            (defer)
```

**Total to Phase 2.6 (full working assistant): ~7 weeks.**

At the end of Phase 2.5, the owner has: talk → it sees → it answers by voice. That's 90% of the vision. Phase 2.6 adds the ability to act, carefully.

---

## Technical notes (for the implementer)

### 1. Agent loop message flow

**Simplified sequence:**

```
User: "what windows are open?"
  → [role: user, content: "what windows are open?"]
  → POST /chat/completions with tools=[list_windows, capture_screen, ...]
  
Model (stream):
  delta: {tool_calls: [{id: "call_abc", function: {name: "list_windows", arguments: ""}}]}
  delta: {tool_calls: [{index: 0, function: {arguments: "{}"}}]}
  finish_reason: "tool_calls"
  
Rust executes list_windows() → result = "[{name: 'Chrome', ...}, ...]"
  → append to history: [role: "tool", tool_call_id: "call_abc", content: result]
  → POST /chat/completions again (same conversation, now with tool result)
  
Model (stream):
  delta: {content: "You have 5 windows open: Chrome, VS Code, ..."}
  finish_reason: "stop"
```

**SQLite:** Store tool messages as `{"role": "tool", "tool_call_id": "call_abc", "name": "list_windows", "content": "[...]"}`. The frontend can render them as gray cards.

### 2. SSE tool_calls delta parser

OpenAI streams tool calls like this:

```
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_123","type":"function","function":{"name":"list_windows","arguments":""}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]}}]}
data: {"choices":[{"finish_reason":"tool_calls"}]}
```

The `arguments` field is a JSON STRING that arrives in fragments. You must buffer it per `index`, then parse when `finish_reason` arrives. **This is the hardest part of Phase 2.3** — the rest is easy.

Extend `extract_delta` to return an enum: `enum Delta { Content(String), ToolCall(ToolCallDelta), Done(FinishReason) }`.

### 3. Which tools to build first (Phase 2.3)

| Tool | Rust implementation | Danger | Confirm? |
|---|---|---|---|
| `capture_screen(region?)` | Already exists | None | No |
| `list_windows()` | `EnumWindows` Win32 API, filter visible | None | No |
| `get_active_window()` | `GetForegroundWindow` | None | No |
| `read_clipboard()` | `arboard::Clipboard::get_text()` | None (read-only) | No |
| `get_datetime()` | `chrono::Local::now()` | None | No |
| `speak(text)` | `ISpVoice::Speak` | None (just audio) | No |

All six are read-only or benign. Ship them together in Phase 2.3.

### 4. Confirmation hook (Phase 2.6)

When Rust sees a tool call with `requires_confirm: true`:
1. Emit `chat://tool_pending` event with `{tool_call_id, name, args, formatted_description}`.
2. Frontend shows the yellow card with YES/NO.
3. Frontend calls a new command `confirm_tool(tool_call_id, approved: bool)`.
4. Rust has a `HashMap<String, oneshot::Sender<bool>>` keyed by `tool_call_id`. The execute-tool function awaits the channel before running.
5. If YES: execute, return result. If NO: return `{"error": "declined"}`.

### 5. Risks and unknowns

| Risk | Mitigation | Test before Phase 2.3 |
|---|---|---|
| Provider quirk: tool calls + vision images together | The OpenAI spec allows it; anymodel.org should too | Send one test request: messages with an image + tools array → verify it doesn't error |
| Tool call arguments spanning 10+ SSE chunks | Buffer per index, parse on `finish_reason` | Write a unit test with a mock 5KB arguments string split into chunks |
| Streaming stops mid-tool-call (network drop) | `finish_reason` never arrives → timeout after 30s → show error, allow retry | Add a tokio timeout around the SSE loop |
| Model calls the same tool 20 times in a loop | Count tool calls per turn; abort after 10 → "Too many tool calls, please try again" | Guard in the execute loop |
| SAPI voice speaks over itself (user sends 2 messages fast) | `ISpVoice::Speak` blocks; queue speak requests in a Rust channel, drain one at a time | The Windows SAPI docs confirm blocking behavior |

**First test (right now, before starting Phase 2.3):** Send this to anymodel.org and verify it returns `finish_reason: "tool_calls"` with `capture_screen` in the name:

```json
POST https://anymodel.org/v1/chat/completions
{
  "model": "gpt-5.6-sol",
  "messages": [{"role": "user", "content": "take a screenshot"}],
  "tools": [{
    "type": "function",
    "function": {
      "name": "capture_screen",
      "description": "Capture a screenshot",
      "parameters": {"type": "object", "properties": {}}
    }
  }],
  "stream": false
}
```

If this works, everything else will work.

---

## For the owner to approve

**Summary:** Three big phases (2.3, 2.4, 2.5) get you a talking, seeing assistant in ~4 weeks. Phase 2.6 (another 3 weeks) adds safe actions. Total: 7 weeks to the full vision.

**What you approve:**
1. Phase 2.3 (agent loop + 6 read-only tools) — 2 weeks
2. Phase 2.4 (voice in/out) — 1 week  
3. Phase 2.5 (eyes + voice together) — 3 days
4. Phase 2.6 (actions with YES/NO confirm) — 3 weeks

After Phase 2.5 you can talk to it and it sees your screen. After Phase 2.6 it can do things. Say YES to start Phase 2.3, or ask questions first.
