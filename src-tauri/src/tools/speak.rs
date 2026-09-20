//! speak tool (specs/phase-2.3 §7.5, refactored in 2.4) — delegates to the
//! shared SAPI implementation in speech.rs.

use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
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
    })
}

pub async fn run(args: &serde_json::Value) -> super::ToolResult {
    let text: String = args["text"].as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return Err("speak: text must not be empty".into());
    }
    tokio::task::spawn_blocking(move || crate::speech::sapi_speak(&text))
        .await
        .map_err(|e| format!("speak task: {e}"))??;
    Ok("Speaking.".into())
}
