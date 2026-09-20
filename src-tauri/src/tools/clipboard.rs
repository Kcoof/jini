//! read_clipboard tool (specs/phase-2.3 §7.3) — wraps the existing
//! clipboard module, read-only.

use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "read_clipboard",
            "description": "Read the current clipboard text. Returns empty string if clipboard holds non-text data.",
            "parameters": { "type": "object", "properties": {}, "required": [] }
        }
    })
}

pub async fn run(_args: &serde_json::Value) -> super::ToolResult {
    Ok(tokio::task::spawn_blocking(|| crate::clipboard::read_text().unwrap_or_default())
        .await
        .map_err(|e| e.to_string())?)
}
