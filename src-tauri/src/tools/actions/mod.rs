//! Action tools with per-action YES/NO confirmation (specs/phase-2.6).
//! Registered SEPARATELY from the read-only tools: `all_schemas()` (used
//! by the built-in chat when actions are off, and by the MCP door) stays
//! read-only; `all_action_schemas()` only joins the chat request when the
//! actions_enabled setting is on (default off — opt-in).

pub mod guard;
pub mod input_synthesis;
pub mod launch_app;
pub mod open_path;
pub mod uia_find;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, watch};

use crate::AppState;

pub type ToolResult = Result<String, String>;

/// Every tool that requires a YES/NO confirmation before executing.
pub fn is_action(name: &str) -> bool {
    matches!(
        name,
        "click_at" | "type_text" | "press_keys" | "launch_app" | "open_path" | "find_elements"
    )
}

/// Schemas of the action tools (only offered when actions_enabled).
pub fn all_action_schemas() -> Vec<Value> {
    vec![
        uia_find::schema(),
        input_synthesis::click_at_schema(),
        input_synthesis::type_text_schema(),
        input_synthesis::press_keys_schema(),
        launch_app::schema(),
        open_path::schema(),
    ]
}

/// Execute an action tool directly (no confirmation — used post-approval
/// and by tests).
pub async fn dispatch(name: &str, args_json: &str) -> ToolResult {
    let args: Value = serde_json::from_str(args_json).unwrap_or(Value::Object(Default::default()));
    match name {
        "click_at" => input_synthesis::click_at(&args).await,
        "type_text" => input_synthesis::type_text(&args).await,
        "press_keys" => input_synthesis::press_keys(&args).await,
        "launch_app" => launch_app::run(&args).await,
        "open_path" => open_path::run(&args).await,
        "find_elements" => uia_find::run(&args).await,
        other => Err(format!("Unknown action: {other}")),
    }
}

/// Human-readable one-liner for the confirmation card (specs §3).
pub fn describe_action(name: &str, args: &Value) -> String {
    match name {
        "click_at" => {
            let x = args["x"].as_i64().unwrap_or(0);
            let y = args["y"].as_i64().unwrap_or(0);
            format!("Click at screen position ({x}, {y})")
        }
        "type_text" => {
            let text = args["text"].as_str().unwrap_or("");
            let shown: String = text.chars().take(40).collect();
            let ellipsis = if text.chars().count() > 40 { "…" } else { "" };
            format!("Type \"{shown}{ellipsis}\"")
        }
        "press_keys" => format!("Press keys: {}", args["keys"].as_str().unwrap_or("")),
        "launch_app" => {
            let path = args["path"].as_str().unwrap_or("");
            let name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            format!("Launch app: {name}")
        }
        "open_path" => format!("Open: {}", args["path"].as_str().unwrap_or("")),
        "find_elements" => {
            let query = args["query"].as_str().unwrap_or("(all)");
            format!("Find UI elements matching \"{query}\"")
        }
        other => format!("Run action: {other}"),
    }
}

/// Dispatch an action behind the YES/NO gate: emit chat://tool-pending,
/// suspend until the user answers (or 60s auto-decline / stream cancel),
/// then execute or decline (specs §1c/§1f).
pub async fn dispatch_with_confirm(
    name: &str,
    args_json: &str,
    call_id: &str,
    app: &AppHandle,
    state: &AppState,
    cancel: &mut watch::Receiver<bool>,
) -> ToolResult {
    let args: Value = serde_json::from_str(args_json).unwrap_or(Value::Object(Default::default()));
    let description = describe_action(name, &args);

    // Register the channel BEFORE emitting so YES can't race the listener.
    let (tx, rx) = oneshot::channel::<bool>();
    state
        .pending_confirmations
        .lock()
        .map_err(|e| e.to_string())?
        .insert(call_id.to_string(), tx);

    let _ = app.emit(
        "chat://tool-pending",
        json!({
            "call_id": call_id,
            "tool_name": name,
            "arguments": args_json,
            "description": description,
        }),
    );

    let approved = tokio::select! {
        result = tokio::time::timeout(tokio::time::Duration::from_secs(60), rx) => {
            result.unwrap_or(Ok(false)).unwrap_or(false)
        }
        _ = cancel.changed() => false, // stream cancelled → auto-decline
    };

    state
        .pending_confirmations
        .lock()
        .map_err(|e| e.to_string())?
        .remove(call_id);

    if !approved {
        return Err(format!("Action declined by user: {name}"));
    }
    dispatch(name, args_json).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_action_covers_all_six() {
        for name in [
            "click_at",
            "type_text",
            "press_keys",
            "launch_app",
            "open_path",
            "find_elements",
        ] {
            assert!(is_action(name), "{name} should be an action");
        }
        assert!(!is_action("list_windows"));
        assert!(!is_action("capture_screen"));
    }

    #[test]
    fn schemas_match_is_action_set() {
        let schemas = all_action_schemas();
        assert_eq!(schemas.len(), 6);
        for s in &schemas {
            let name = s.pointer("/function/name").and_then(|v| v.as_str()).unwrap();
            assert!(is_action(name), "{name} in schemas but not in is_action");
        }
    }

    #[test]
    fn descriptions_are_human_readable() {
        let d = describe_action(
            "click_at",
            &json!({ "x": 512, "y": 384 }),
        );
        assert_eq!(d, "Click at screen position (512, 384)");
        let d = describe_action("press_keys", &json!({ "keys": "Ctrl+S" }));
        assert_eq!(d, "Press keys: Ctrl+S");
    }
}
