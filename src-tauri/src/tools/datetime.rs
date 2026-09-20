//! get_datetime tool (specs/phase-2.3 §7.4).

use serde_json::json;

pub fn schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_datetime",
            "description": "Return the current local date, time, and timezone offset.",
            "parameters": { "type": "object", "properties": {}, "required": [] }
        }
    })
}

pub async fn run(_args: &serde_json::Value) -> super::ToolResult {
    use chrono::Local;
    let now = Local::now();
    Ok(json!({
        "date":     now.format("%Y-%m-%d").to_string(),
        "time":     now.format("%H:%M:%S").to_string(),
        "weekday":  now.format("%A").to_string(),
        "timezone": now.format("%z").to_string(),
        "iso8601":  now.to_rfc3339(),
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn datetime_returns_valid_json() {
        let result = super::run(&serde_json::Value::Null).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["date"].as_str().unwrap().len(), 10); // YYYY-MM-DD
        assert!(v["iso8601"].as_str().unwrap().contains('T'));
    }
}
