# Phase 2.3b — MCP stdio Server (`thuki-mcp`)

**Status:** SPEC  
**Estimated effort:** 1 week  
**Depends on:** Phase 2.3 (VERDICT OK)  
**Out of scope:** HTTP/SSE transport (2.3c), auth/tokens, action tools (2.6), tunnel/ngrok

---

## Goal

Expose Thuki's 6 read-only tools over the Model Context Protocol (MCP) stdio transport so external clients (Claude Desktop, ZCode, any MCP-capable host) can call them without the Tauri GUI being involved. The tools module is written once and shared by Door 1 (agent loop in `commands.rs`) and Door 2 (MCP server binary).

---

## 1. Binary Layout Decision

**Decision: `[[bin]]` in the same Cargo package, entry at `src-tauri/src/bin/mcp.rs`.**

Rationale:
- No workspace restructure needed.
- The binary links against the same `thuki_win_lib` library crate, so `tools::*` modules are shared without duplication.
- `cargo build --bin thuki-mcp` produces a standalone exe with no Tauri app handle.
- The `lib` target (used by the Tauri app) is unaffected.

The binary has its own `#[tokio::main]` entry point. It does **not** initialise Tauri, AppState, or SQLite.

---

## 2. `spawn_blocking` Migration

**Decision: Switch `tauri::async_runtime::spawn_blocking` → `tokio::task::spawn_blocking` everywhere in `tools/`.**

Justification:
- The Tauri GUI already runs inside a `tokio` multi-thread runtime (Tauri v2 uses tokio internally). `tokio::task::spawn_blocking` works identically inside the Tauri runtime — this is a drop-in replacement with no behavioural change for Door 1.
- The standalone `thuki-mcp` binary has its own `#[tokio::main]` runtime. `tauri::async_runtime::spawn_blocking` would panic here because the Tauri global runtime is never initialised.
- After the switch, both doors use the same code path with no conditional compilation needed.

Files to touch: `src-tauri/src/tools/mod.rs` and any tool module that calls `spawn_blocking` directly (audit: `capture.rs`, `windows_info.rs`, `clipboard.rs`, `speak.rs`). `datetime.rs` is pure sync and needs no change.

---

## 3. Cargo.toml Changes

File: `src-tauri/Cargo.toml`

### 3a. Add rmcp dependency

```toml
[dependencies]
# ... existing deps unchanged ...
rmcp = { version = "3.4.0", features = ["server", "transport-io", "macros", "schemars"] }
```

`server` + `macros` are defaults but list them explicitly for clarity. `transport-io` enables `stdio()`. `schemars` enables JSON Schema generation for tool parameters (needed by `#[tool]` macro).

### 3b. Add the binary target

```toml
[[bin]]
name   = "thuki-mcp"
path   = "src/bin/mcp.rs"
```

The existing implicit binary (if any) from `[[bin]]` or the default `src/main.rs` is unaffected.

### 3c. reqwest version

**No change required.** rmcp 3.4.0 does not pull in reqwest as a dependency for the server+transport-io feature set (HTTP client is only needed for the HTTP transport feature, which we are not enabling). Confirm with `cargo tree -p rmcp` after adding — if a conflict appears, bump reqwest to `0.13` in our dep and update any breaking call sites (primarily `client.rs` header map construction). This is gated behind the build step check below.

---

## 4. `src-tauri/src/bin/mcp.rs` — Full Source

```rust
//! thuki-mcp: MCP stdio server exposing Thuki's 6 read-only tools.
//! Run by Claude Desktop / ZCode; no Tauri app handle required.

use rmcp::{
    ErrorData, ServerHandler,
    model::{CallToolResult, Content, Tool},
    schemars,
    service::serve_server,
    tool, tool_handler, tool_router,
    transport::io::stdio,
};
use serde::Deserialize;
use std::sync::Arc;
use thuki_win_lib::tools;

// ---------------------------------------------------------------------------
// Parameter structs (schemars-derived for JSON Schema generation)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CaptureScreenParams {
    /// Optional sub-region to capture: [x, y, width, height] in logical pixels.
    /// Omit or set to null for full primary monitor.
    region: Option<[i32; 4]>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SpeakParams {
    /// Text for the SAPI voice to speak aloud (fire-and-forget, async).
    text: String,
}

// Zero-field structs for tools with no parameters.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NoParams {}

// ---------------------------------------------------------------------------
// Server struct
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ThukiMcpServer;

#[tool_router]
impl ThukiMcpServer {
    /// Capture a screenshot of the primary monitor (or a sub-region).
    /// Returns a data URL string (PNG, base64-encoded).
    /// Also silently saves to Pictures\Thuki and copies to clipboard.
    #[tool(description = "Capture a screenshot of the primary monitor or a sub-region. Returns a PNG data URL string. Also saves to Pictures\\Thuki and copies to clipboard.")]
    async fn capture_screen(
        &self,
        #[tool(aggr)] params: CaptureScreenParams,
    ) -> Result<CallToolResult, ErrorData> {
        let region = params.region.map(|r| tools::RegionArg {
            x: r[0],
            y: r[1],
            width: r[2],
            height: r[3],
        });
        let result = tools::dispatch("capture_screen", &serde_json::json!({ "region": region }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.output)]))
    }

    /// List all visible top-level windows (title + process name), excluding Thuki itself.
    #[tool(description = "List all visible top-level windows with their title and process name, excluding Thuki itself.")]
    async fn list_windows(
        &self,
        #[tool(aggr)] _params: NoParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = tools::dispatch("list_windows", &serde_json::json!({}))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.output)]))
    }

    /// Get the title and process name of the currently focused window.
    #[tool(description = "Get the title and process name of the currently focused (active) window.")]
    async fn get_active_window(
        &self,
        #[tool(aggr)] _params: NoParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = tools::dispatch("get_active_window", &serde_json::json!({}))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.output)]))
    }

    /// Read the current text content of the Windows clipboard.
    #[tool(description = "Read the current text content of the Windows clipboard.")]
    async fn read_clipboard(
        &self,
        #[tool(aggr)] _params: NoParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = tools::dispatch("read_clipboard", &serde_json::json!({}))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.output)]))
    }

    /// Get the current date and time (local timezone, ISO 8601).
    #[tool(description = "Get the current local date and time in ISO 8601 format.")]
    async fn get_datetime(
        &self,
        #[tool(aggr)] _params: NoParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = tools::dispatch("get_datetime", &serde_json::json!({}))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.output)]))
    }

    /// Speak text aloud using Windows SAPI (fire-and-forget, async).
    #[tool(description = "Speak text aloud using Windows SAPI text-to-speech. Returns immediately; speech plays in background.")]
    async fn speak(
        &self,
        #[tool(aggr)] params: SpeakParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = tools::dispatch("speak", &serde_json::json!({ "text": params.text }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(result.output)]))
    }
}

#[tool_handler]
impl ServerHandler for ThukiMcpServer {}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // All logging goes to stderr so stdout stays clean for JSON-RPC.
    eprintln!("[thuki-mcp] starting stdio server");
    let service = serve_server(ThukiMcpServer, stdio()).await?;
    service.waiting().await?;
    eprintln!("[thuki-mcp] server shut down");
    Ok(())
}
```

### 4a. `capture_screen` result format decision

**Decision: text block containing the data URL string.**

Rationale:
- The data URL (`data:image/png;base64,...`) is already what Door 1 returns and what vision models accept as an image source in a subsequent user message.
- `rmcp`'s `Content::image` requires the base64 bytes and a mime-type separately; extracting those from the data URL adds parsing complexity for no benefit — the MCP client can pass the data URL string directly to a vision model.
- Keeps Door 1 and Door 2 result formats identical, simplifying future tests.

If a client specifically needs raw `Content::image`, that can be added as a follow-up without breaking existing clients (additive change).

---

## 5. `spawn_blocking` Migration — Exact Diff Pattern

In every tool module that currently uses `tauri::async_runtime::spawn_blocking`, replace:

```rust
// Before
tauri::async_runtime::spawn_blocking(|| { ... }).await
```

```rust
// After
tokio::task::spawn_blocking(|| { ... }).await
```

Remove the `tauri` import from any module where it was the only usage. No logic changes. The Tauri GUI app continues to work because its runtime is tokio; `tokio::task::spawn_blocking` dispatches to the same blocking thread pool.

Files to audit and patch (check each for `spawn_blocking`):
- `src-tauri/src/tools/mod.rs`
- `src-tauri/src/tools/capture.rs`
- `src-tauri/src/tools/windows_info.rs`
- `src-tauri/src/tools/clipboard.rs`
- `src-tauri/src/tools/speak.rs`

---

## 6. Unit Tests

Add to `src-tauri/src/tools/mod.rs` (or a new `src-tauri/src/bin/mcp_tests.rs` — prefer `mod.rs` to keep tests co-located with the dispatch table):

```rust
#[cfg(test)]
mod mcp_tests {
    use super::*;

    /// Gate: all_schemas() returns exactly 6 tools with the expected names.
    #[test]
    fn schema_listing_has_six_tools() {
        let schemas = all_schemas();
        assert_eq!(schemas.len(), 6, "expected 6 tools, got {}", schemas.len());
        let names: Vec<&str> = schemas.iter().map(|t| t.name.as_str()).collect();
        for expected in &[
            "capture_screen",
            "list_windows",
            "get_active_window",
            "read_clipboard",
            "get_datetime",
            "speak",
        ] {
            assert!(
                names.contains(expected),
                "missing tool: {}; have: {:?}",
                expected,
                names
            );
        }
    }

    /// Gate: dispatch("get_datetime", {}) returns a non-empty string.
    /// (Passthrough test — verifies dispatch table routes correctly.)
    #[tokio::test]
    async fn dispatch_get_datetime_passthrough() {
        let result = dispatch("get_datetime", &serde_json::json!({}))
            .await
            .expect("dispatch should not error");
        assert!(!result.output.is_empty(), "datetime output should not be empty");
        // Sanity: should look like an ISO 8601 datetime
        assert!(
            result.output.contains('-') && result.output.contains(':'),
            "unexpected datetime format: {}",
            result.output
        );
    }

    /// Gate: dispatch("read_clipboard", {}) does not panic (clipboard may be empty).
    #[tokio::test]
    async fn dispatch_read_clipboard_does_not_panic() {
        // We don't assert content because clipboard state is test-environment-dependent.
        let _ = dispatch("read_clipboard", &serde_json::json!({})).await;
    }
}
```

These three tests run under `cargo test` (no `#[ignore]`). They join the existing 40 tests for a new total of 43.

---

## 7. Manual stdin JSON-RPC Probe

Run this in PowerShell after `cargo build --bin thuki-mcp`:

```powershell
# Build first (debug)
cargo build --bin thuki-mcp 2>&1

# Compose the three JSON-RPC messages as a single string piped to stdin.
# The server reads newline-delimited JSON from stdin; each line is one message.
$msgs = @(
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"0.1"}}}',
    '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}',
    '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}',
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_datetime","arguments":{}}}'
) -join "`n"

$msgs | .\target\debug\thuki-mcp.exe
```

**Expected stdout (abbreviated, order may vary per line):**
```
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{...},"serverInfo":{"name":"thuki-mcp","version":"0.1.0"}}}
{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"capture_screen",...},{"name":"list_windows",...},{"name":"get_active_window",...},{"name":"read_clipboard",...},{"name":"get_datetime",...},{"name":"speak",...}]}}
{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"2026-09-19T..."}],"isError":false}}
```

`eprintln!` breadcrumbs appear on **stderr** and do not interfere with stdout parsing.

---

## 8. Client Configuration

### 8a. Claude Desktop

File: `%AppData%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "thuki": {
      "command": "C:\src\jini\src-tauri\target\release\jini-mcp.exe",
      "args": [],
      "env": {}
    }
  }
}
```

For dev builds, replace `release` with `debug`. Restart Claude Desktop after editing.

### 8b. ZCode

In ZCode's settings (exact file path: `%AppData%\ZCode\settings.json` or the path ZCode uses for context servers — confirm in ZCode docs if different):

```json
{
  "context_servers": {
    "thuki": {
      "command": "C:\src\jini\src-tauri\target\release\jini-mcp.exe",
      "args": [],
      "transport": "stdio"
    }
  }
}
```

Again, swap `release` → `debug` during development.

---

## 9. `ServerHandler` Metadata (name/version)

rmcp's `#[tool_handler]` macro generates a default `ServerHandler` impl. To override the server name/version returned in the `initialize` response, add an explicit `get_info` override:

```rust
#[tool_handler]
impl ServerHandler for ThukiMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            name: "thuki-mcp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        }
    }
}
```

If `ServerInfo` does not implement `Default`, construct it fully from the fields rmcp exposes. Confirm field names at compile time; adjust if the macro signature differs.

---

## 10. Ordered Build Steps with Gates

| # | Step | Gate (must pass before next step) |
|---|------|----------------------------------|
| 1 | `spawn_blocking` migration: replace `tauri::async_runtime::spawn_blocking` → `tokio::task::spawn_blocking` in all tools modules | `cargo check --lib` clean |
| 2 | Add rmcp 3.4.0 + `[[bin]]` to `Cargo.toml` | `cargo check` clean (no dep conflicts); if reqwest conflict appears, bump reqwest to 0.13 and fix call sites in `client.rs` |
| 3 | Write `src-tauri/src/bin/mcp.rs` (Section 4 above) | `cargo check --bin thuki-mcp` clean |
| 4 | Add unit tests (Section 6) | `cargo test` — all 43 tests pass (40 existing + 3 new) |
| 5 | `cargo build --bin thuki-mcp` (debug) | Build succeeds, `target/debug/thuki-mcp.exe` exists |
| 6 | Run PowerShell probe (Section 7) | 6 tools in `tools/list` response; `get_datetime` returns valid ISO string |
| 7 | `cargo build --release --bin thuki-mcp` | Release exe exists at `target/release/thuki-mcp.exe` |
| 8 | Configure Claude Desktop (Section 8a) with debug path, restart | Thuki tools appear in Claude Desktop's tool panel |
| 9 | Configure ZCode (Section 8b) with debug path, restart | Thuki tools appear as context server tools in ZCode |
| 10 | `tsc` + `vite build` (frontend unchanged, sanity check) | No errors |

---

## 11. Acceptance Tests

| ID | Test | Pass Criterion |
|----|------|----------------|
| AT-B1 | `cargo test` | 43/43 green |
| AT-B2 | PowerShell probe: `tools/list` | Response contains exactly 6 tools with names matching Door 1 schema names |
| AT-B3 | PowerShell probe: `tools/call get_datetime` | `content[0].text` is a non-empty ISO 8601 datetime string |
| AT-B4 | PowerShell probe: `tools/call list_windows` | `content[0].text` is valid JSON array with at least 1 window entry |
| AT-B5 | Claude Desktop: open new conversation | Thuki tools listed in tool panel; calling `get_datetime` returns current time |
| AT-B6 | Claude Desktop: call `capture_screen` | Response contains a data URL string beginning with `data:image/png;base64,` |
| AT-B7 | ZCode: call `read_clipboard` | Returns clipboard text (pre-copy some text to clipboard before testing) |
| AT-B8 | Tauri GUI still works | Launch Thuki app → send a message with tools enabled → agent loop uses `list_windows` → ToolCard appears; no regression from `spawn_blocking` migration |

---

## 12. Out of Scope

- HTTP/SSE transport — Phase 2.3c
- OAuth / Bearer token authentication
- Action tools (`click`, `type`, `run_command`, etc.) — Phase 2.6
- Tunnel / ngrok / remote access
- MCP resource or prompt primitives (tools only)
- Windows installer / PATH registration of `thuki-mcp.exe`
- Auto-restart of MCP server on crash (client responsibility)

---

## 13. Risk Notes

1. **rmcp `#[tool]` macro parameter binding**: The `#[tool(aggr)]` attribute aggregates all JSON fields into the struct. If rmcp 3.4.0's macro uses a different attribute name (e.g., `#[tool(param)]`), adjust. Confirm with `cargo doc --open -p rmcp` or the crate's examples.
2. **`ServerInfo::default()`**: If `ServerInfo` lacks a `Default` impl, construct all fields explicitly. Check at compile time.
3. **reqwest conflict**: Unlikely (rmcp server+transport-io does not use HTTP client) but verify with `cargo tree` at Step 2. Bumping to reqwest 0.13 is straightforward if needed.
4. **ZCode settings path**: The path `%AppData%\ZCode\settings.json` is an assumption. Confirm the exact path from ZCode's documentation or UI before AT-B7.
5. **Clipboard test environment**: `dispatch_read_clipboard_does_not_panic` intentionally does not assert content, because CI/test runners may have empty clipboards. This is by design.
--- end report ---

result: <temp>
esult.json
