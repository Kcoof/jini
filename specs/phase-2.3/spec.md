# Phase 2.3 — Agent Loop + 6 Read-Only Tools

> **Status:** SPEC — written before building. Owner approved. Orchestrator implements step by step; Claude reviews each gate; owner tests before Phase 2.4 begins.

---

## 1. Goal

The AI inside Thuki's built-in chat can call tools by itself. When the model wants to look at the screen, list open windows, read the clipboard, know the time, or speak aloud, it emits a `tool_calls` finish reason instead of text. Thuki executes the tool in Rust, appends the result as a `role: "tool"` message, and re-POSTs — the model then streams its final answer. The owner sees a gray "tool card" for each call and the answer streams in normally afterward. Six tools ship in this phase; all are read-only or benign. No action tools. No MCP door.

---

## 2. Tool Definitions

All six tools are declared as JSON Schema objects sent in every `send_message` request when the master switch is on (default: **on**).

```jsonc
// tools array sent with every chat request (when tools_enabled = true)
[
  {
    "type": "function",
    "function": {
      "name": "capture_screen",
      "description": "Take a screenshot of the full screen or a region. Returns a base64 PNG.",
      "parameters": {
        "type": "object",
        "properties": {
          "region": {
            "type": "object",
            "description": "Optional crop region in physical pixels.",
            "properties": {
              "x": { "type": "integer" },
              "y": { "type": "integer" },
              "w": { "type": "integer", "minimum": 1 },
              "h": { "type": "integer", "minimum": 1 }
            },
            "required": ["x", "y", "w", "h"]
          }
        },
        "required": []
      }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "list_windows",
      "description": "List all visible, non-minimized top-level windows. Returns title, process name, and PID for each.",
      "parameters": { "type": "object", "properties": {}, "required": [] }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "get_active_window",
      "description": "Return the title and process name of the currently focused window.",
      "parameters": { "type": "object", "properties": {}, "required": [] }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "read_clipboard",
      "description": "Read the current clipboard text. Returns empty string if clipboard holds non-text data.",
      "parameters": { "type": "object", "properties": {}, "required": [] }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "get_datetime",
      "description": "Return the current local date, time, and timezone offset.",
      "parameters": { "type": "object", "properties": {}, "required": [] }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "speak",
      "description": "Speak text aloud using the system voice (Windows SAPI). Non-blocking: queues the speech and returns immediately.",
      "parameters": {
        "type": "object",
        "properties": {
          "text": {
            "type": "string",
            "description": "The text to speak. Maximum 500 characters.",
            "maxLength": 500
          }
        },
        "required": ["text"]
      }
    }
  }
]
```

**When to include tools:** every call through `send_message` / the agent loop when `settings.tools_enabled = true`. `tool_choice` is always `"auto"` — the model decides whether to call tools or answer directly.

**When tools are excluded:** the second (and later) POST within the same agent loop turn uses `tool_choice: "none"` — this forces the model to produce a text answer instead of calling another tool round-trip needlessly after the results are in. (The agent loop still allows up to 10 tool call rounds if the model chains them; only the final forced-answer POST uses `"none"`.)

---

## 3. Cargo.toml additions

```toml
# src-tauri/Cargo.toml — add to [dependencies]
chrono = { version = "0.4", features = ["clock"] }

# windows crate — extend existing features list:
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_Graphics_Gdi",
  "Win32_UI_WindowsAndMessaging",        # already present
  "Win32_System_Threading",              # OpenProcess, GetProcessId — for pid→name
  "Win32_System_ProcessStatus",          # GetModuleBaseNameW
  "Win32_Media_Speech",                  # ISpVoice (SAPI)
  "Win32_System_Com",                    # CoInitializeEx, CoCreateInstance
] }
```

No new crates for SAPI or window enumeration — the `windows` crate already covers all of it.

---

## 4. New Rust modules

```
src-tauri/src/
  tools/
    mod.rs          ← registry: name → (schema, handler fn pointer)
    capture.rs      ← wraps existing capture.rs logic; no duplication
    windows_info.rs ← list_windows + get_active_window (Win32)
    clipboard.rs    ← wraps existing clipboard.rs read_text()
    datetime.rs     ← chrono::Local::now()
    speak.rs        ← SAPI ISpVoice, blocking thread
```

`api/client.rs` gains the `Delta` enum and updated parser. `commands.rs` gains the agent loop logic inside `send_message`. `lib.rs` gains the `tools_enabled` flag in `AppState` (read from settings).

---

## 5. `api/client.rs` — Delta enum + SSE parser rewrite

### 5.1 New types

```rust
// In api/client.rs

/// A single tool-call fragment arriving in one SSE chunk.
/// Arguments arrive as partial JSON strings split across many chunks.
#[derive(Debug, Clone)]
pub struct ToolCallDelta {
    pub index: usize,          // which parallel call (0-based)
    pub id: Option<String>,    // present only in the first chunk for that index
    pub name: Option<String>,  // present only in the first chunk for that index
    pub arguments: String,     // partial JSON fragment; may be empty ""
}

/// What a single SSE data line means after parsing.
#[derive(Debug, Clone)]
pub enum Delta {
    /// Normal text token — forward to the UI.
    Content(String),
    /// One fragment of a tool call (may span many SSE lines).
    ToolCall(ToolCallDelta),
    /// Stream finished. Variant carries the finish_reason string.
    Done(FinishReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FinishReason {
    Stop,       // normal end
    ToolCalls,  // model wants tools executed
    Length,     // max_tokens hit
    Other(String),
}
```

### 5.2 `parse_delta(payload: &str) -> Option<Delta>`

Replaces `extract_delta`. Signature:

```rust
pub fn parse_delta(payload: &str) -> Option<Delta>
```

Logic (exact field paths, in order):

1. Trim. If empty or `"[DONE]"` → return `None`.
2. `serde_json::from_str::<Value>(payload)` → on failure return `None`.
3. Check `choices[0].finish_reason` — if present and not `null`:
   - `"stop"` → return `Some(Delta::Done(FinishReason::Stop))`
   - `"tool_calls"` → return `Some(Delta::Done(FinishReason::ToolCalls))`
   - `"length"` → return `Some(Delta::Done(FinishReason::Length))`
   - anything else → return `Some(Delta::Done(FinishReason::Other(s.to_string())))`
4. Check `choices[0].delta.tool_calls` — if present and is an array:
   - for each element in the array (there is usually exactly one per chunk):
     - `index` = element["index"].as_u64() as usize (default 0)
     - `id` = element["id"].as_str().map(str::to_string) (absent in all but first chunk)
     - `name` = element["function"]["name"].as_str().map(...) (absent in all but first chunk)
     - `arguments` = element["function"]["arguments"].as_str().unwrap_or("").to_string()
   - return the **first** element's delta (the stream emits one tool call index per line)
   - as `Some(Delta::ToolCall(ToolCallDelta { index, id, name, arguments }))`
5. Check `choices[0].delta.content` — non-empty string → `Some(Delta::Content(s))`
6. Check `choices[0].delta.reasoning_content` — non-empty string → `Some(Delta::Content(s))` (same as before, for thinking models)
7. Otherwise → `None`

**Note:** `finish_reason` in step 3 must be checked before `tool_calls` in step 4, because some providers send a chunk that contains both `finish_reason: "tool_calls"` AND the final (possibly empty) `tool_calls` delta on the same line. Returning `Done(ToolCalls)` on that line is correct — the buffer will already hold all argument fragments from prior lines.

### 5.3 `ChatRequest` extended

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}
```

`build_request` signature grows two parameters:

```rust
pub fn build_request(
    model: &str,
    mut messages: Vec<ChatMessage>,
    clipboard_context: Option<&str>,
    image_base64: Option<&str>,
    tools: Option<Vec<serde_json::Value>>,       // NEW
    tool_choice: Option<serde_json::Value>,       // NEW
) -> ChatRequest
```

The new fields are passed through verbatim into `ChatRequest`. All existing call sites pass `None, None` — no behavior change for non-tool requests.

### 5.4 `ChatMessage` — `tool_calls` field and `tool_call_id`

The existing `ChatMessage` carries `content: Value` which is already `serde_json::Value` — this covers every case. Two new optional fields:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,    // unchanged
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,   // NEW: assistant's tool call list
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,            // NEW: role:"tool" result message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,                    // NEW: tool name in role:"tool" messages
}
```

Wire format for the assistant message that requested tools (sent in subsequent POST):
```json
{
  "role": "assistant",
  "content": null,
  "tool_calls": [
    {
      "id": "call_abc123",
      "type": "function",
      "function": { "name": "list_windows", "arguments": "{}" }
    }
  ]
}
```

Wire format for the tool result message:
```json
{
  "role": "tool",
  "tool_call_id": "call_abc123",
  "name": "list_windows",
  "content": "[{\"title\":\"Notepad\",\"process\":\"notepad.exe\",\"pid\":1234}]"
}
```

`content` in a `role: "tool"` message is always a plain `Value::String` (JSON-encoded result string). This is what OpenAI-compatible APIs expect.

---

## 6. `tools/mod.rs` — registry

```rust
// src-tauri/src/tools/mod.rs

pub mod capture;
pub mod clipboard;
pub mod datetime;
pub mod speak;
pub mod windows_info;

use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

pub type ToolResult = Result<String, String>;
pub type BoxFuture<'a> = Pin<Box<dyn Future<Output = ToolResult> + Send + 'a>>;

pub struct ToolDef {
    pub name: &'static str,
    pub schema: fn() -> Value,
}

/// All tools available in Phase 2.3.
pub fn all_schemas() -> Vec<Value> {
    vec![
        capture::schema(),
        windows_info::list_windows_schema(),
        windows_info::get_active_window_schema(),
        clipboard::schema(),
        datetime::schema(),
        speak::schema(),
    ]
}

/// Dispatch a tool call by name. `args` is the fully assembled JSON string
/// of the arguments object (may be "{}" for no-arg tools).
/// Returns the result string that goes into the role:"tool" message content.
pub async fn dispatch(name: &str, args_json: &str) -> ToolResult {
    let args: Value = serde_json::from_str(args_json)
        .unwrap_or(Value::Object(serde_json::Map::new()));
    match name {
        "capture_screen"    => capture::run(&args).await,
        "list_windows"      => windows_info::run_list(&args).await,
        "get_active_window" => windows_info::run_active(&args).await,
        "read_clipboard"    => clipboard::run(&args).await,
        "get_datetime"      => datetime::run(&args).await,
        "speak"             => speak::run(&args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}
```

---

## 7. Tool implementations

### 7.1 `tools/capture.rs`

```rust
pub fn schema() -> serde_json::Value { /* the JSON schema object from §2 */ }

pub async fn run(args: &serde_json::Value) -> super::ToolResult {
    // parse optional region from args["region"]
    let region: Option<crate::commands::Region> = args.get("region")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    // delegate to spawn_blocking → existing capture pipeline
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let full = crate::capture::capture_display()?;
        let img = match region {
            Some(r) if r.w > 0 && r.h > 0 => crate::capture::crop(&full,
                r.x.max(0) as u32, r.y.max(0) as u32, r.w, r.h),
            _ => full,
        };
        let b64 = crate::capture::base64_png(&img)?;
        // Save + clipboard as side-effects (same as the existing command)
        let _ = crate::capture::save_png(&img);
        let _ = crate::capture::copy_to_clipboard(&img);
        // Return as a JSON string: the model receives the base64 PNG inline
        // wrapped in a data URL so vision models can understand it.
        Ok(format!("data:image/png;base64,{b64}"))
    })
    .await
    .map_err(|e| format!("capture task failed: {e}"))??;
    Ok(result)
}
```

**Important:** The tool result is a plain string — it cannot be a multi-part content block in the OpenAI tool-result wire format. The base64 data URL is returned as text. The model sees it as text in the `content` field of the `role:"tool"` message. This is sufficient for the model to describe what it sees (models that support vision also process data URLs embedded in tool result text for some providers). If the provider ignores the image in the tool result, the final answer will be text-only — that is acceptable for Phase 2.3. Vision from tool calls will be verified in the probe test (§15 step 1c).

### 7.2 `tools/windows_info.rs`

```rust
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsWindowVisible, IsIconic,
    GetWindowThreadProcessId, GetForegroundWindow,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;

#[derive(serde::Serialize)]
struct WindowInfo {
    title: String,
    process: String,
    pid: u32,
}

fn enumerate_visible_windows() -> Vec<WindowInfo> {
    // uses EnumWindows with a callback that:
    //   1. skips if !IsWindowVisible(hwnd)
    //   2. skips if IsIconic(hwnd) (minimized)
    //   3. gets title via GetWindowTextW; skips if title is empty
    //   4. gets pid via GetWindowThreadProcessId
    //   5. opens process PROCESS_QUERY_INFORMATION|PROCESS_VM_READ
    //      → GetModuleBaseNameW for process name; falls back to pid.to_string()
    //   6. skips if process name == "thuki-win.exe" (exclude self)
    // Collects into Vec<WindowInfo>
}

pub fn list_windows_schema() -> serde_json::Value { /* schema from §2 */ }
pub fn get_active_window_schema() -> serde_json::Value { /* schema from §2 */ }

pub async fn run_list(_args: &serde_json::Value) -> super::ToolResult {
    let windows = tauri::async_runtime::spawn_blocking(enumerate_visible_windows)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&windows).map_err(|e| e.to_string())
}

pub async fn run_active(_args: &serde_json::Value) -> super::ToolResult {
    let info = tauri::async_runtime::spawn_blocking(|| -> WindowInfo {
        let hwnd = unsafe { GetForegroundWindow() };
        // get title, pid, process name — same helper as enumerate_visible_windows
        // return a single WindowInfo
    })
    .await
    .map_err(|e| e.to_string())?;
    serde_json::to_string(&info).map_err(|e| e.to_string())
}
```

The `EnumWindows` callback stores results in a `Vec<WindowInfo>` via a raw pointer passed through `LPARAM` (standard safe pattern for this Win32 API on Rust — cast `&mut Vec<WindowInfo>` to `*mut c_void` as `LPARAM`, cast back inside the callback). The callback is marked `unsafe extern "system"`.

### 7.3 `tools/clipboard.rs`

```rust
pub fn schema() -> serde_json::Value { /* schema from §2 */ }

pub async fn run(_args: &serde_json::Value) -> super::ToolResult {
    Ok(tauri::async_runtime::spawn_blocking(|| {
        crate::clipboard::read_text().unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())?)
}
```

### 7.4 `tools/datetime.rs`

```rust
pub fn schema() -> serde_json::Value { /* schema from §2 */ }

pub async fn run(_args: &serde_json::Value) -> super::ToolResult {
    use chrono::Local;
    let now = Local::now();
    Ok(serde_json::json!({
        "date":     now.format("%Y-%m-%d").to_string(),
        "time":     now.format("%H:%M:%S").to_string(),
        "weekday":  now.format("%A").to_string(),
        "timezone": now.format("%z").to_string(),
        "iso8601":  now.to_rfc3339(),
    }).to_string())
}
```

### 7.5 `tools/speak.rs`

SAPI must be called from a thread that has a COM apartment initialized. The function queues speech asynchronously (using `SPF_ASYNC`) so it returns immediately from the tool's perspective — the model gets the result without waiting for the voice to finish speaking.

```rust
use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPF_ASYNC};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx,
    CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::core::PCWSTR;

pub fn schema() -> serde_json::Value { /* schema from §2 */ }

pub async fn run(args: &serde_json::Value) -> super::ToolResult {
    let text = args["text"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(500)
        .collect::<String>();
    if text.is_empty() {
        return Err("speak: text must not be empty".into());
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        unsafe {
            // CoInitializeEx for this thread (idempotent if already inited)
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)
                .map_err(|e| format!("SAPI init: {e}"))?;
            // Convert &str → null-terminated UTF-16
            let wide: Vec<u16> = text.encode_utf16()
                .chain(std::iter::once(0u16))
                .collect();
            voice.Speak(PCWSTR(wide.as_ptr()), SPF_ASYNC, None)
                .map_err(|e| format!("SAPI speak: {e}"))
        }
    })
    .await
    .map_err(|e| format!("speak task: {e}"))??;
    Ok("Speaking.".into())
}
```

`SPF_ASYNC` means the `Speak` call returns as soon as the speech is queued. The `ISpVoice` COM object is dropped at the end of the blocking closure — the already-queued speech continues in the SAPI background thread. This is the correct pattern: the `ISpVoice` reference count keeps the underlying object alive until playback finishes.

**SAPI default voice:** uses whatever Windows TTS voice the user has set in Control Panel → Speech. No configuration needed.

---

## 8. Agent loop in `commands.rs`

### 8.1 Tool call buffer (assembled between SSE chunks)

```rust
/// In-progress tool call being assembled from streaming fragments.
struct PendingCall {
    id: String,        // call_xxx — arrives in first chunk only
    name: String,      // function name — arrives in first chunk only
    arguments: String, // accumulates across many chunks
}
```

### 8.2 `stream_chat` return type change

Current: `Result<(String, bool), String>`
New: `Result<StreamResult, String>`

```rust
enum StreamResult {
    /// Model produced text; `text` is the full assembled string, `cancelled` flag.
    Text { text: String, cancelled: bool },
    /// Model requested tool calls. Carry the assembled calls forward.
    ToolCalls(Vec<AssembledCall>),
}

struct AssembledCall {
    id: String,
    name: String,
    arguments: String,  // complete JSON string (all fragments joined)
}
```

### 8.3 Updated `stream_chat` inner loop

The existing SSE loop in `stream_chat` becomes:

```rust
let mut text_buf = String::new();
let mut tool_bufs: std::collections::BTreeMap<usize, PendingCall> = BTreeMap::new();

// ... same tokio::select! structure, same cancel check ...
// Replace extract_delta with:

if let Some(data) = line.strip_prefix("data:") {
    let data = data.trim();
    if data == "[DONE]" {
        // [DONE] without a prior finish_reason: treat as Stop
        return Ok(StreamResult::Text { text: text_buf, cancelled: false });
    }
    match parse_delta(data) {
        Some(Delta::Content(t)) => {
            text_buf.push_str(&t);
            let _ = app.emit("chat://chunk",
                json!({ "conversation_id": conversation_id, "text": t }));
        }
        Some(Delta::ToolCall(tc)) => {
            let entry = tool_bufs.entry(tc.index).or_insert_with(|| PendingCall {
                id: String::new(), name: String::new(), arguments: String::new(),
            });
            if let Some(id) = tc.id { entry.id = id; }
            if let Some(nm) = tc.name { entry.name = nm; }
            entry.arguments.push_str(&tc.arguments);
        }
        Some(Delta::Done(FinishReason::ToolCalls)) => {
            let calls: Vec<AssembledCall> = tool_bufs.into_values()
                .map(|p| AssembledCall { id: p.id, name: p.name, arguments: p.arguments })
                .collect();
            return Ok(StreamResult::ToolCalls(calls));
        }
        Some(Delta::Done(_)) => {
            return Ok(StreamResult::Text { text: text_buf, cancelled: false });
        }
        None => {}
    }
}
```

### 8.4 Agent loop in `send_message`

The existing `tauri::async_runtime::spawn` block is replaced with:

```rust
tauri::async_runtime::spawn(async move {
    let result = agent_loop(
        &http, &url, &api_key, &model,
        initial_chat_messages,
        &app_clone, &conv_id,
        &mut rx,
        tools_json,       // Option<Vec<Value>>: None if tools disabled
        tools_enabled,
    ).await;
    match result {
        Ok(()) => {
            let _ = app_clone.emit("chat://done",
                json!({ "conversation_id": conv_id, "cancelled": false }));
        }
        Err(err) => {
            let _ = app_clone.emit("chat://error",
                json!({ "conversation_id": conv_id, "message": err }));
        }
    }
});
```

### 8.5 `agent_loop` function

```rust
async fn agent_loop(
    http: &reqwest::Client,
    url: &str,
    api_key: &str,
    model: &str,
    mut messages: Vec<ChatMessage>,    // history already built, ready to POST
    app: &AppHandle,
    conversation_id: &str,
    cancel: &mut watch::Receiver<bool>,
    tools: Option<Vec<serde_json::Value>>,
    tools_enabled: bool,
) -> Result<(), String> {
    const MAX_TOOL_ROUNDS: usize = 10;
    const LOOP_TIMEOUT_SECS: u64 = 120;

    let deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(LOOP_TIMEOUT_SECS);

    let mut round = 0usize;

    loop {
        // Overall timeout guard
        if tokio::time::Instant::now() >= deadline {
            return Err("Agent loop timed out after 120 seconds.".into());
        }
        // Cancel check
        if *cancel.borrow() {
            let _ = app.emit("chat://done",
                json!({ "conversation_id": conversation_id, "cancelled": true }));
            return Ok(());
        }

        // Build request for this round.
        // - Round 0: include tools (if enabled), tool_choice: "auto"
        // - Round 1..MAX-1: include tools, tool_choice: "auto"
        // - Final text-only forced answer: include tools: None, tool_choice: "none"
        //   (only if the previous round returned ToolCalls and we've hit MAX)
        let (req_tools, req_tool_choice) = if round >= MAX_TOOL_ROUNDS {
            // Force a text answer; don't allow more tool calls
            (None, Some(serde_json::json!("none")))
        } else if tools_enabled {
            (tools.clone(), Some(serde_json::json!("auto")))
        } else {
            (None, None)
        };

        let request = build_request_full(model, messages.clone(), req_tools, req_tool_choice);

        let stream_result = tokio::time::timeout_at(
            deadline,
            stream_chat(http, url, api_key, &request, app, conversation_id, cancel),
        )
        .await
        .map_err(|_| "Agent loop timed out.".into())??;

        match stream_result {
            StreamResult::Text { text, cancelled } => {
                if cancelled {
                    let _ = app.emit("chat://done",
                        json!({ "conversation_id": conversation_id, "cancelled": true }));
                    return Ok(());
                }
                if !text.is_empty() {
                    persist_assistant(app, conversation_id, &text, None);
                }
                return Ok(()); // normal exit — chat://done emitted by caller
            }

            StreamResult::ToolCalls(calls) => {
                round += 1;

                // 1. Build the assistant message that records the tool_calls request.
                let tool_calls_json: Vec<Value> = calls.iter().map(|c| json!({
                    "id":   c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })).collect();
                let assistant_msg = ChatMessage {
                    role: "assistant".into(),
                    content: Value::Null,
                    tool_calls: Some(Value::Array(tool_calls_json)),
                    tool_call_id: None,
                    name: None,
                };
                messages.push(assistant_msg.clone());
                persist_tool_assistant(app, conversation_id, &assistant_msg);

                // 2. Execute each tool call sequentially.
                //    (Parallel execution is deferred to Phase 2.6 — sequential
                //    is simpler and correct for read-only tools.)
                for call in &calls {
                    // Emit UI event: gray card appears immediately
                    let _ = app.emit("chat://tool-start", json!({
                        "conversation_id": conversation_id,
                        "call_id":   call.id,
                        "tool_name": call.name,
                        "arguments": call.arguments,
                    }));

                    let result_str = crate::tools::dispatch(&call.name, &call.arguments).await
                        .unwrap_or_else(|e| format!("Tool error: {e}"));

                    let tool_msg = ChatMessage {
                        role: "tool".into(),
                        content: Value::String(result_str.clone()),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.name.clone()),
                    };
                    messages.push(tool_msg.clone());
                    persist_tool_result(app, conversation_id, &tool_msg);

                    // Emit UI event: card shows result preview
                    let _ = app.emit("chat://tool-result", json!({
                        "conversation_id": conversation_id,
                        "call_id":   call.id,
                        "tool_name": call.name,
                        "result":    result_str.chars().take(200).collect::<String>(),
                    }));
                }
                // Loop continues: next iteration POSTs the messages with
                // the tool results appended.
            }
        }
    }
}
```

**Max rounds exceeded path:** when `round >= MAX_TOOL_ROUNDS`, the request goes out with `tool_choice: "none"` and `tools: None`. The model is forced to produce a text answer. If the model still returns `ToolCalls` (impossible with `tool_choice: "none"` but defensive), treat it as an error: `return Err("Model refused to stop calling tools after 10 rounds.".into())`.

**Cancellation:** the existing `watch::Receiver<bool>` is checked at the top of every loop iteration and inside `stream_chat` via `tokio::select!`. Cancellation mid-tool-execution (after `dispatch` starts) is not interrupted for Phase 2.3 — the tool runs to completion, then the cancel is detected at the top of the next loop iteration. This is acceptable since all Phase 2.3 tools are fast (< 1 second each).

---

## 9. SQLite — message persistence for tool messages

### 9.1 Storage format

The `messages` JSON array in `conversations.messages` gains two new message shapes:

**Assistant tool-call request message:**
```json
{
  "id": "uuid-v4",
  "role": "assistant",
  "content": null,
  "tool_calls": [
    {
      "id": "call_abc123",
      "type": "function",
      "function": { "name": "list_windows", "arguments": "{}" }
    }
  ],
  "created_at": "1726000000"
}
```

**Tool result message:**
```json
{
  "id": "uuid-v4",
  "role": "tool",
  "tool_call_id": "call_abc123",
  "name": "list_windows",
  "content": "[{\"title\":\"Notepad\",\"process\":\"notepad.exe\",\"pid\":1234}]",
  "created_at": "1726000000"
}
```

No schema migration is required — `messages` is a JSON blob; adding new fields is backward-compatible.

### 9.2 New persistence helpers in `commands.rs`

```rust
/// Persist an assistant message that contains tool_calls (no text content).
fn persist_tool_assistant(app: &AppHandle, conversation_id: &str, msg: &ChatMessage) {
    let state = app.state::<AppState>();
    let Ok(conn) = state.db.lock() else { return };
    let Ok(mut rec) = db::get_conversation(&conn, conversation_id) else { return };
    let mut messages = rec.messages.as_array().cloned().unwrap_or_default();
    messages.push(json!({
        "id":         Uuid::new_v4().to_string(),
        "role":       "assistant",
        "content":    null,
        "tool_calls": msg.tool_calls,
        "created_at": db::stamp(),
    }));
    rec.messages = Value::Array(messages);
    rec.updated_at = db::stamp();
    let _ = db::upsert_conversation(&conn, &rec);
}

/// Persist a role:"tool" result message.
fn persist_tool_result(app: &AppHandle, conversation_id: &str, msg: &ChatMessage) {
    let state = app.state::<AppState>();
    let Ok(conn) = state.db.lock() else { return };
    let Ok(mut rec) = db::get_conversation(&conn, conversation_id) else { return };
    let mut messages = rec.messages.as_array().cloned().unwrap_or_default();
    messages.push(json!({
        "id":          Uuid::new_v4().to_string(),
        "role":        "tool",
        "tool_call_id": msg.tool_call_id,
        "name":        msg.name,
        "content":     msg.content,
        "created_at":  db::stamp(),
    }));
    rec.messages = Value::Array(messages);
    rec.updated_at = db::stamp();
    let _ = db::upsert_conversation(&conn, &rec);
}

/// Extend existing persist_assistant to accept optional tool_calls field.
fn persist_assistant(app: &AppHandle, conversation_id: &str, text: &str,
                     tool_calls: Option<&Value>) {
    // existing logic; tool_calls is always None for text answers
}
```

### 9.3 History rebuild fix — the critical bug

The current `send_message` history-rebuild code is:

```rust
.filter_map(|(idx, m)| {
    let role = m.get("role")?.as_str()?.to_string();
    let text = m.get("content")?.as_str()?.to_string();  // ← BREAKS on null content
    ...
})
```

`m.get("content")?.as_str()?` returns `None` for assistant tool-call messages (content is `null`) and for tool result messages (content is a String, so that one is fine, but role "tool" never gets a `ChatMessage` built at all).

**Fix — replace the entire filter_map block with:**

```rust
let chat_messages: Vec<ChatMessage> = history_messages
    .iter()
    .enumerate()
    .filter_map(|(idx, m)| {
        let role = m.get("role")?.as_str()?.to_string();
        match role.as_str() {
            "user" => {
                let text = m.get("content").and_then(|v| v.as_str())
                    .unwrap_or("").to_string();
                let image = if idx == last_idx {
                    None
                } else {
                    m.get("image_base64")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                };
                let content = match image {
                    Some(b64) => json!([
                        { "type": "text", "text": text },
                        { "type": "image_url", "image_url": {
                            "url": format!("data:image/png;base64,{b64}")
                        }}
                    ]),
                    None => Value::String(text),
                };
                Some(ChatMessage {
                    role, content,
                    tool_calls: None, tool_call_id: None, name: None,
                })
            }
            "assistant" => {
                // May have text content or tool_calls or both.
                let content = m.get("content")
                    .cloned()
                    .unwrap_or(Value::Null);
                let tool_calls = m.get("tool_calls").cloned();
                Some(ChatMessage {
                    role, content, tool_calls,
                    tool_call_id: None, name: None,
                })
            }
            "tool" => {
                // Tool result — must be included so the model sees the results.
                let content = m.get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = m.get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let name = m.get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                Some(ChatMessage {
                    role,
                    content: Value::String(content),
                    tool_calls: None,
                    tool_call_id,
                    name,
                })
            }
            _ => None,  // skip unknown roles
        }
    })
    .collect();
```

This fix is required for the conversation to survive a page reload or history reopen when it contains tool messages.

---

## 10. Settings — master switch

### 10.1 New setting: `tools_enabled`

**Default: `true` (on).** Rationale: the main benefit of this phase is that the AI spontaneously uses tools. Off-by-default would make it invisible. The owner can turn it off if the model keeps calling tools when he doesn't want it to.

**Storage key:** `"tools_enabled"` in the `settings` SQLite table. Value: `"true"` / `"false"`.

**`db::Settings`** gains:
```rust
pub tools_enabled: bool,
```

`load_settings` reads the key; absent → `true`. `save_core_settings` gains a `tools_enabled: bool` parameter and writes it.

**`SaveSettingsInput`** in `commands.rs` gains:
```rust
pub tools_enabled: Option<bool>,  // None → leave unchanged
```

**Frontend `Settings` type** in `types.ts` gains:
```ts
tools_enabled: boolean;
```

### 10.2 UI toggle in Settings panel

In `Settings.tsx`, add a row between the model selector and hotkey input:

```
AI Tools (agent)    [toggle: ON]
When on, the AI can call tools on its own — look at your screen,
list open windows, read the clipboard, and speak.
```

Toggle is a `<button type="button" role="switch" aria-checked={...}>` (same style as existing toggles).

---

## 11. Tauri events (Rust → Frontend)

| Event name | Payload | When emitted |
|---|---|---|
| `chat://chunk` | `{ conversation_id, text }` | unchanged — text tokens |
| `chat://done` | `{ conversation_id, cancelled }` | unchanged — loop complete |
| `chat://error` | `{ conversation_id, message }` | unchanged — fatal error |
| `chat://tool-start` | `{ conversation_id, call_id, tool_name, arguments }` | before each tool executes |
| `chat://tool-result` | `{ conversation_id, call_id, tool_name, result }` | after each tool returns; `result` truncated to 200 chars |

No new commands registered — all new behavior is internal to `send_message`'s agent loop.

---

## 12. Frontend changes

### 12.1 `types.ts`

```ts
// Add to types.ts

export type ToolCall = {
  call_id: string;
  tool_name: string;
  arguments: string;     // raw JSON string as sent by model
  result?: string;       // arrives later via chat://tool-result
  status: "running" | "done" | "error";
};

export type ChatToolStart = {
  conversation_id: string;
  call_id: string;
  tool_name: string;
  arguments: string;
};

export type ChatToolResult = {
  conversation_id: string;
  call_id: string;
  tool_name: string;
  result: string;
};

// Extend ChatMessage role union:
export type ChatMessage = {
  id: string;
  role: "user" | "assistant" | "tool";   // add "tool"
  content: string;
  created_at: string;
  clipboard_context?: string | null;
  image_base64?: string | null;
  tool_calls?: ToolCall[];               // on assistant messages
  tool_call_id?: string | null;          // on tool messages
  name?: string | null;                  // tool name on tool messages
};
```

### 12.2 `useChat.ts` — new listeners

Add to the `attach()` function (alongside the existing `chunk`/`done`/`error` listeners):

```ts
unlisteners.push(
  await listen<ChatToolStart>("chat://tool-start", (event) => {
    if (cancelled) return;
    const { call_id, tool_name, arguments: args } = event.payload;
    setState((prev) => {
      const newCall: ToolCall = {
        call_id, tool_name, arguments: args, status: "running",
      };
      // Find the last assistant message that is the streaming placeholder,
      // or create a new tool-tracking entry.
      // Simpler: keep tool calls in a separate parallel list keyed by call_id.
      return { ...prev, activeCalls: [...(prev.activeCalls ?? []), newCall] };
    });
  }),
);

unlisteners.push(
  await listen<ChatToolResult>("chat://tool-result", (event) => {
    if (cancelled) return;
    const { call_id, tool_name, result } = event.payload;
    setState((prev) => ({
      ...prev,
      activeCalls: (prev.activeCalls ?? []).map((c) =>
        c.call_id === call_id
          ? { ...c, result, status: "done" }
          : c,
      ),
    }));
  }),
);
```

`ChatState` gains:
```ts
activeCalls: ToolCall[];
```

Reset `activeCalls` to `[]` in `chat://done` and in `reset()` and in `send()`.

### 12.3 `ResponsePanel.tsx` — tool cards

Between assistant messages (or where tool activity happened), render a gray card for each active/completed tool call.

**ToolCard component** (inline in `ResponsePanel.tsx` or a separate `ToolCard.tsx`):

```tsx
function ToolCard({ call }: { call: ToolCall }) {
  const [expanded, setExpanded] = useState(false);
  const label = TOOL_LABELS[call.tool_name] ?? call.tool_name;
  return (
    <div className={`tool-card ${call.status}`}>
      <button
        type="button"
        className="tool-card-header"
        onClick={() => setExpanded((e) => !e)}
      >
        <span className="tool-icon">⚙</span>
        <span className="tool-label">
          {call.status === "running" ? `Calling ${label}…` : `Used ${label}`}
        </span>
        <span className="tool-chevron">{expanded ? "▲" : "▾"}</span>
      </button>
      {expanded && (
        <div className="tool-card-body">
          {call.arguments !== "{}" && call.arguments !== "" && (
            <pre className="tool-args">{formatArgs(call.tool_name, call.arguments)}</pre>
          )}
          {call.result && (
            <pre className="tool-result">{call.result}</pre>
          )}
        </div>
      )}
    </div>
  );
}

const TOOL_LABELS: Record<string, string> = {
  capture_screen:    "Screenshot",
  list_windows:      "List windows",
  get_active_window: "Active window",
  read_clipboard:    "Read clipboard",
  get_datetime:      "Date & time",
  speak:             "Speak",
};

function formatArgs(toolName: string, argsJson: string): string {
  // For capture_screen with a region: show "Region x,y w×h"
  // For speak: show the text value
  // For others: just pretty-print the JSON
  try {
    const obj = JSON.parse(argsJson);
    if (toolName === "speak" && obj.text) return `"${obj.text}"`;
    return JSON.stringify(obj, null, 2);
  } catch {
    return argsJson;
  }
}
```

**Rendering in `ResponsePanel.tsx`:** render `activeCalls` as a block above the current streaming text (or after the last completed assistant message). Collapsed by default (`expanded = false`).

**CSS (add to `index.css`):**

```css
.tool-card {
  margin: 4px 0;
  border-radius: 6px;
  background: #1e1e2e;
  border: 1px solid #2e2e3e;
  font-size: 12px;
}
.tool-card.running { border-color: #4a4a6a; }
.tool-card.done    { border-color: #2e2e3e; opacity: 0.85; }
.tool-card-header  {
  width: 100%; display: flex; align-items: center; gap: 6px;
  padding: 6px 8px; background: none; border: none;
  cursor: pointer; color: #a0a0c0; text-align: left;
}
.tool-icon  { font-size: 11px; opacity: 0.6; }
.tool-label { flex: 1; }
.tool-chevron { font-size: 9px; opacity: 0.5; }
.tool-card-body { padding: 0 8px 8px; color: #7070a0; }
.tool-args, .tool-result {
  margin: 4px 0 0; padding: 4px 6px;
  background: #12121e; border-radius: 4px;
  white-space: pre-wrap; word-break: break-all;
  font-size: 11px; max-height: 120px; overflow-y: auto;
}
```

---

## 13. `lib.rs` and `AppState` changes

```rust
pub struct AppState {
    pub db: Mutex<Connection>,
    pub cancel: Mutex<Option<watch::Sender<bool>>>,
    pub hotkey: Mutex<String>,
    pub http: reqwest::Client,
    pub tools_enabled: Mutex<bool>,   // NEW — cache; re-read from settings on each send
}
```

In `setup`: read `db::get_value_public(&conn, "tools_enabled")` → parse as bool (absent/unknown → `true`) → store in `AppState`.

`tools_enabled` in `AppState` is only for caching; `send_message` always reads fresh from `db::load_settings` to avoid stale state after the user changes the toggle.

`lib.rs` module list gains:
```rust
pub mod tools;
```

---

## 14. Unit tests

### 14.1 `api/client.rs` — `parse_delta` tests

```rust
#[cfg(test)]
mod parse_delta_tests {
    use super::*;

    // Helper
    fn tool_call_chunk(index: usize, id: Option<&str>, name: Option<&str>, args: &str) -> String {
        let mut tc = serde_json::json!({
            "index": index,
            "type": "function",
            "function": { "arguments": args }
        });
        if let Some(i) = id { tc["id"] = json!(i); }
        if let Some(n) = name { tc["function"]["name"] = json!(n); }
        json!({ "choices": [{ "delta": { "tool_calls": [tc] } }] }).to_string()
    }

    #[test]
    fn content_delta() {
        let d = parse_delta(r#"{"choices":[{"delta":{"content":"hello"}}]}"#);
        assert!(matches!(d, Some(Delta::Content(t)) if t == "hello"));
    }

    #[test]
    fn reasoning_content_falls_back() {
        let d = parse_delta(r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#);
        assert!(matches!(d, Some(Delta::Content(t)) if t == "think"));
    }

    #[test]
    fn done_stop() {
        let d = parse_delta(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        assert!(matches!(d, Some(Delta::Done(FinishReason::Stop))));
    }

    #[test]
    fn done_tool_calls() {
        let d = parse_delta(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        assert!(matches!(d, Some(Delta::Done(FinishReason::ToolCalls))));
    }

    #[test]
    fn done_with_simultaneous_tool_call_fragment() {
        // Some providers send finish_reason and tool_calls on the same line.
        // finish_reason must win (checked first).
        let d = parse_delta(r#"{
            "choices":[{
                "delta":{"tool_calls":[{"index":0,"function":{"arguments":""}}]},
                "finish_reason":"tool_calls"
            }]
        }"#);
        assert!(matches!(d, Some(Delta::Done(FinishReason::ToolCalls))));
    }

    #[test]
    fn empty_and_done_sentinel() {
        assert!(parse_delta("").is_none());
        assert!(parse_delta("[DONE]").is_none());
        assert!(parse_delta(r#"{"choices":[{"delta":{}}]}"#).is_none());
    }

    #[test]
    fn tool_call_first_chunk_has_id_and_name() {
        let raw = tool_call_chunk(0, Some("call_abc"), Some("list_windows"), "");
        let d = parse_delta(&raw);
        let Some(Delta::ToolCall(tc)) = d else { panic!("expected ToolCall") };
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id.as_deref(), Some("call_abc"));
        assert_eq!(tc.name.as_deref(), Some("list_windows"));
        assert_eq!(tc.arguments, "");
    }

    #[test]
    fn tool_call_subsequent_chunks_no_id_name() {
        let raw = tool_call_chunk(0, None, None, r#"{"re"#);
        let d = parse_delta(&raw);
        let Some(Delta::ToolCall(tc)) = d else { panic!("expected ToolCall") };
        assert!(tc.id.is_none());
        assert!(tc.name.is_none());
        assert_eq!(tc.arguments, r#"{"re"#);
    }

    #[test]
    fn chunked_5kb_arguments_reassemble() {
        // Simulate a 5KB arguments string delivered in 20 chunks of ~256 bytes.
        // This validates that the agent loop's BTreeMap buffer produces the right output.
        let full_args = format!(r#"{{"query":"{}"}}"#, "x".repeat(5000));
        let chunk_size = 256;
        let mut assembled = String::new();
        // First chunk: has id + name
        let first = tool_call_chunk(0, Some("call_big"), Some("some_tool"),
                                    &full_args[..chunk_size]);
        match parse_delta(&first) {
            Some(Delta::ToolCall(tc)) => {
                assert_eq!(tc.id.as_deref(), Some("call_big"));
                assembled.push_str(&tc.arguments);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        // Middle chunks: no id/name
        let mut offset = chunk_size;
        while offset < full_args.len() {
            let end = (offset + chunk_size).min(full_args.len());
            let chunk = tool_call_chunk(0, None, None, &full_args[offset..end]);
            match parse_delta(&chunk) {
                Some(Delta::ToolCall(tc)) => assembled.push_str(&tc.arguments),
                other => panic!("expected ToolCall at offset {offset}: {other:?}"),
            }
            offset = end;
        }
        assert_eq!(assembled, full_args);
    }

    #[test]
    fn tool_calls_and_content_in_same_stream() {
        // Content delta before tool_calls finish_reason — both must be handled.
        let content_line = r#"{"choices":[{"delta":{"content":"Let me check "}}]}"#;
        let tc_line = tool_call_chunk(0, Some("call_1"), Some("get_datetime"), "{}");
        let done_line = r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#;

        assert!(matches!(parse_delta(content_line), Some(Delta::Content(_))));
        assert!(matches!(parse_delta(&tc_line), Some(Delta::ToolCall(_))));
        assert!(matches!(parse_delta(done_line), Some(Delta::Done(FinishReason::ToolCalls))));
    }

    #[test]
    fn no_tools_stream_unchanged_behavior() {
        // A plain text stream without tool_calls produces only Content + Done(Stop).
        let lines = [
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{"content":"!"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ];
        let deltas: Vec<_> = lines.iter().filter_map(|l| parse_delta(l)).collect();
        assert!(matches!(&deltas[0], Delta::Content(t) if t == "Hi"));
        assert!(matches!(&deltas[1], Delta::Content(t) if t == "!"));
        assert!(matches!(&deltas[2], Delta::Done(FinishReason::Stop)));
    }

    #[test]
    fn parallel_tool_calls_two_indices() {
        // Two tool calls at indices 0 and 1 — each must go to its own buffer slot.
        let c0 = tool_call_chunk(0, Some("call_0"), Some("get_datetime"), "{}");
        let c1 = tool_call_chunk(1, Some("call_1"), Some("list_windows"), "{}");
        let d0 = parse_delta(&c0);
        let d1 = parse_delta(&c1);
        let Some(Delta::ToolCall(tc0)) = d0 else { panic!() };
        let Some(Delta::ToolCall(tc1)) = d1 else { panic!() };
        assert_eq!(tc0.index, 0);
        assert_eq!(tc1.index, 1);
        assert_ne!(tc0.id, tc1.id);
    }
}
```

### 14.2 `tools/datetime.rs` test

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn datetime_returns_valid_json() {
        let result = super::run(&serde_json::Value::Null).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["date"].as_str().unwrap().len() == 10); // YYYY-MM-DD
        assert!(v["iso8601"].as_str().unwrap().contains('T'));
    }
}
```

### 14.3 `tools/windows_info.rs` test

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn list_windows_returns_at_least_one() {
        // Any live Windows machine has at least one visible window.
        let result = super::run_list(&serde_json::Value::Null).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v.as_array().unwrap().len() >= 1);
    }

    #[tokio::test]
    async fn active_window_has_fields() {
        let result = super::run_active(&serde_json::Value::Null).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["title"].is_string());
        assert!(v["pid"].is_number());
    }
}
```

### 14.4 `tools/capture.rs` test

```rust
#[tokio::test]
async fn capture_returns_data_url() {
    let result = super::run(&serde_json::Value::Null).await.unwrap();
    assert!(result.starts_with("data:image/png;base64,"));
    assert!(result.len() > 100);
}
```

### 14.5 History round-trip test (db.rs)

```rust
#[test]
fn tool_messages_survive_roundtrip() {
    let conn = memory();
    let messages = serde_json::json!([
        { "id": "1", "role": "user", "content": "what windows are open?", "created_at": "1" },
        { "id": "2", "role": "assistant", "content": null,
          "tool_calls": [{"id":"call_x","type":"function",
              "function":{"name":"list_windows","arguments":"{}"}}],
          "created_at": "2" },
        { "id": "3", "role": "tool", "tool_call_id": "call_x",
          "name": "list_windows", "content": "[{\"title\":\"Notepad\"}]", "created_at": "3" },
        { "id": "4", "role": "assistant", "content": "You have Notepad open.", "created_at": "4" }
    ]);
    let rec = ConversationRecord {
        id: "c1".into(), title: "t".into(),
        messages: messages.clone(),
        created_at: "1".into(), updated_at: "4".into(),
    };
    upsert_conversation(&conn, &rec).unwrap();
    let got = get_conversation(&conn, "c1").unwrap();
    let arr = got.messages.as_array().unwrap();
    assert_eq!(arr.len(), 4);
    assert_eq!(arr[1]["role"], "assistant");
    assert!(arr[1]["tool_calls"].is_array());
    assert_eq!(arr[2]["role"], "tool");
    assert_eq!(arr[2]["tool_call_id"], "call_x");
    assert_eq!(arr[3]["content"], "You have Notepad open.");
}
```

---

## 15. Build steps (ordered, each independently gate-able)

Each step ends with: `npx tsc --noEmit` + `npm run build` + `cargo test`. Do not proceed to the next step if any gate fails.

### Step 0 — Streaming tool_calls probe (before any code change)

Send a request to the configured provider with tools included, `stream: true`, using the existing HTTP client — just log raw SSE lines to stderr. Confirm that `finish_reason: "tool_calls"` appears in the stream for a prompt like "What time is it?" (models reliably call `get_datetime` when offered it).

This is a one-off Rust integration test, not a unit test. Write it in `src-tauri/src/api/probe.rs` behind `#[cfg(test)]` + `#[ignore]` — run it manually with `cargo test probe -- --ignored --nocapture`. It is deleted or left ignored after the probe passes; it is not part of the CI gate.

```rust
// Probe test shape:
#[ignore]
#[tokio::test]
async fn probe_streaming_tool_calls() {
    // read key from env: THUKI_API_KEY, base_url: THUKI_BASE_URL, model: THUKI_MODEL
    // build a ChatRequest with tools=[get_datetime schema], stream:true
    // POST, print every SSE line to stderr
    // assert that at least one line contains "tool_calls"
}
```

### Step 1 — `api/client.rs`: Delta enum + `parse_delta`

- Add `ToolCallDelta`, `AssembledCall`, `FinishReason`, `Delta`, `StreamResult` types.
- Add `parse_delta` function.
- Keep `extract_delta` as an alias: `pub fn extract_delta(p: &str) -> Option<String>` that calls `parse_delta` and returns `Some(text)` only for `Delta::Content`. This preserves all existing call sites including `run_provider_test`.
- Add `tool_calls` and `tool_choice` to `ChatRequest`; update `build_request` signature (add two `None` params at all call sites).
- Add `tool_calls`, `tool_call_id`, `name` to `ChatMessage`.
- Add all `parse_delta` unit tests from §14.1.
- **Gate:** `cargo test` — all new tests pass, all existing tests pass.

### Step 2 — `tools/` module: datetime + clipboard + windows_info + speak + capture

- Create `src-tauri/src/tools/mod.rs` with `all_schemas()` and `dispatch()`.
- Create `tools/datetime.rs`, `tools/clipboard.rs`, `tools/capture.rs`, `tools/windows_info.rs`, `tools/speak.rs`.
- Add `pub mod tools;` to `lib.rs`.
- Add `chrono` to `Cargo.toml`; extend `windows` feature list (§3).
- Add tool unit tests from §14.2–14.4.
- **Gate:** `cargo test` — datetime, clipboard, windows, capture tests pass.

### Step 3 — Settings: `tools_enabled` toggle

- `db.rs`: add `tools_enabled` to `Settings`; update `load_settings` and `save_core_settings`.
- `commands.rs`: add `tools_enabled` to `SaveSettingsInput`; read it in `send_message`.
- `AppState` in `lib.rs`: add `tools_enabled: Mutex<bool>`.
- `types.ts`: add `tools_enabled: boolean` to `Settings`.
- `Settings.tsx`: add the toggle row (§10.2).
- **Gate:** `tsc --noEmit` + `npm run build` + `cargo test`.

### Step 4 — `stream_chat` returns `StreamResult`; agent loop

- Update `stream_chat` to use `parse_delta`, build `tool_bufs`, return `StreamResult`.
- Implement `agent_loop` in `commands.rs` (§8.5).
- Replace the spawn block in `send_message` with the agent loop call.
- Add `persist_tool_assistant` and `persist_tool_result` helpers.
- Add `tools_json` construction from `tools::all_schemas()` in `send_message`.
- **Gate:** `cargo test` — no regressions; `cargo build` clean.

### Step 5 — History rebuild fix

- Replace the `filter_map` block in `send_message` with the new match-based version (§9.3).
- Add the `tool_messages_survive_roundtrip` DB test (§14.5).
- **Gate:** `cargo test`.

### Step 6 — Tauri events + frontend types

- `types.ts`: add `ToolCall`, `ChatToolStart`, `ChatToolResult`; extend `ChatMessage`.
- `useChat.ts`: add `activeCalls` state; add `chat://tool-start` and `chat://tool-result` listeners; reset `activeCalls` in `done`/`reset`/`send`.
- **Gate:** `tsc --noEmit` + `npm run build`.

### Step 7 — ResponsePanel: ToolCard

- Add `ToolCard` component and `TOOL_LABELS` map.
- Add CSS (§12.3) to `index.css`.
- Render `activeCalls` in `ResponsePanel`.
- **Gate:** `tsc --noEmit` + `npm run build`.

### Step 8 — End-to-end smoke test (manual)

Owner runs the app, asks "what time is it?" — verifies the agent loop, tool card, and answer. Then acceptance tests below.

---

## 16. Acceptance tests (objective — each is a pass/fail statement)

### AT-1: list_windows round-trip
1. Type: "What windows do I have open right now?"
2. **Pass:** SQLite `messages` array for that conversation contains (in order):
   - `role: "user"` with the question
   - `role: "assistant"` with `tool_calls` containing `list_windows`
   - `role: "tool"` with `tool_call_id` matching the call id and `content` being a valid JSON array of window objects
   - `role: "assistant"` with `content` being a human-readable list of windows
3. UI shows a gray "Used List windows" card collapsed by default; expanding it reveals the window list preview.

### AT-2: get_datetime
1. Type: "What is today's date and time?"
2. **Pass:** Final assistant answer contains today's correct date. Tool card shows "Used Date & time".

### AT-3: read_clipboard
1. Copy some text to clipboard. Ask: "What's in my clipboard?"
2. **Pass:** The answer matches the clipboard text. No hallucination.

### AT-4: capture_screen (model-initiated)
1. Open Notepad with some text visible. Ask: "What is on my screen right now?"
2. **Pass:** The answer describes Notepad and the visible text. Tool card shows "Used Screenshot".

### AT-5: speak
1. Ask: "Say hello to me out loud."
2. **Pass:** Windows speaks the word "hello" (or equivalent). Tool card shows "Used Speak". The response streams immediately without waiting for speech to finish.

### AT-6: multi-tool chain
1. Ask: "What windows are open and what time is it?"
2. **Pass:** Two tool cards appear (list_windows + get_datetime). Final answer addresses both questions. SQLite has 2 tool_call entries under the assistant message.

### AT-7: no-tool response (tools enabled, model chooses not to call)
1. Ask: "What is 2+2?"
2. **Pass:** No tool card appears. Answer is "4". `stream_chat` returned `StreamResult::Text` on the first call.

### AT-8: cancellation mid-loop
1. Ask a question that will trigger a tool call (e.g., "list my windows and describe each one").
2. Click Cancel immediately after the tool card appears (while the second POST is streaming).
3. **Pass:** `chat://done` with `cancelled: true` is emitted. Streaming stops. No crash. DB state is consistent (may have partial tool messages — that is acceptable).

### AT-9: max tool rounds guard
(Requires temporarily patching `MAX_TOOL_ROUNDS = 2` for the test.)
1. Use a provider/model known to aggressively call tools. Ask a question.
2. **Pass:** After 2 tool rounds, the third POST uses `tool_choice: "none"` and the model produces a text answer. `chat://error` is NOT emitted. The cap is transparent to the user.

### AT-10: history reopen round-trip
1. Complete AT-1 (list_windows conversation).
2. Close the chat (`Back`), go to History, reopen the same conversation.
3. **Pass:** All messages display correctly — user question, tool cards (rendered from the stored `tool_calls` field), final assistant answer. No blank message. No crash.
4. Type a follow-up: "Which of those windows is for a browser?"
5. **Pass:** The follow-up sends the full history including the tool messages; the model answers correctly.

### AT-11: tools_enabled = false
1. Go to Settings → AI Tools → toggle **Off**.
2. Ask: "What time is it?"
3. **Pass:** No tool card. The model answers from its training knowledge (approximate time or "I don't have live access"). The request had `tools: null` and `tool_choice: null`.

### AT-12: `cargo test` clean
Run `cargo test` with no live network. All unit tests pass. No test requires an API key.

### AT-13: `tsc --noEmit` + `npm run build` clean
TypeScript compiles with zero errors. Vite build produces no warnings about missing types.

---

## 17. Out of scope for Phase 2.3

- Action tools: `click_at`, `type_text`, `press_keys`, `launch_app`, `open_path` — Phase 2.6
- YES/NO confirm cards — Phase 2.6
- MCP server (Door 2) — Phase 2.3b/c
- Voice input / push-to-talk — Phase 2.4
- Eyes + voice capture-on-speak — Phase 2.5
- Wake word — Phase 2.7
- Parallel tool call execution — acceptable in Phase 2.6 alongside action tools
- Streaming vision from tool result (the `capture_screen` result is text/data URL, not a true vision message part) — may improve in Phase 2.3b when MCP image content blocks are defined
- System prompt injection ("you have the following tools…") — the provider uses the `tools` array; no system prompt change needed
- Tool call retry on malformed arguments — out of scope; log and return error string

---

*End of Phase 2.3 spec. Next step: orchestrator implements Step 0 (probe) → Step 1 (Delta enum) per the gates above.*The spec is complete above. Note that I'm in plan/read-only mode, so I can't save it to disk — you'll need to save the text above as `specs/phase-2.3/spec.md` yourself (or have the orchestrator do it as Step 0 of the build).

