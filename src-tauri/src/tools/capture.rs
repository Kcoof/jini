//! capture_screen tool (specs/phase-2.3 §7.1) — delegates to the existing
//! capture pipeline; no duplication. Returns a data URL so vision models
//! can read it; the OpenAI tool-result wire format only allows a string.

use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
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
    })
}

#[derive(Debug, serde::Deserialize)]
struct RegionArg {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

pub async fn run(args: &serde_json::Value) -> super::ToolResult {
    let region: Option<RegionArg> = args
        .get("region")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let full = crate::capture::capture_display()?;
        let img = match region {
            Some(r) if r.w > 0 && r.h > 0 => {
                crate::capture::crop(&full, r.x.max(0) as u32, r.y.max(0) as u32, r.w, r.h)
            }
            _ => full,
        };
        let b64 = crate::capture::base64_png(&img)?;
        // Same quiet side-effects as the tray camera: saved + on clipboard.
        let _ = crate::capture::save_png(&img);
        let _ = crate::capture::copy_to_clipboard(&img);
        Ok(format!("data:image/png;base64,{b64}"))
    })
    .await
    .map_err(|e| format!("capture task failed: {e}"))??;
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn capture_returns_data_url() {
        let result = super::run(&serde_json::Value::Null).await.unwrap();
        assert!(result.starts_with("data:image/png;base64,"));
        assert!(result.len() > 100);
    }
}
