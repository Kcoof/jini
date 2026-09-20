//! The one OpenAI-compatible chat client (specs/phase-1 + 2.1). Message
//! content is plain text, or text + one base64 image part for vision —
//! same endpoint, same connection.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
    /// Assistant message that requested tools: the OpenAI tool_calls array.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<serde_json::Value>,
    /// role:"tool" result message: which call this answers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
    /// role:"tool" result message: tool name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

pub fn text_message(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: serde_json::Value::String(content.into()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

/// A single tool-call fragment arriving in one SSE chunk. Arguments arrive
/// as partial JSON strings split across many chunks (specs/phase-2.3 §5.1).
#[derive(Debug, Clone)]
pub struct ToolCallDelta {
    /// Which parallel call this belongs to (0-based).
    pub index: usize,
    /// Present only in the first chunk for that index.
    pub id: Option<String>,
    /// Present only in the first chunk for that index.
    pub name: Option<String>,
    /// Partial JSON fragment; may be empty "".
    pub arguments: String,
}

/// What a single SSE data line means after parsing.
#[derive(Debug, Clone)]
pub enum Delta {
    /// Normal text token — forward to the UI.
    Content(String),
    /// One fragment of a tool call (may span many SSE lines).
    ToolCall(ToolCallDelta),
    /// Stream finished. Carries the finish_reason.
    Done(FinishReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    Other(String),
}

pub fn join_chat_url(base: &str) -> String {
    format!("{}/chat/completions", base.trim().trim_end_matches('/'))
}

/// Wrap a user message into OpenAI-compatible vision parts (text + data
/// URL image). A message that already carries parts gets the image part
/// appended instead of being nested inside `"text"`.
fn attach_image(messages: &mut Vec<ChatMessage>, image_base64: &str) {
    let data_url = format!("data:image/png;base64,{image_base64}");
    let image_part = serde_json::json!({
        "type": "image_url",
        "image_url": { "url": data_url },
    });
    if let Some(last) = messages.last_mut() {
        if last.role == "user" {
            match &last.content {
                serde_json::Value::String(text) => {
                    let text_part = serde_json::json!({
                        "type": "text",
                        "text": text.clone(),
                    });
                    last.content = serde_json::Value::Array(vec![text_part, image_part]);
                }
                serde_json::Value::Array(parts) => {
                    let mut parts = parts.clone();
                    parts.push(image_part);
                    last.content = serde_json::Value::Array(parts);
                }
                _ => {}
            }
            return;
        }
    }
    messages.push(ChatMessage {
        role: "user".into(),
        content: serde_json::Value::Array(vec![image_part]),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });
}

pub fn build_request(
    model: &str,
    mut messages: Vec<ChatMessage>,
    clipboard_context: Option<&str>,
    image_base64: Option<&str>,
    tools: Option<Vec<serde_json::Value>>,
    tool_choice: Option<serde_json::Value>,
) -> ChatRequest {
    if let Some(ctx) = clipboard_context.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(last) = messages.last_mut() {
            if last.role == "user" {
                if let serde_json::Value::String(text) = &last.content {
                    last.content =
                        serde_json::Value::String(format!("{text}\n\nClipboard context:\n{ctx}"));
                } else {
                    messages.push(text_message("user", &format!("Clipboard context:\n{ctx}")));
                }
            } else {
                messages.push(text_message("user", &format!("Clipboard context:\n{ctx}")));
            }
        }
    }
    if let Some(image) = image_base64.map(str::trim).filter(|s| !s.is_empty()) {
        attach_image(&mut messages, image);
    }
    ChatRequest {
        model: model.to_string(),
        messages,
        stream: true,
        tools,
        tool_choice,
    }
}

/// Content delta from an SSE payload; falls back to reasoning_content for
/// models that stream thinking first. `[DONE]` and empty lines → None.
/// Kept for callers that only care about text (provider test); the agent
/// loop uses `parse_delta` (specs/phase-2.3 §5.2).
pub fn extract_delta(payload: &str) -> Option<String> {
    match parse_delta(payload) {
        Some(Delta::Content(text)) => Some(text),
        _ => None,
    }
}

/// Parse one SSE data line into a Delta. Order matters: finish_reason is
/// checked BEFORE tool_calls fragments — some providers send both on the
/// same line and Done(ToolCalls) must win (the argument buffer already
/// holds everything from prior lines).
pub fn parse_delta(payload: &str) -> Option<Delta> {
    let data = payload.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    // anymodel quirk: unfinished chunks carry finish_reason as EMPTY STRING,
    // not null — treat "" as absent or every chunk reads as Done(Other("")).
    if let Some(reason) = value
        .pointer("/choices/0/finish_reason")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(Delta::Done(match reason {
            "stop" => FinishReason::Stop,
            "tool_calls" => FinishReason::ToolCalls,
            "length" => FinishReason::Length,
            other => FinishReason::Other(other.to_string()),
        }));
    }
    if let Some(calls) = value.pointer("/choices/0/delta/tool_calls").and_then(|v| v.as_array()) {
        let first = calls.first()?;
        let index = first.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        // Same quirk: later fragments repeat fields as "" — only real values
        // count, or the buffered name/id gets overwritten with "".
        let id = first
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let name = first
            .pointer("/function/name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let arguments = first
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(Delta::ToolCall(ToolCallDelta {
            index,
            id,
            name,
            arguments,
        }));
    }
    let content = value
        .pointer("/choices/0/delta/content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    if let Some(text) = content {
        return Some(Delta::Content(text.to_string()));
    }
    value
        .pointer("/choices/0/delta/reasoning_content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| Delta::Content(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::presets::preset;
    use crate::api::presets::PresetId;

    fn content_str(m: &ChatMessage) -> String {
        m.content.as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn glm_url_join() {
        assert_eq!(
            join_chat_url(preset(PresetId::Glm).base_url),
            "https://api.z.ai/api/coding/paas/v4/chat/completions"
        );
    }

    #[test]
    fn anymodel_url_join() {
        assert_eq!(
            join_chat_url(preset(PresetId::Anymodel).base_url),
            "https://anymodel.org/v1/chat/completions"
        );
    }

    #[test]
    fn trailing_slash_custom() {
        assert_eq!(
            join_chat_url("https://example.com/v1/"),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn request_sets_stream_and_order() {
        let req = build_request(
            "m",
            vec![
                text_message("user", "Hi"),
                text_message("assistant", "Hello"),
                text_message("user", "Next"),
            ],
            None,
            None,
            None,
            None,
        );
        assert!(req.stream);
        assert_eq!(req.messages.len(), 3);
        assert_eq!(content_str(&req.messages[0]), "Hi");
        assert_eq!(content_str(&req.messages[2]), "Next");
    }

    #[test]
    fn clipboard_appended_when_set() {
        let req = build_request(
            "m",
            vec![text_message("user", "Summarize")],
            Some("copied text"),
            None,
            None,
            None,
        );
        let text = content_str(&req.messages[0]);
        assert!(text.contains("copied text"));
        assert!(text.contains("Summarize"));
    }

    #[test]
    fn clipboard_skipped_when_blank() {
        let req = build_request(
            "m",
            vec![text_message("user", "Summarize")],
            Some("   "),
            None,
            None,
            None,
        );
        assert_eq!(content_str(&req.messages[0]), "Summarize");
    }

    #[test]
    fn image_wraps_last_user_message_into_parts() {
        let req = build_request(
            "vision-model",
            vec![text_message("user", "What is in this picture?")],
            None,
            Some("aGk="),
            None,
            None,
        );
        let parts = req.messages[0].content.as_array().expect("parts array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "What is in this picture?");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,aGk=");
    }

    #[test]
    fn image_ignored_when_blank() {
        let req = build_request(
            "m",
            vec![text_message("user", "Hi")],
            None,
            Some("   "),
            None,
            None,
        );
        assert_eq!(content_str(&req.messages[0]), "Hi");
    }

    #[test]
    fn delta_extraction() {
        assert_eq!(
            extract_delta(r#"{"choices":[{"delta":{"content":"he"}}]}"#),
            Some("he".to_string())
        );
        assert_eq!(
            extract_delta(r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#),
            Some("think".to_string())
        );
        assert_eq!(extract_delta("[DONE]"), None);
        assert_eq!(extract_delta(""), None);
        assert_eq!(extract_delta(r#"{"choices":[{"delta":{}}]}"#), None);
    }
}

#[cfg(test)]
mod parse_delta_tests {
    use super::*;

    fn tool_call_chunk(index: usize, id: Option<&str>, name: Option<&str>, args: &str) -> String {
        let mut tc = serde_json::json!({
            "index": index,
            "type": "function",
            "function": { "arguments": args }
        });
        if let Some(i) = id {
            tc["id"] = serde_json::json!(i);
        }
        if let Some(n) = name {
            tc["function"]["name"] = serde_json::json!(n);
        }
        serde_json::json!({ "choices": [{ "delta": { "tool_calls": [tc] } }] }).to_string()
    }

    #[test]
    fn content_delta() {
        let d = parse_delta(r#"{"choices":[{"delta":{"content":"hello"}}]}"#);
        assert!(matches!(d, Some(Delta::Content(t)) if t == "hello"));
    }

    #[test]
    fn reasoning_content_falls_back() {
        let d = parse_delta(r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#);
        assert!(matches!(d, Some(Delta::Content(t)) if t == "think"));
    }

    #[test]
    fn done_stop() {
        let d = parse_delta(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
        assert!(matches!(d, Some(Delta::Done(FinishReason::Stop))));
    }

    #[test]
    fn done_tool_calls() {
        let d = parse_delta(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        assert!(matches!(d, Some(Delta::Done(FinishReason::ToolCalls))));
    }

    #[test]
    fn done_with_simultaneous_tool_call_fragment() {
        // Some providers send finish_reason and tool_calls on the same line.
        // finish_reason must win (checked first).
        let d = parse_delta(
            r#"{
            "choices":[{
                "delta":{"tool_calls":[{"index":0,"function":{"arguments":""}}]},
                "finish_reason":"tool_calls"
            }]
        }"#,
        );
        assert!(matches!(d, Some(Delta::Done(FinishReason::ToolCalls))));
    }

    #[test]
    fn empty_and_done_sentinel() {
        assert!(parse_delta("").is_none());
        assert!(parse_delta("[DONE]").is_none());
        assert!(parse_delta(r#"{"choices":[{"delta":{}}]}"#).is_none());
    }

    #[test]
    fn tool_call_first_chunk_has_id_and_name() {
        let raw = tool_call_chunk(0, Some("call_abc"), Some("list_windows"), "");
        let d = parse_delta(&raw);
        let Some(Delta::ToolCall(tc)) = d else {
            panic!("expected ToolCall")
        };
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id.as_deref(), Some("call_abc"));
        assert_eq!(tc.name.as_deref(), Some("list_windows"));
        assert_eq!(tc.arguments, "");
    }

    #[test]
    fn tool_call_subsequent_chunks_no_id_name() {
        let raw = tool_call_chunk(0, None, None, r#"{"re"#);
        let d = parse_delta(&raw);
        let Some(Delta::ToolCall(tc)) = d else {
            panic!("expected ToolCall")
        };
        assert!(tc.id.is_none());
        assert!(tc.name.is_none());
        assert_eq!(tc.arguments, r#"{"re"#);
    }

    #[test]
    fn chunked_5kb_arguments_reassemble() {
        // A 5KB arguments string delivered in ~256-byte chunks must
        // reassemble exactly (the agent loop joins these fragments).
        let filler = "x".repeat(5000);
        let full_args = format!(r#"{{"query":"{filler}"}}"#);
        let chunk_size = 256;
        let mut assembled = String::new();
        let bytes = full_args.as_bytes();
        let first = tool_call_chunk(0, Some("call_big"), Some("some_tool"), &full_args[..chunk_size]);
        match parse_delta(&first) {
            Some(Delta::ToolCall(tc)) => {
                assert_eq!(tc.id.as_deref(), Some("call_big"));
                assembled.push_str(&tc.arguments);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        let mut offset = chunk_size;
        while offset < bytes.len() {
            let end = (offset + chunk_size).min(bytes.len());
            let chunk = tool_call_chunk(0, None, None, &full_args[offset..end]);
            match parse_delta(&chunk) {
                Some(Delta::ToolCall(tc)) => assembled.push_str(&tc.arguments),
                other => panic!("expected ToolCall at offset {offset}: {other:?}"),
            }
            offset = end;
        }
        assert_eq!(assembled, full_args);
    }

    #[test]
    fn tool_calls_and_content_in_same_stream() {
        let content_line = r#"{"choices":[{"delta":{"content":"Let me check "}}]}"#;
        let tc_line = tool_call_chunk(0, Some("call_1"), Some("get_datetime"), "{}");
        let done_line = r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#;

        assert!(matches!(parse_delta(content_line), Some(Delta::Content(_))));
        assert!(matches!(parse_delta(&tc_line), Some(Delta::ToolCall(_))));
        assert!(matches!(
            parse_delta(done_line),
            Some(Delta::Done(FinishReason::ToolCalls))
        ));
    }

    #[test]
    fn no_tools_stream_unchanged_behavior() {
        let lines = [
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{"content":"!"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ];
        let deltas: Vec<_> = lines.iter().filter_map(|l| parse_delta(l)).collect();
        assert!(matches!(&deltas[0], Delta::Content(t) if t == "Hi"));
        assert!(matches!(&deltas[1], Delta::Content(t) if t == "!"));
        assert!(matches!(&deltas[2], Delta::Done(FinishReason::Stop)));
    }

    #[test]
    fn parallel_tool_calls_two_indices() {
        let c0 = tool_call_chunk(0, Some("call_0"), Some("get_datetime"), "{}");
        let c1 = tool_call_chunk(1, Some("call_1"), Some("list_windows"), "{}");
        let Some(Delta::ToolCall(tc0)) = parse_delta(&c0) else {
            panic!()
        };
        let Some(Delta::ToolCall(tc1)) = parse_delta(&c1) else {
            panic!()
        };
        assert_eq!(tc0.index, 0);
        assert_eq!(tc1.index, 1);
        assert_ne!(tc0.id, tc1.id);
    }

    // anymodel/gpt-5.6-sol wire quirks found by the live probe (2026-09-19):
    // unfinished chunks carry finish_reason:"" and later fragments repeat
    // id/name as "" — both must read as absent, not as values.

    #[test]
    fn empty_finish_reason_is_ignored() {
        let raw = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"id":"call_1","type":"function","function":{"name":"list_windows","arguments":""},"index":0}]},"finish_reason":""}]}"#;
        assert!(matches!(parse_delta(raw), Some(Delta::ToolCall(_))));
    }

    #[test]
    fn empty_name_fragment_does_not_count() {
        let raw = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"function":{"name":"","arguments":"{}"},"index":0}]}}]}"#;
        let Some(Delta::ToolCall(tc)) = parse_delta(raw) else {
            panic!()
        };
        assert!(tc.name.is_none());
        assert_eq!(tc.arguments, "{}");
    }

    #[test]
    fn anymodel_full_sequence_parses() {
        // The exact 3-chunk sequence the probe captured: id+name fragment,
        // arguments fragment, then a bare finish_reason:"tool_calls" chunk.
        let c1 = r#"{"choices":[{"index":0,"delta":{"content":"","reasoning_content":"","tool_calls":[{"id":"call_1Lrm","type":"function","function":{"name":"list_windows","arguments":""},"index":0}]},"logprobs":null,"finish_reason":""}]}"#;
        let c2 = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"function":{"name":"","arguments":"{}"},"index":0}]},"logprobs":null,"finish_reason":""}]}"#;
        let c3 = r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""},"logprobs":null,"finish_reason":"tool_calls"}]}"#;
        assert!(matches!(parse_delta(c1), Some(Delta::ToolCall(_))));
        assert!(matches!(parse_delta(c2), Some(Delta::ToolCall(_))));
        assert!(matches!(
            parse_delta(c3),
            Some(Delta::Done(FinishReason::ToolCalls))
        ));
    }
}
