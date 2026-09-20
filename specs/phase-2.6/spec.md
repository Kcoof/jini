# Phase 2.6 — Action Tools with YES/NO Confirmation

**Status:** SPEC  
**Estimated effort:** ~3 weeks (4 independently shippable sub-steps)  
**Depends on:** Phase 2.5 (VERDICT OK)  
**Out of scope:** MCP door actions (Phase 2.6b), rung-D sequences/batches, drag/scroll gestures, wake word, watch mode, set_clipboard write action (read_clipboard already covers the read case; write adds undo complexity — deferred)

---

## Architecture Overview

Six action tools, each requiring an explicit per-action YES/NO confirmation from the user before execution. The confirmation flow:

```
agent_loop calls action tool
  → Rust emits chat://tool-pending
  → Frontend shows YELLOW confirmation card (YES / NO)
  → User clicks YES or NO (or waits → auto-NO after 60s)
  → Frontend calls confirm_tool(call_id, approved)
  → Rust oneshot::Sender<bool> in pending_confirmations map
  → dispatch_action resumes: executes (YES) or returns error (NO/timeout)
  → normal tool-result path continues
```

**MCP door isolation (future Phase 2.6b):** Action tools are registered ONLY in `all_action_schemas()`, NOT in `all_schemas()` (which MCP uses). The MCP server in `src/bin/mcp.rs` calls `tools::all_schemas()` and remains read-only. A 2.6b step will add an opt-in gate when the owner decides to expose actions to Claude Desktop.

---

## Sub-Step Plan

| Sub-step | Scope | Gate |
|----------|-------|------|
| **A** | Confirmation infra + `open_path` (safest action, proves the flow) | AT-A1 through AT-A4 |
| **B** | Input synthesis trio: `click_at`, `type_text`, `press_keys` | AT-B1 through AT-B6 |
| **C** | UIA element finder: `find_elements` | AT-C1 through AT-C3 |
| **D** | `launch_app` + `actions_enabled` setting + Settings UI + MCP gating note | AT-D1 through AT-D4 |

Each sub-step is independently shippable and reviewable. Do not start the next until the current is VERDICT OK.

---

## 1. Confirmation Infrastructure

### 1a. AppState additions (`src-tauri/src/lib.rs`)

```rust
use tokio::sync::oneshot;
use std::collections::HashMap;

pub struct AppState {
    // ... existing fields ...
    /// Pending action confirmations: call_id → oneshot sender.
    /// Agent loop suspends the action dispatch until the user answers.
    pub pending_confirmations: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// Master switch: actions enabled (default false; user opts in).
    pub actions_enabled: Mutex<bool>,
}
```

Initialise in `run()`:
```rust
app.manage(AppState {
    // ... existing ...
    pending_confirmations: Mutex::new(HashMap::new()),
    actions_enabled: Mutex::new(actions_on),  // loaded from db key "actions_enabled"
});
```

Load from db in `run()`:
```rust
let actions_on = db::get_value_public(&conn, "actions_enabled")?
    .map(|v| v.eq_ignore_ascii_case("true"))
    .unwrap_or(false);  // DEFAULT OFF — safety-first
```

**Default OFF rationale:** Action tools execute OS-level effects. Requiring the user to explicitly enable them in Settings protects against accidental enabling and makes the capability opt-in. Read-only tools (Phase 2.3) default ON; action tools default OFF.

### 1b. `confirm_tool` command (`src-tauri/src/commands.rs`)

```rust
#[tauri::command]
pub fn confirm_tool(
    state: State<AppState>,
    call_id: String,
    approved: bool,
) -> Result<(), String> {
    let mut map = state.pending_confirmations.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = map.remove(&call_id) {
        let _ = tx.send(approved);
        Ok(())
    } else {
        Err(format!("No pending confirmation for call_id: {call_id}"))
    }
}
```

Register in `lib.rs` invoke_handler: `commands::confirm_tool`.

### 1c. `dispatch_action_with_confirm` in `src-tauri/src/tools/actions/mod.rs`

The agent loop calls this for action tools instead of `tools::dispatch`:

```rust
/// Dispatch an action tool with a confirmation gate.
/// Emits chat://tool-pending, suspends until YES/NO, then executes or declines.
/// Timeout: 60 seconds → auto-decline.
pub async fn dispatch_with_confirm(
    name: &str,
    args_json: &str,
    call_id: &str,
    app: &AppHandle,
    state: &AppState,
) -> ToolResult {
    // Build human-readable description from name + args.
    let args: Value = serde_json::from_str(args_json).unwrap_or(Value::Object(Default::default()));
    let description = describe_action(name, &args);

    // Register the oneshot channel before emitting (avoid race).
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    {
        let mut map = state.pending_confirmations.lock().map_err(|e| e.to_string())?;
        map.insert(call_id.to_string(), tx);
    }

    // Emit pending event → frontend shows yellow YES/NO card.
    let _ = app.emit("chat://tool-pending", serde_json::json!({
        "call_id":     call_id,
        "tool_name":   name,
        "arguments":   args_json,
        "description": description,
    }));

    // Await user decision, with 60s auto-decline timeout.
    let approved = tokio::time::timeout(
        tokio::time::Duration::from_secs(60),
        rx,
    )
    .await
    .unwrap_or(Ok(false))   // timeout → false
    .unwrap_or(false);       // sender dropped → false

    // Clean up the map entry (may already be removed by confirm_tool).
    {
        let mut map = state.pending_confirmations.lock().map_err(|e| e.to_string())?;
        map.remove(call_id);
    }

    if !approved {
        return Err(format!("Action declined by user: {name}"));
    }

    // Execute the action.
    dispatch(name, args_json).await
}
```

### 1d. Agent loop integration (`commands.rs` `agent_loop`)

The agent loop currently calls `crate::tools::dispatch(&call.name, &call.arguments)`. Change to:

```rust
let result_str = if crate::tools::actions::is_action(&call.name) {
    let state = app.state::<AppState>();
    crate::tools::actions::dispatch_with_confirm(
        &call.name,
        &call.arguments,
        &call.id,
        app,
        &state,
    )
    .await
    .unwrap_or_else(|e| format!("Action error: {e}"))
} else {
    crate::tools::dispatch(&call.name, &call.arguments)
        .await
        .unwrap_or_else(|e| format!("Tool error: {e}"))
};
```

Also, in `send_message`, pass `actions_enabled` to `agent_loop` (same pattern as `tools_enabled`). Only include action schemas when `actions_enabled && tools_enabled`:

```rust
let actions_on = settings.actions_enabled;  // loaded from db

let tools_json = if settings.tools_enabled {
    let mut schemas = crate::tools::all_schemas();
    if actions_on {
        schemas.extend(crate::tools::actions::all_action_schemas());
    }
    Some(schemas)
} else {
    None
};
```

### 1e. Lock discipline

- `pending_confirmations` mutex: held only to insert/remove. Never held while awaiting (the await is on the oneshot receiver, not under the mutex). No deadlock risk.
- Each lock acquisition is a short critical section. No nested locks with `db` or `cancel`.
- If `cancel_message` fires while a confirmation is pending: the agent loop's cancel check at top-of-loop will trip on the NEXT iteration. The current `dispatch_with_confirm` await will still hold. To handle cancel cleanly: add a `tokio::select!` in `dispatch_with_confirm` between the 60s timeout and the cancel receiver. See §1f.

### 1f. Cancel integration

```rust
// In dispatch_with_confirm, replace the simple timeout with a select:
let approved = tokio::select! {
    result = tokio::time::timeout(Duration::from_secs(60), rx) => {
        result.unwrap_or(Ok(false)).unwrap_or(false)
    }
    _ = cancel.changed() => {
        false  // stream cancelled → auto-decline
    }
};
```

`cancel: &mut watch::Receiver<bool>` is already threaded through `agent_loop`. Pass it down to `dispatch_with_confirm`.

Updated signature:
```rust
pub async fn dispatch_with_confirm(
    name: &str,
    args_json: &str,
    call_id: &str,
    app: &AppHandle,
    state: &AppState,
    cancel: &mut watch::Receiver<bool>,
) -> ToolResult
```

---

## 2. Confirmation Card UI

### 2a. TypeScript types (add to `src/lib/types.ts`)

```typescript
export type ChatToolPending = {
  conversation_id: string;
  call_id: string;
  tool_name: string;
  arguments: string;
  description: string;
};

// Extend ToolCall status:
export type ToolCall = {
  call_id: string;
  tool_name: string;
  arguments: string;
  result?: string;
  status: "pending" | "running" | "done" | "error" | "declined";
  description?: string;
};
```

### 2b. `useChat.ts` additions

Add listener for `chat://tool-pending`:
```typescript
unlisteners.push(
  await listen<ChatToolPending>("chat://tool-pending", (event) => {
    if (cancelled) return;
    const { call_id, tool_name, arguments: args, description } = event.payload;
    setState((prev) => ({
      ...prev,
      activeCalls: [
        ...prev.activeCalls,
        { call_id, tool_name, arguments: args, description, status: "pending" },
      ],
    }));
  }),
);
```

Update `chat://tool-start` handler: when a tool-start event fires for a previously-pending call, update status to `"running"`:
```typescript
// In chat://tool-start handler:
setState((prev) => ({
  ...prev,
  activeCalls: prev.activeCalls.map((c) =>
    c.call_id === call_id
      ? { ...c, status: "running" as const }
      : c,
  ).concat(
    prev.activeCalls.some(c => c.call_id === call_id)
      ? []
      : [{ call_id, tool_name, arguments: args, status: "running" }]
  ),
}));
```

Update `chat://tool-result` to handle `"declined"` status (result string will contain "Action declined by user"):
```typescript
// In chat://tool-result handler:
setState((prev) => ({
  ...prev,
  activeCalls: prev.activeCalls.map((c) =>
    c.call_id === call_id
      ? { ...c, result, status: result.startsWith("Action declined") ? "declined" : "done" }
      : c,
  ),
}));
```

Add `confirm_tool` API call to `src/lib/api.ts`:
```typescript
export function confirmTool(callId: string, approved: boolean): Promise<void> {
  return invoke("confirm_tool", { callId, approved });
}
```

### 2c. `ConfirmCard` component (`src/components/ConfirmCard.tsx`)

```tsx
// Pending action confirmation card (specs/phase-2.6 §2c).
// Yellow background, YES/NO buttons, disabled after answer.

import { useState } from "react";
import { confirmTool } from "../lib/api";
import type { ToolCall } from "../lib/types";

const ACTION_LABELS: Record<string, string> = {
  click_at:      "Click",
  type_text:     "Type text",
  press_keys:    "Press keys",
  launch_app:    "Launch app",
  open_path:     "Open",
  find_elements: "Find elements",
};

export function ConfirmCard({ call }: { call: ToolCall }) {
  const [answered, setAnswered] = useState(false);
  const label = ACTION_LABELS[call.tool_name] ?? call.tool_name;

  async function answer(approved: boolean) {
    if (answered) return;
    setAnswered(true);
    await confirmTool(call.call_id, approved);
  }

  // After the call completes (running/done/declined), show read-only state.
  if (call.status !== "pending") {
    return (
      <div
        className={`confirm-card resolved ${call.status}`}
        role="status"
        aria-label={`${label} — ${call.status}`}
      >
        <span className="confirm-icon">⚡</span>
        <span className="confirm-label">
          {call.status === "declined" ? `Declined: ${label}` : `Approved: ${label}`}
        </span>
        <span className="confirm-desc">{call.description}</span>
      </div>
    );
  }

  return (
    <div
      className="confirm-card pending"
      role="dialog"
      aria-label={`Confirm action: ${call.description}`}
    >
      <span className="confirm-icon">⚡</span>
      <div className="confirm-body">
        <span className="confirm-label">{label}</span>
        <span className="confirm-desc">{call.description}</span>
      </div>
      <div className="confirm-actions" role="group" aria-label="Approve or decline">
        <button
          type="button"
          className="confirm-yes"
          disabled={answered}
          onClick={() => void answer(true)}
          aria-label="Approve this action"
        >
          YES
        </button>
        <button
          type="button"
          className="confirm-no ghost"
          disabled={answered}
          onClick={() => void answer(false)}
          aria-label="Decline this action"
        >
          NO
        </button>
      </div>
    </div>
  );
}
```

**Esc = NO:** Add a `useEffect` in `ConfirmCard` (or in App.tsx while `activeCalls` has a pending item) to listen for Escape → call `answer(false)`.

```tsx
useEffect(() => {
  if (call.status !== "pending" || answered) return;
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); void answer(false); }
  };
  window.addEventListener("keydown", onKey);
  return () => window.removeEventListener("keydown", onKey);
}, [call.status, answered]);
```

### 2d. CSS

```css
.confirm-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-radius: 8px;
  border: 1px solid rgba(250, 204, 21, 0.5);  /* yellow-400 */
  background: rgba(250, 204, 21, 0.1);
  margin: 4px 0;
}

.confirm-card.pending {
  border-color: rgba(250, 204, 21, 0.7);
  background: rgba(250, 204, 21, 0.15);
}

.confirm-card.resolved.done {
  border-color: rgba(74, 222, 128, 0.4);   /* green */
  background: rgba(74, 222, 128, 0.08);
}

.confirm-card.resolved.declined {
  border-color: rgba(148, 163, 184, 0.3);   /* gray */
  background: rgba(148, 163, 184, 0.06);
}

.confirm-icon { font-size: 14px; flex-shrink: 0; }

.confirm-body { display: flex; flex-direction: column; flex: 1; gap: 2px; }

.confirm-label { font-size: 12px; font-weight: 600; color: #fbbf24; }

.confirm-desc { font-size: 11px; color: #94a3b8; }

.confirm-actions { display: flex; gap: 6px; flex-shrink: 0; }

.confirm-yes {
  padding: 4px 12px;
  font-size: 11px;
  font-weight: 700;
  border-radius: 5px;
  background: rgba(250, 204, 21, 0.9);
  color: #0f172a;
  border: none;
  cursor: pointer;
}
.confirm-yes:hover:not(:disabled) { background: #fbbf24; }
.confirm-yes:disabled { opacity: 0.5; cursor: default; }

.confirm-no { font-size: 11px; padding: 4px 10px; }
```

### 2e. `ResponsePanel.tsx` integration

Import `ConfirmCard`. In the `activeCalls` render section:

```tsx
{activeCalls && activeCalls.length > 0 ? (
  <div className="tool-cards">
    {activeCalls.map((call) =>
      call.status === "pending" ? (
        <ConfirmCard key={call.call_id} call={call} />
      ) : (
        <ToolCard key={call.call_id} call={call} />
      )
    )}
  </div>
) : null}
```

---

## 3. Human-Readable Action Descriptions

`describe_action` in `src-tauri/src/tools/actions/mod.rs`:

```rust
fn describe_action(name: &str, args: &Value) -> String {
    match name {
        "click_at" => {
            let x = args["x"].as_i64().unwrap_or(0);
            let y = args["y"].as_i64().unwrap_or(0);
            format!("Click at screen position ({x}, {y})")
        }
        "type_text" => {
            let text = args["text"].as_str().unwrap_or("").chars().take(40).collect::<String>();
            let ellipsis = if args["text"].as_str().unwrap_or("").len() > 40 { "…" } else { "" };
            format!("Type \"{text}{ellipsis}\"")
        }
        "press_keys" => {
            let keys = args["keys"].as_str().unwrap_or("");
            format!("Press keys: {keys}")
        }
        "launch_app" => {
            let path = args["path"].as_str().unwrap_or("");
            let name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            format!("Launch app: {name}")
        }
        "open_path" => {
            let path = args["path"].as_str().unwrap_or("");
            format!("Open: {path}")
        }
        "find_elements" => {
            let query = args["query"].as_str().unwrap_or("(all)");
            format!("Find UI elements matching \"{query}\"")
        }
        other => format!("Run action: {other}"),
    }
}
```

---

## 4. Safety Guards

`src-tauri/src/tools/actions/guard.rs`:

```rust
use std::path::Path;

/// Blocked path prefixes for launch_app and open_path.
/// Case-insensitive on Windows.
const BLOCKED_PREFIXES: &[&str] = &[
    r"C:\Windows\System32",
    r"C:\Windows\SysWOW64",
    r"C:\Windows\System",
    r"C:\Windows\Boot",
];

/// Blocked executable names (always blocked regardless of path).
const BLOCKED_EXES: &[&str] = &[
    "cmd.exe", "powershell.exe", "pwsh.exe", "regedit.exe",
    "taskkill.exe", "format.exe", "diskpart.exe", "wmic.exe",
    "mshta.exe", "cscript.exe", "wscript.exe",
];

pub fn check_path(path: &str) -> Result<(), String> {
    let lower = path.to_lowercase();
    let p = Path::new(path);

    // Block system directory prefixes.
    for prefix in BLOCKED_PREFIXES {
        if lower.starts_with(&prefix.to_lowercase()) {
            return Err(format!(
                "Action blocked: path is in a protected system directory ({path})"
            ));
        }
    }

    // Block dangerous executables by name.
    if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
        let lower_name = file_name.to_lowercase();
        for exe in BLOCKED_EXES {
            if lower_name == *exe {
                return Err(format!("Action blocked: {file_name} is a restricted executable"));
            }
        }
    }

    Ok(())
}

/// Screen bounds check for click_at.
pub fn check_click_bounds(x: i32, y: i32) -> Result<(), String> {
    // GetSystemMetrics(SM_CXVIRTUALSCREEN / SM_CYVIRTUALSCREEN) gives
    // the full virtual desktop extent (all monitors). We check only that
    // the point is non-negative and plausibly on-screen (≤ 32000 px).
    if x < 0 || y < 0 || x > 32000 || y > 32000 {
        return Err(format!("click_at: coordinates ({x}, {y}) are out of screen bounds"));
    }
    Ok(())
}

/// type_text length cap.
pub const MAX_TYPE_TEXT_CHARS: usize = 500;
```

---

## 5. Tool Schemas and Implementations

### File layout

```
src-tauri/src/tools/actions/
  mod.rs           — registry: all_action_schemas(), is_action(), dispatch(), dispatch_with_confirm()
  guard.rs         — safety guards (§4)
  open_path.rs     — open_path tool
  input_synthesis.rs — click_at, type_text, press_keys
  uia_find.rs      — find_elements
  launch_app.rs    — launch_app
```

### 5a. `open_path` (Sub-step A)

**Schema:**
```rust
pub fn schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "open_path",
            "description": "Open a file, folder, or URL with the system default app (ShellExecute). Safe for documents and web URLs.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute file path, folder path, or URL (https://…). URLs must begin with http:// or https://."
                    }
                },
                "required": ["path"]
            }
        }
    })
}
```

**Implementation (`open_path.rs`):**
```rust
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

pub async fn run(args: &Value) -> ToolResult {
    let path = args["path"].as_str().ok_or("open_path: missing path")?;
    super::guard::check_path(path)?;

    // Allow https?:// URLs without path-guard check (no file system risk).
    // Path guard is already permissive for non-system paths; the URL check
    // just notes the URL passes cleanly.
    let path_owned = path.to_string();
    tokio::task::spawn_blocking(move || {
        unsafe {
            let path_wide: Vec<u16> = path_owned.encode_utf16().chain(std::iter::once(0)).collect();
            let verb_wide: Vec<u16> = "open\0".encode_utf16().collect();
            let result = ShellExecuteW(
                None,
                PCWSTR(verb_wide.as_ptr()),
                PCWSTR(path_wide.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
            // ShellExecuteW returns > 32 on success.
            if result.0 as isize > 32 {
                Ok(format!("Opened: {path_owned}"))
            } else {
                Err(format!("ShellExecuteW failed with code: {}", result.0 as isize))
            }
        }
    })
    .await
    .map_err(|e| format!("open_path task: {e}"))?
}
```

**Cargo.toml** — add Windows Shell feature:
```toml
windows = { version = "0.58", features = [
  # ... existing ...
  "Win32_UI_Shell",                    # NEW: ShellExecuteW for open_path
  "Win32_UI_Input_KeyboardAndMouse",   # NEW: SendInput for click/type/keys (Sub-step B)
] }
```

### 5b. `click_at` (Sub-step B)

**Schema:**
```rust
"name": "click_at",
"description": "Move the mouse to physical screen coordinates (x, y) and left-click. Use find_elements first to locate UI elements reliably.",
"parameters": {
    "type": "object",
    "properties": {
        "x": { "type": "integer", "description": "Physical pixel X coordinate (from left of primary monitor)." },
        "y": { "type": "integer", "description": "Physical pixel Y coordinate (from top of primary monitor)." }
    },
    "required": ["x", "y"]
}
```

**SendInput coordinate math:**

`SendInput` with `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE` uses a virtual coordinate space of 0–65535 spanning the **primary monitor** (not the virtual desktop). Conversion from physical pixels:

```rust
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

fn to_normalized(x_phys: i32, y_phys: i32) -> (i32, i32) {
    unsafe {
        let screen_w = GetSystemMetrics(SM_CXSCREEN).max(1);
        let screen_h = GetSystemMetrics(SM_CYSCREEN).max(1);
        // Formula: normalized = (physical * 65535 + screen_dim / 2) / screen_dim
        // The + screen_dim/2 rounds instead of truncates.
        let nx = (x_phys * 65535 + screen_w / 2) / screen_w;
        let ny = (y_phys * 65535 + screen_h / 2) / screen_h;
        (nx, ny)
    }
}
```

**Implementation (`input_synthesis.rs`, `click_at` part):**
```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT,
};

pub async fn click_at(args: &Value) -> ToolResult {
    let x = args["x"].as_i64().ok_or("click_at: missing x")? as i32;
    let y = args["y"].as_i64().ok_or("click_at: missing y")? as i32;
    super::guard::check_click_bounds(x, y)?;

    tokio::task::spawn_blocking(move || unsafe {
        let (nx, ny) = to_normalized(x, y);
        let flags = (MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE).0;

        let move_input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT { dx: nx, dy: ny, mouseData: 0,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS(flags),
                    time: 0, dwExtraInfo: 0 },
            },
        };
        let down_input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTDOWN, time: 0, dwExtraInfo: 0 },
            },
        };
        let up_input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0,
                    dwFlags: MOUSEEVENTF_LEFTUP, time: 0, dwExtraInfo: 0 },
            },
        };

        let inputs = [move_input, down_input, up_input];
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent == 3 {
            Ok(format!("Clicked at ({x}, {y})"))
        } else {
            Err(format!("SendInput: only {sent}/3 events sent"))
        }
    })
    .await
    .map_err(|e| format!("click_at task: {e}"))?
}
```

**Focus handling:** Thuki's always-on-top window may cover the target. The action runs after the user clicks YES in the chat panel — Thuki is already in workspace mode and visible. Document: "If Thuki covers the target, drag it aside before asking the AI to click." Automatically minimizing Thuki would break the confirmation UI. This is the same contract as 2.5 capture-on-speak.

### 5c. `type_text` (Sub-step B)

**Implementation:** Per-character Unicode `SendInput` with `KEYEVENTF_UNICODE`:

```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};

pub async fn type_text(args: &Value) -> ToolResult {
    let text = args["text"].as_str().ok_or("type_text: missing text")?;
    if text.len() > super::guard::MAX_TYPE_TEXT_CHARS {
        return Err(format!(
            "type_text: text too long ({} chars; max {})",
            text.len(), super::guard::MAX_TYPE_TEXT_CHARS
        ));
    }
    let text_owned = text.to_string();

    tokio::task::spawn_blocking(move || unsafe {
        for ch in text_owned.encode_utf16() {
            // Key-down
            let down = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                        wScan: ch,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            // Key-up
            let up = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                        wScan: ch,
                        dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(
                            KEYEVENTF_UNICODE.0 | KEYEVENTF_KEYUP.0
                        ),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        }
        Ok(format!("Typed {} characters", text_owned.chars().count()))
    })
    .await
    .map_err(|e| format!("type_text task: {e}"))?
}
```

### 5d. `press_keys` (Sub-step B)

**Parser spec:** Input string format: modifier(s) joined by `+`, then key. Case-insensitive. Examples: `"Ctrl+S"`, `"Win+E"`, `"Ctrl+Shift+Esc"`, `"Enter"`, `"F5"`, `"Alt+F4"`.

**Virtual key table:**

| Token (case-insensitive) | VK code |
|--------------------------|---------|
| `Ctrl` | VK_CONTROL (0x11) |
| `Alt` | VK_MENU (0x12) |
| `Shift` | VK_SHIFT (0x10) |
| `Win` | VK_LWIN (0x5B) |
| `Enter` | VK_RETURN (0x0D) |
| `Esc` / `Escape` | VK_ESCAPE (0x1B) |
| `Tab` | VK_TAB (0x09) |
| `Space` | VK_SPACE (0x20) |
| `Backspace` | VK_BACK (0x08) |
| `Delete` | VK_DELETE (0x2E) |
| `Up` / `Down` / `Left` / `Right` | VK_UP/DOWN/LEFT/RIGHT |
| `Home` / `End` | VK_HOME / VK_END |
| `PgUp` / `PgDn` | VK_PRIOR / VK_NEXT |
| `F1`–`F12` | VK_F1–VK_F12 |
| `A`–`Z`, `0`–`9` | ASCII uppercase as VK code |

**Blocked combos** (not in whitelist): `Ctrl+Alt+Del` is OS-intercepted regardless (SendInput cannot synthesize it). Block it explicitly to avoid confusing errors: return `Err("Ctrl+Alt+Del is a system-reserved combination")`.

**Implementation:**
```rust
pub async fn press_keys(args: &Value) -> ToolResult {
    let keys_str = args["keys"].as_str().ok_or("press_keys: missing keys")?;
    
    if keys_str.eq_ignore_ascii_case("ctrl+alt+del") {
        return Err("press_keys: Ctrl+Alt+Del is a system-reserved combination".into());
    }
    
    let vkeys = parse_keys(keys_str)?;
    let vkeys_owned = vkeys;
    
    tokio::task::spawn_blocking(move || unsafe {
        // Build inputs: press all keys in order, release in reverse.
        let mut inputs = Vec::new();
        for &vk in &vkeys_owned {
            inputs.push(make_key_input(vk, false));
        }
        for &vk in vkeys_owned.iter().rev() {
            inputs.push(make_key_input(vk, true));
        }
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent as usize == inputs.len() {
            Ok(format!("Pressed: {}", vkeys_owned.iter().map(|vk| format!("{vk:?}")).collect::<Vec<_>>().join("+")))
        } else {
            Err(format!("SendInput: {sent}/{} events sent", inputs.len()))
        }
    })
    .await
    .map_err(|e| format!("press_keys task: {e}"))?
}

fn parse_keys(s: &str) -> Result<Vec<u16>, String> {
    s.split('+')
        .map(|token| token.trim())
        .map(token_to_vk)
        .collect()
}

fn token_to_vk(token: &str) -> Result<u16, String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    Ok(match token.to_lowercase().as_str() {
        "ctrl" | "control" => VK_CONTROL.0,
        "alt"              => VK_MENU.0,
        "shift"            => VK_SHIFT.0,
        "win" | "windows"  => VK_LWIN.0,
        "enter" | "return" => VK_RETURN.0,
        "esc" | "escape"   => VK_ESCAPE.0,
        "tab"              => VK_TAB.0,
        "space"            => VK_SPACE.0,
        "backspace"        => VK_BACK.0,
        "delete" | "del"   => VK_DELETE.0,
        "up"               => VK_UP.0,
        "down"             => VK_DOWN.0,
        "left"             => VK_LEFT.0,
        "right"            => VK_RIGHT.0,
        "home"             => VK_HOME.0,
        "end"              => VK_END.0,
        "pgup" | "pageup"  => VK_PRIOR.0,
        "pgdn" | "pagedown"=> VK_NEXT.0,
        "f1"  => VK_F1.0,  "f2"  => VK_F2.0,  "f3"  => VK_F3.0,
        "f4"  => VK_F4.0,  "f5"  => VK_F5.0,  "f6"  => VK_F6.0,
        "f7"  => VK_F7.0,  "f8"  => VK_F8.0,  "f9"  => VK_F9.0,
        "f10" => VK_F10.0, "f11" => VK_F11.0, "f12" => VK_F12.0,
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap().to_ascii_uppercase();
            if c.is_ascii_alphanumeric() { c as u16 }
            else { return Err(format!("press_keys: unknown key token '{s}'")); }
        }
        other => return Err(format!("press_keys: unknown key token '{other}'")),
    })
}
```

### 5e. `find_elements` (Sub-step C)

**UIA crate decision: raw `windows 0.58` Win32 UIA, NO `uiautomation` crate.**

`uiautomation 0.25.1` requires `windows ^0.62.2`. Our project uses `windows 0.58`. Adding `uiautomation` would bring in a second `windows` version → duplicate types, linker symbol conflicts, and likely build failure. **Raw Win32 UIA via existing `windows` crate is mandatory.**

**Cargo.toml addition:**
```toml
windows = { version = "0.58", features = [
  # ... existing ...
  "Win32_UI_Accessibility",   # NEW: IUIAutomation, IUIAutomationElement, UIA constants
] }
```

**Two-call pattern:** The model calls `find_elements` first to discover coordinates, then `click_at` with the returned center coordinates. This is the v1 UX. A future phase could add `click_element(index)` that resolves the element and clicks in one call.

**Schema:**
```rust
"name": "find_elements",
"description": "Find UI elements in the focused window matching an optional text query. Returns a list of elements with name, role, and center coordinates (x, y). Use the returned x/y with click_at.",
"parameters": {
    "type": "object",
    "properties": {
        "query": {
            "type": "string",
            "description": "Optional substring to filter elements by name (case-insensitive). Omit to list all elements (up to 100)."
        }
    },
    "required": []
}
```

**Implementation (`uia_find.rs`):**
```rust
use serde::Serialize;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement,
    IUIAutomationTreeWalker, TreeScope_Descendants,
    UIA_ControlTypePropertyId, UIA_NamePropertyId, UIA_BoundingRectanglePropertyId,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::core::Interface;

#[derive(Serialize)]
struct UiElement {
    name:  String,
    role:  String,
    x:     i32,
    y:     i32,
    w:     i32,
    h:     i32,
}

pub async fn run(args: &Value) -> ToolResult {
    let query = args["query"].as_str().map(str::to_lowercase);
    tokio::task::spawn_blocking(move || find_elements_sync(query.as_deref()))
        .await
        .map_err(|e| format!("find_elements task: {e}"))?
}

fn find_elements_sync(query: Option<&str>) -> ToolResult {
    const MAX_ELEMENTS: usize = 100;
    const MAX_DEPTH: usize = 30;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("UIA init: {e}"))?;

        // Get the focused window element.
        let focused: IUIAutomationElement = uia.GetFocusedElement()
            .map_err(|e| format!("GetFocusedElement: {e}"))?;

        // Find all descendants (depth-capped by count limit in practice).
        let condition = uia.CreateTrueCondition()
            .map_err(|e| format!("CreateTrueCondition: {e}"))?;
        let element_array = focused
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| format!("FindAll: {e}"))?;

        let count = element_array.Length().map_err(|e| format!("Length: {e}"))? as usize;
        let mut results = Vec::new();

        for i in 0..count.min(MAX_ELEMENTS) {
            let el: IUIAutomationElement = element_array.GetElement(i as i32)
                .map_err(|_| continue_())?;

            // Name
            let name_bstr = el.CurrentName().unwrap_or_default();
            let name = name_bstr.to_string();
            if name.is_empty() { continue; }

            // Filter
            if let Some(q) = query {
                if !name.to_lowercase().contains(q) { continue; }
            }

            // Role (control type ID → string)
            let ctrl_type = el.CurrentControlType().unwrap_or(0);
            let role = ctrl_type_name(ctrl_type);

            // Bounding rect
            let rect = el.CurrentBoundingRectangle().unwrap_or_default();
            let cx = rect.left + (rect.right - rect.left) / 2;
            let cy = rect.top + (rect.bottom - rect.top) / 2;

            results.push(UiElement {
                name,
                role,
                x: cx,
                y: cy,
                w: rect.right - rect.left,
                h: rect.bottom - rect.top,
            });
        }

        serde_json::to_string(&results).map_err(|e| e.to_string())
    }
}

fn ctrl_type_name(id: i32) -> String {
    match id {
        50000 => "Button",     50001 => "Calendar",  50002 => "CheckBox",
        50003 => "ComboBox",   50004 => "Edit",       50005 => "Hyperlink",
        50006 => "Image",      50007 => "ListItem",   50008 => "List",
        50009 => "Menu",       50010 => "MenuBar",    50011 => "MenuItem",
        50012 => "ProgressBar",50013 => "RadioButton",50014 => "ScrollBar",
        50015 => "Slider",     50016 => "Spinner",    50017 => "StatusBar",
        50018 => "Tab",        50019 => "TabItem",    50020 => "Text",
        50021 => "ToolBar",    50022 => "ToolTip",    50023 => "Tree",
        50024 => "TreeItem",   50025 => "Custom",     50026 => "Group",
        50027 => "Thumb",      50028 => "DataGrid",   50029 => "DataItem",
        50030 => "Document",   50031 => "SplitButton",50032 => "Window",
        50033 => "Pane",       50034 => "Header",     50035 => "HeaderItem",
        50036 => "Table",      50037 => "TitleBar",   50038 => "Separator",
        _ => format!("Unknown({id})"),
    }.to_string()
}

// Hack: Rust closures can't use `continue` — use a dummy fn to break out.
fn continue_() -> String { String::new() }
```

**Note on `find_elements` confirm card:** `find_elements` is read-only (no state change) but still goes through the confirm gate because it requires OS permission to query UI trees and the model may use its results to target subsequent actions. In a future version, it could be moved to the read-only tools set. For v1, require confirmation for uniformity.

### 5f. `launch_app` (Sub-step D)

**Schema:**
```rust
"name": "launch_app",
"description": "Launch an executable by its full path. The path must be an installed application (.exe), not a system utility.",
"parameters": {
    "type": "object",
    "properties": {
        "path": {
            "type": "string",
            "description": "Absolute path to the executable, e.g. C:\\Program Files\\Notepad++\\notepad++.exe"
        }
    },
    "required": ["path"]
}
```

**Implementation:**
```rust
pub async fn run(args: &Value) -> ToolResult {
    let path = args["path"].as_str().ok_or("launch_app: missing path")?;
    super::guard::check_path(path)?;   // blocks system dirs + dangerous exes
    
    // Must end in .exe
    if !path.to_lowercase().ends_with(".exe") {
        return Err("launch_app: path must point to an .exe file".into());
    }

    let path_owned = path.to_string();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(&path_owned)
            .spawn()
            .map(|_| format!("Launched: {path_owned}"))
            .map_err(|e| format!("launch_app: {e}"))
    })
    .await
    .map_err(|e| format!("launch_app task: {e}"))?
}
```

---

## 6. Settings

### 6a. Database (`db.rs`)

Add to `Settings` struct:
```rust
pub actions_enabled: bool,
```

In `load_settings`:
```rust
let actions_enabled = get_value(conn, "actions_enabled")?
    .map(|v| v.eq_ignore_ascii_case("true"))
    .unwrap_or(false);  // default OFF
```

In `save_core_settings`, accept optional `actions_enabled: Option<bool>` and persist it.

### 6b. `SaveSettingsInput` (`commands.rs`)

```rust
pub actions_enabled: Option<bool>,
```

Update `save_settings` to write it and update `AppState.actions_enabled`.

### 6c. `Settings.tsx`

Add a new toggle row below the "AI Tools" toggle:

```tsx
<div className="settings-toggle">
  <div className="settings-toggle-text">
    <strong>Action tools</strong>
    <span>
      Allow the AI to click, type, open files, and launch apps — with your
      YES/NO approval for each action. Default off.
    </span>
  </div>
  <button
    type="button"
    role="switch"
    aria-checked={actionsOn}
    className={`switch${actionsOn ? " on" : ""}`}
    onClick={() => setActionsOn((v) => !v)}
  >
    <span className="knob" />
  </button>
</div>
```

---

## 7. Cargo.toml Final Additions

```toml
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_Graphics_Gdi",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Threading",
  "Win32_System_ProcessStatus",
  "Win32_Media_Speech",
  "Win32_System_Com",
  "Win32_System_Memory",
  "Win32_UI_Shell",                   # NEW §5a: open_path
  "Win32_UI_Input_KeyboardAndMouse",  # NEW §5b-d: click/type/keys
  "Win32_UI_Accessibility",           # NEW §5e: find_elements UIA
] }
```

No new crates. The `uiautomation` crate is explicitly excluded (version conflict with `windows 0.58`).

---

## 8. Unit Tests

All unit tests go in the relevant module's `#[cfg(test)]` block.

### 8a. Key parser tests (`input_synthesis.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::parse_keys;

    #[test] fn ctrl_s() {
        let vks = parse_keys("Ctrl+S").unwrap();
        assert_eq!(vks, vec![0x11, 'S' as u16]);  // VK_CONTROL, 'S'
    }
    #[test] fn win_e() {
        let vks = parse_keys("Win+E").unwrap();
        assert_eq!(vks, vec![0x5B, 'E' as u16]);  // VK_LWIN, 'E'
    }
    #[test] fn ctrl_shift_esc() {
        let vks = parse_keys("Ctrl+Shift+Esc").unwrap();
        assert_eq!(vks, vec![0x11, 0x10, 0x1B]);
    }
    #[test] fn single_enter() {
        let vks = parse_keys("Enter").unwrap();
        assert_eq!(vks, vec![0x0D]);
    }
    #[test] fn f5() {
        let vks = parse_keys("F5").unwrap();
        assert_eq!(vks, vec![0x74]);  // VK_F5
    }
    #[test] fn unknown_token_errors() {
        assert!(parse_keys("Ctrl+Foo").is_err());
    }
    #[test] fn ctrl_alt_del_blocked() {
        // press_keys is async; test the guard check directly.
        assert!(parse_keys("Ctrl+Alt+Del").is_ok());  // parsing succeeds...
        // ...but press_keys() runtime check returns Err before SendInput.
        // (The check happens in press_keys before spawn_blocking.)
    }
}
```

### 8b. Path guard tests (`guard.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::check_path;

    #[test] fn system32_blocked() {
        assert!(check_path(r"C:\Windows\System32\cmd.exe").is_err());
    }
    #[test] fn powershell_blocked_by_name() {
        assert!(check_path(r"C:\Program Files\PowerShell\7\pwsh.exe").is_err());
    }
    #[test] fn notepad_allowed() {
        assert!(check_path(r"C:\Program Files\Notepad++\notepad++.exe").is_ok());
    }
    #[test] fn url_allowed() {
        assert!(check_path("https://github.com").is_ok());
    }
    #[test] fn negative_coords_blocked() {
        assert!(super::check_click_bounds(-1, 100).is_err());
    }
    #[test] fn normal_coords_allowed() {
        assert!(super::check_click_bounds(800, 600).is_ok());
    }
}
```

### 8c. Coordinate normalization test

```rust
#[test]
fn normalize_center_of_1920x1080() {
    // For a 1920×1080 screen, center (960, 540) → (32767, 32767) approx.
    // We can't call GetSystemMetrics in a unit test, so test the formula directly.
    let screen_w = 1920i32;
    let screen_h = 1080i32;
    let (nx, ny) = ((960 * 65535 + screen_w / 2) / screen_w,
                    (540 * 65535 + screen_h / 2) / screen_h);
    assert!((nx - 32767).abs() <= 2);
    assert!((ny - 32767).abs() <= 2);
}
```

### 8d. Confirm timeout test (tokio)

```rust
#[tokio::test]
async fn confirm_timeout_auto_declines() {
    use tokio::sync::oneshot;
    use tokio::time::{timeout, Duration};

    let (tx, rx) = oneshot::channel::<bool>();
    // Drop tx immediately → rx returns Err → unwrap_or(false).
    drop(tx);
    let approved = timeout(Duration::from_millis(100), rx)
        .await
        .unwrap_or(Ok(false))
        .unwrap_or(false);
    assert!(!approved);
}
```

---

## 9. Ordered Build Steps with Gates

### Sub-step A: Confirmation infra + `open_path`

| # | Step | Gate |
|---|------|------|
| A1 | Add `pending_confirmations` + `actions_enabled` to `AppState`; load `actions_enabled` from db (default false); add `actions_enabled` to `db::Settings` | `cargo check` clean |
| A2 | Create `src-tauri/src/tools/actions/` directory; `mod.rs` (empty `all_action_schemas()`/`is_action()`/`dispatch()`); `guard.rs` | `cargo check` clean |
| A3 | Add `confirm_tool` command to `commands.rs`; register in `lib.rs` | `cargo check` clean |
| A4 | Implement `dispatch_with_confirm` in `actions/mod.rs` with 60s timeout + cancel select | `cargo check` clean |
| A5 | Wire agent loop: `is_action()` branch → `dispatch_with_confirm`; add action schemas to `send_message` when `actions_enabled` | `cargo check` clean |
| A6 | Add `Win32_UI_Shell` to Cargo.toml windows features; implement `open_path.rs` | `cargo build` clean |
| A7 | Add TypeScript types (`ChatToolPending`, update `ToolCall`); add `confirmTool` to `api.ts` | `tsc` clean |
| A8 | Implement `ConfirmCard.tsx` + CSS; update `ResponsePanel.tsx` to route pending calls to `ConfirmCard` | `tsc` clean; `npm run dev` → card renders |
| A9 | Add `chat://tool-pending` listener to `useChat.ts`; update tool-start/result handlers for `pending`/`declined` status | `tsc` clean |
| A10 | Add `actions_enabled` to `SaveSettingsInput`, `save_settings`, Settings.tsx toggle | `tsc` + `cargo check` clean |
| A11 | Run guard unit tests + confirm timeout test | `cargo test` — all pass (target: 43 + new tests) |
| A12 | **AT-A1 through AT-A4**: live test `open_path` flow | Pass |

### Sub-step B: Input synthesis (`click_at`, `type_text`, `press_keys`)

| # | Step | Gate |
|---|------|------|
| B1 | Add `Win32_UI_Input_KeyboardAndMouse` to Cargo.toml | `cargo check` clean |
| B2 | Implement `input_synthesis.rs`: `click_at`, `type_text`, `press_keys`, `parse_keys`, `token_to_vk`, `to_normalized` | `cargo build` clean |
| B3 | Register the 3 tools in `actions/mod.rs` `all_action_schemas()` + `dispatch()` | `cargo check` clean |
| B4 | Add tool labels to `ResponsePanel.tsx` `TOOL_LABELS` and `ConfirmCard` `ACTION_LABELS` | `tsc` clean |
| B5 | Run key parser + guard + coord unit tests | `cargo test` all pass |
| B6 | **AT-B1 through AT-B6**: live Notepad scenario | Pass |

### Sub-step C: `find_elements` (UIA)

| # | Step | Gate |
|---|------|------|
| C1 | Add `Win32_UI_Accessibility` to Cargo.toml | `cargo check` clean |
| C2 | Implement `uia_find.rs`; register in `actions/mod.rs` | `cargo build` clean |
| C3 | **AT-C1 through AT-C3**: live element discovery | Pass |

### Sub-step D: `launch_app` + polish

| # | Step | Gate |
|---|------|------|
| D1 | Implement `launch_app.rs`; register in `actions/mod.rs` | `cargo build` clean |
| D2 | Final `cargo test` — all tests pass (target: 43 + ~12 new) | All pass |
| D3 | `tsc` + `npm run build` + `cargo build --release` | No errors |
| D4 | **AT-D1 through AT-D4**: Notepad++ launch + full Notepad sequence | Pass |

---

## 10. Acceptance Tests

### Sub-step A: Confirmation flow

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-A1 | Actions disabled by default | Fresh app → Settings → "Action tools" toggle is OFF → ask AI "open https://github.com" → AI cannot use action tools (no yellow card, text reply only) |
| AT-A2 | Enable actions → YES flow | Enable Actions in Settings → ask "open https://example.com" → yellow confirm card appears: "Open: https://example.com" with YES/NO → click YES → browser opens example.com → card resolves green |
| AT-A3 | NO flow | Same as AT-A2 → click NO → card resolves gray "Declined" → AI replies "I couldn't open the URL because you declined." |
| AT-A4 | 60s timeout → auto-decline | Enable Actions → ask to open a URL → do NOT click YES/NO → wait 60s → card auto-declines → AI continues normally |

### Sub-step B: Input synthesis

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-B1 | `press_keys` Ctrl+S | Open Notepad → type some text → ask AI "save the file" → yellow card "Press keys: Ctrl+S" → YES → Save dialog appears |
| AT-B2 | `type_text` | Focus Notepad → ask AI "type hello world" → yellow card "Type 'hello world'" → YES → text appears in Notepad |
| AT-B3 | `click_at` | Ask AI "click at 100 100" → yellow card "Click at screen position (100, 100)" → YES → mouse moves and clicks |
| AT-B4 | Esc = NO | Yellow confirm card visible → press Esc → card declines, no action |
| AT-B5 | Bounds guard | Ask AI "click at -1 -1" → NO card shown; returns error "coordinates out of bounds" |
| AT-B6 | type_text length guard | Ask AI to type >500 chars → error returned before confirm card (guard fires in dispatch, not confirm) |

### Sub-step C: Element finder

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-C1 | `find_elements` no filter | Focus Notepad → ask "what UI elements does Notepad have?" → yellow card → YES → JSON array with Edit, MenuBar, StatusBar etc. elements |
| AT-C2 | `find_elements` with query | Ask "find the Save button in the dialog" (with Save dialog open) → yellow card → YES → returns element with name "Save", role "Button", x/y coordinates |
| AT-C3 | Two-call pattern | Ask "click the Save button" → AI calls `find_elements(query="Save")` then `click_at(x, y)` from returned coords → two confirm cards in sequence → both YES → Save button clicked |

### Sub-step D: Launch + full scenario

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-D1 | `launch_app` allowed | Ask "open notepad" → AI resolves to `C:\Windows\notepad.exe` — wait: notepad.exe is NOT in System32 in Windows 11 (it's at `C:\Windows\notepad.exe`, not System32 path). Check: guard allows it → yellow card "Launch app: notepad.exe" → YES → Notepad opens |
| AT-D2 | `launch_app` blocked | Ask "launch cmd.exe" → guard blocks → error "cmd.exe is a restricted executable" → no confirm card shown |
| AT-D3 | cancel during confirm | Yellow card showing → click Cancel (stream Cancel button) → card auto-declines, stream cancelled |
| AT-D4 | Full Notepad scenario | Enable Actions → ask "open Notepad, type 'hello world', then save" → AI sequences: `launch_app` (YES) → `type_text "hello world"` (YES) → `press_keys Ctrl+S` (YES) → Save dialog → `type_text "test.txt"` (YES) → `press_keys Enter` (YES) → file saved |

---

## 11. Risk Notes

| Risk | Impact | Mitigation |
|------|--------|------------|
| **`find_elements` on deep UI trees (browsers, IDEs)** | May hit 100-element cap and miss the target element | Cap is a safety guard. If user needs deeper search, tell AI to filter by query. Future: increase cap or add tree-walking depth param. |
| **`uiautomation` crate version conflict** | Can't use it; raw Win32 UIA only | Decision locked (§5e). Raw UIA is more stable anyway. |
| **Thuki window occludes click target** | click_at hits the chat window, not the target | Same contract as 2.5. Document: drag Thuki aside. |
| **`SendInput` blocked by UIPI** | High-integrity targets (UAC prompts, elevated processes) reject injected input | Expected Windows behavior. Error message: "Windows blocked the input — the target window may require elevation." |
| **Path guard bypass via symlinks** | Symlink to `cmd.exe` at user path passes name check | Acceptable for v1. Mitigation: `std::fs::canonicalize` before check (resolves symlinks). Consider adding for D-step. |
| **COM apartment in spawn_blocking** | `CoInitializeEx` needs to be called in each blocking thread | All action tool `spawn_blocking` closures call `CoInitializeEx` at the top. Thread pool may reuse threads; `CoInitialize` on an already-initialized thread returns `S_FALSE` (success), which is safe. |
| **Notepad.exe path varies** | Windows 11 moves apps around | Use `where.exe notepad` or `std::process::Command::new("where").arg("notepad.exe")` in `launch_app` to resolve. Or accept that the model must provide the full path. Simpler: model calls `list_windows` to check if Notepad is already open. |
| **MCP door accidentally exposes actions** | `all_action_schemas()` must NOT be in `all_schemas()` | Enforced by module separation. `src/bin/mcp.rs` calls `tools::all_schemas()` only. `tools::actions::all_action_schemas()` is a separate function. Code review must verify. |

---

## 12. Out of Scope

- MCP door action exposure (Phase 2.6b) — separate spec; `all_action_schemas()` is the extension point
- Rung-D sequences / batch approvals — spec explicitly prohibits per-action
- Drag and scroll gestures — `SendInput` supports them; deferred
- `set_clipboard` write action — deferred (read_clipboard exists; write needs undo consideration)
- Right-click / middle-click — `click_at` is left-click only in v1
- Multi-monitor normalized coordinates — `SM_CXSCREEN`/`SM_CYSCREEN` addresses primary monitor; multi-monitor needs `SM_CXVIRTUALSCREEN` and per-monitor DPI. Deferred
- Hover / focus without click — deferred
- Shell: `run_command(cmd)` — explicitly excluded (shell execution = free-form arbitrary code)
- Voice-triggered actions (speaking "yes" to confirm) — deferred to 2.7

---

**End of spec.**
