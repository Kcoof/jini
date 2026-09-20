//! launch_app action (specs/phase-2.6 §5f) — guarded .exe spawn.

use serde_json::{json, Value};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "function": {
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
        }
    })
}

pub async fn run(args: &Value) -> super::ToolResult {
    let path = args["path"].as_str().ok_or("launch_app: missing path")?;
    super::guard::check_path(path)?;
    if !path.to_lowercase().ends_with(".exe") {
        return Err("launch_app: path must point to an .exe file".into());
    }

    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(&path)
            .spawn()
            .map(|_| format!("Launched: {path}"))
            .map_err(|e| format!("launch_app: {e}"))
    })
    .await
    .map_err(|e| format!("launch_app task: {e}"))?
}
