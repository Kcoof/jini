//! Tool registry for the agent loop (specs/phase-2.3). Tools are written
//! once here; both doors use them — the built-in chat (OpenAI tool calls)
//! now, the MCP server later (phase 2.3b/c).

pub mod actions;
pub mod capture;
pub mod clipboard;
pub mod datetime;
pub mod speak;
pub mod windows_info;

use serde_json::Value;

pub type ToolResult = Result<String, String>;

/// The `tools` array sent with every chat request while tools are on.
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

/// Dispatch a tool call by name. `args_json` is the fully assembled JSON
/// string of the arguments object (may be "{}" for no-arg tools). Returns
/// the result string for the role:"tool" message content.
pub async fn dispatch(name: &str, args_json: &str) -> ToolResult {
    let args: Value =
        serde_json::from_str(args_json).unwrap_or(Value::Object(serde_json::Map::new()));
    match name {
        "capture_screen" => capture::run(&args).await,
        "list_windows" => windows_info::run_list(&args).await,
        "get_active_window" => windows_info::run_active(&args).await,
        "read_clipboard" => clipboard::run(&args).await,
        "get_datetime" => datetime::run(&args).await,
        "speak" => speak::run(&args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod mcp_tests {
    use super::*;

    /// The tools/list surface: exactly 6 tools with Door-1 names
    /// (specs/phase-2.3b §6).
    #[test]
    fn schema_listing_has_six_tools() {
        let schemas = all_schemas();
        assert_eq!(schemas.len(), 6, "expected 6 tools, got {}", schemas.len());
        let names: Vec<String> = schemas
            .iter()
            .filter_map(|t| t.pointer("/function/name").and_then(|v| v.as_str().map(String::from)))
            .collect();
        for expected in [
            "capture_screen",
            "list_windows",
            "get_active_window",
            "read_clipboard",
            "get_datetime",
            "speak",
        ] {
            assert!(names.iter().any(|n| n == expected), "missing tool: {expected}; have: {names:?}");
        }
    }

    /// Dispatch passthrough — the MCP door reuses the same routing.
    #[tokio::test]
    async fn dispatch_get_datetime_passthrough() {
        let result = dispatch("get_datetime", "{}").await.expect("dispatch should not error");
        assert!(!result.is_empty());
        assert!(result.contains('-') && result.contains(':'), "unexpected datetime: {result}");
    }

    /// Clipboard content is environment-dependent; must not panic either way.
    #[tokio::test]
    async fn dispatch_read_clipboard_does_not_panic() {
        let _ = dispatch("read_clipboard", "{}").await;
    }
}
