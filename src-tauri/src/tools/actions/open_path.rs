//! open_path action (specs/phase-2.6 §5a) — ShellExecute with the guard.

use serde_json::{json, Value};
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub fn schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "open_path",
            "description": "Open a file, folder, or URL with the system default app. Safe for documents and web URLs.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute file path, folder path, or URL (https://…)."
                    }
                },
                "required": ["path"]
            }
        }
    })
}

pub async fn run(args: &Value) -> super::ToolResult {
    let path = args["path"]
        .as_str()
        .ok_or("open_path: missing path")?
        .to_string();
    super::guard::check_path(&path)?;

    tokio::task::spawn_blocking(move || {
        unsafe {
            let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let verb_wide: Vec<u16> = "open\0".encode_utf16().collect();
            let result = ShellExecuteW(
                None,
                PCWSTR(verb_wide.as_ptr()),
                PCWSTR(path_wide.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
            if result.0 as isize > 32 {
                Ok(format!("Opened: {path}"))
            } else {
                Err(format!("ShellExecuteW failed with code: {}", result.0 as isize))
            }
        }
    })
    .await
    .map_err(|e| format!("open_path task: {e}"))?
}
