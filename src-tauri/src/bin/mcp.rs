//! jini-mcp: MCP stdio server exposing Thuki's 6 read-only tools
//! (specs/phase-2.3b). Launched by Claude Desktop / ZCode; runs standalone
//! with its own tokio runtime — no Tauri app, no AppState, no DB.
//! All logs go to stderr so stdout stays clean JSON-RPC.

use rmcp::{
    handler::server::wrapper::Parameters,
    tool_handler,
    model::{CallToolResult, ContentBlock},
    schemars,
    tool, tool_router,
    transport::io::stdio,
    ErrorData,
};
use serde::Deserialize;
use jini_lib::tools;

// Parameter structs (schemars generates the JSON Schema for each tool).

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CaptureScreenParams {
    /// Optional sub-region to capture: [x, y, width, height] in physical
    /// pixels. Omit for the full primary monitor.
    region: Option<[i32; 4]>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SpeakParams {
    /// Text for the SAPI voice to speak aloud (fire-and-forget, async).
    text: String,
}

#[derive(Clone)]
struct JiniMcpServer;

/// Run one tool by name and wrap its string result as MCP text content.
async fn run_tool(name: &str, args: serde_json::Value) -> Result<CallToolResult, ErrorData> {
    let output = tools::dispatch(name, &args.to_string())
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(output)]))
}

#[tool_router]
impl JiniMcpServer {
    #[tool(description = "Capture a screenshot of the primary monitor or a sub-region. Returns a PNG data URL string. Also saves to Pictures\\Thuki and copies to clipboard.")]
    async fn capture_screen(
        &self,
        Parameters(CaptureScreenParams { region }): Parameters<CaptureScreenParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let args = match region {
            Some([x, y, w, h]) => serde_json::json!({
                "region": { "x": x, "y": y, "w": w.max(0), "h": h.max(0) }
            }),
            None => serde_json::json!({}),
        };
        run_tool("capture_screen", args).await
    }

    #[tool(description = "List all visible top-level windows with their title, process name, and PID, excluding Jini itself.")]
    async fn list_windows(&self) -> Result<CallToolResult, ErrorData> {
        run_tool("list_windows", serde_json::json!({})).await
    }

    #[tool(description = "Get the title and process name of the currently focused (active) window.")]
    async fn get_active_window(&self) -> Result<CallToolResult, ErrorData> {
        run_tool("get_active_window", serde_json::json!({})).await
    }

    #[tool(description = "Read the current text content of the Windows clipboard.")]
    async fn read_clipboard(&self) -> Result<CallToolResult, ErrorData> {
        run_tool("read_clipboard", serde_json::json!({})).await
    }

    #[tool(description = "Get the current local date and time in ISO 8601 format.")]
    async fn get_datetime(&self) -> Result<CallToolResult, ErrorData> {
        run_tool("get_datetime", serde_json::json!({})).await
    }

    #[tool(description = "Speak text aloud using Windows SAPI text-to-speech. Returns immediately; speech plays in background.")]
    async fn speak(
        &self,
        Parameters(SpeakParams { text }): Parameters<SpeakParams>,
    ) -> Result<CallToolResult, ErrorData> {
        run_tool("speak", serde_json::json!({ "text": text })).await
    }
}

#[tool_handler(name = "jini", version = "0.1.0")]
impl rmcp::handler::server::ServerHandler for JiniMcpServer {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[jini-mcp] starting stdio server");
    let service = rmcp::service::serve_server(JiniMcpServer, stdio()).await?;
    service.waiting().await?;
    eprintln!("[jini-mcp] server shut down");
    Ok(())
}
