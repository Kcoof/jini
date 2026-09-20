//! Frontend-invokable commands (specs/phase-1/spec.md 1.1–1.5).

use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;
use uuid::Uuid;

use crate::api::client::{build_request, extract_delta, join_chat_url, text_message, ChatMessage};
use crate::api::presets::{preset, PresetId};
use crate::db::{self, ConversationRecord};
use crate::{hotkey, secrets, AppState};

/* ---------- settings ---------- */

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<db::Settings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::load_settings(&conn, secrets::key_map()?)
}

#[derive(Debug, Deserialize)]
pub struct SaveSettingsInput {
    pub active_preset: PresetId,
    pub model: String,
    pub hotkey: String,
    pub theme: String,
    pub base_url: Option<String>,
    /// Agent tools master switch; None → leave unchanged.
    pub tools_enabled: Option<bool>,
    /// Spoken replies mode: "always" | "auto" | "never"; None → unchanged.
    pub voice_replies_mode: Option<String>,
    /// BCP-47 speech language; None → unchanged.
    pub stt_language: Option<String>,
    /// Action tools master switch; None → unchanged (default off).
    pub actions_enabled: Option<bool>,
}

#[tauri::command]
pub fn save_settings(
    state: State<AppState>,
    settings: SaveSettingsInput,
) -> Result<db::Settings, String> {
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let current = db::load_settings(&conn, secrets::key_map()?)?;
        db::save_core_settings(
            &conn,
            settings.active_preset,
            &settings.model,
            &settings.hotkey,
            &settings.theme,
            settings.base_url.as_deref(),
            settings.tools_enabled.unwrap_or(current.tools_enabled),
            settings
                .voice_replies_mode
                .as_deref()
                .or(Some(current.voice_replies_mode.as_str())),
            settings
                .stt_language
                .as_deref()
                .or(Some(current.stt_language.as_str())),
            settings.actions_enabled,
        )?;
    }
    if let Some(on) = settings.tools_enabled {
        if let Ok(mut guard) = state.tools_enabled.lock() {
            *guard = on;
        }
    }
    if let Some(on) = settings.actions_enabled {
        if let Ok(mut guard) = state.actions_enabled.lock() {
            *guard = on;
        }
    }
    get_settings(state)
}

#[tauri::command]
pub fn set_api_key(preset_id: PresetId, api_key: String) -> Result<(), String> {
    secrets::set_api_key(preset_id, &api_key)
}

#[tauri::command]
pub fn has_api_key(preset_id: PresetId) -> Result<bool, String> {
    secrets::has_api_key(preset_id)
}

#[tauri::command]
pub fn delete_api_key(preset_id: PresetId) -> Result<(), String> {
    secrets::delete_api_key(preset_id)
}

#[tauri::command]
pub fn set_hotkey(app: AppHandle, hotkey: String) -> Result<(), String> {
    crate::hotkey::register(&app, &hotkey)?;
    let state = app.state::<AppState>();
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::set_value(&conn, "hotkey", &hotkey)?;
    *state.hotkey.lock().map_err(|e| e.to_string())? = hotkey;
    Ok(())
}

/* ---------- clipboard ---------- */

#[tauri::command]
pub fn get_clipboard() -> Result<Value, String> {
    Ok(json!({ "text": crate::clipboard::read_text() }))
}

/* ---------- provider test ---------- */

#[derive(Debug, Deserialize)]
pub struct TestProviderInput {
    pub preset_id: PresetId,
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// Key typed in Settings; used for this request only, never stored.
    pub api_key: Option<String>,
}

#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    input: TestProviderInput,
) -> Result<String, String> {
    let preset_id = input.preset_id;
    let (stored_base_url, stored_model) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let settings = db::load_settings(&conn, secrets::key_map()?)?;
        (
            settings.base_urls.get(preset_id.as_str()).cloned(),
            settings.models.get(preset_id.as_str()).cloned(),
        )
    };
    let base_url = input
        .base_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .or(stored_base_url)
        .unwrap_or_else(|| preset(preset_id).base_url.to_string());
    if base_url.is_empty() {
        return Err("Set an endpoint before testing.".into());
    }
    let model = input
        .model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .or(stored_model)
        .unwrap_or_else(|| preset(preset_id).default_model.to_string());
    if model.is_empty() {
        return Err("Set a model before testing.".into());
    }
    let api_key = match input.api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(key) => key,
        None => secrets::get_api_key(preset_id)?.ok_or_else(|| {
            "No key to test. Paste your key in the API key field, then press Test.".to_string()
        })?,
    };

    let request = build_request(
        &model,
        vec![text_message("user", "Reply with the single word: ok")],
        None,
        None,
        None,
        None,
    );
    let url = join_chat_url(&base_url);
    run_provider_test(&state.http, &url, &api_key, &request, &model).await
}

async fn run_provider_test(
    http: &reqwest::Client,
    url: &str,
    api_key: &str,
    request: &crate::api::client::ChatRequest,
    model: &str,
) -> Result<String, String> {
    let response = http
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Could not reach the endpoint: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status.as_u16(), &body));
    }
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Connection dropped during test: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = buffer.find('\n') {
            let line = buffer[..idx].trim_end_matches('\r').to_string();
            buffer.drain(..=idx);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            if let Some(delta) = extract_delta(data) {
                let snippet: String = delta.chars().take(60).collect();
                return Ok(format!("{model} replied: {snippet}"));
            }
        }
    }
    Err("Endpoint connected but streamed no text. Check the model name.".into())
}

/* ---------- conversations ---------- */

#[tauri::command]
pub fn get_conversations(state: State<AppState>) -> Result<Vec<db::ConversationSummary>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::list_conversations(&conn)
}

#[tauri::command]
pub fn get_conversation(state: State<AppState>, id: String) -> Result<ConversationRecord, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_conversation(&conn, &id)
}

#[tauri::command]
pub fn delete_conversation(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_conversation(&conn, &id)
}

#[tauri::command]
pub fn save_conversation(
    state: State<AppState>,
    conversation: ConversationRecord,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::upsert_conversation(&conn, &conversation)
}

/* ---------- chat ---------- */

#[derive(Debug, Deserialize)]
pub struct SendMessageInput {
    pub conversation_id: Option<String>,
    pub content: String,
    pub clipboard_context: Option<String>,
    pub preset_id: Option<PresetId>,
    pub model: Option<String>,
    /// Optional base64 PNG attached to this message (screenshot analysis).
    pub image_base64: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendMessageOutput {
    pub conversation_id: String,
}

#[tauri::command]
pub async fn cancel_message(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(tx) = state.cancel.lock().map_err(|e| e.to_string())?.as_ref() {
        let _ = tx.send(true);
    }
    let _ = app.emit(
        "chat://done",
        json!({ "conversation_id": "", "cancelled": true }),
    );
    Ok(())
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SendMessageInput,
) -> Result<SendMessageOutput, String> {
    let settings = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::load_settings(&conn, secrets::key_map()?)?
    };
    let preset_id = input.preset_id.unwrap_or(settings.active_preset);
    let model = input
        .model
        .filter(|m| !m.is_empty())
        .unwrap_or(settings.model.clone());
    let base_url = settings
        .base_urls
        .get(preset_id.as_str())
        .cloned()
        .unwrap_or_else(|| preset(preset_id).base_url.to_string());
    if base_url.trim().is_empty() {
        return Err("Set an endpoint in Settings before sending.".into());
    }
    let api_key = secrets::get_api_key(preset_id)?.ok_or_else(|| {
        "No key saved yet — open Settings, paste your API key, and click Save first.".to_string()
    })?;

    let conversation_id = input
        .conversation_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let created = db::stamp();
    let user_msg = json!({
        "id": Uuid::new_v4().to_string(),
        "role": "user",
        "content": input.content,
        "created_at": created,
        "clipboard_context": input.clipboard_context,
        "image_base64": input.image_base64,
    });

    let mut history_messages = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        match db::get_conversation(&conn, &conversation_id) {
            Ok(existing) => existing.messages.as_array().cloned().unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    };
    history_messages.push(user_msg.clone());
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::upsert_conversation(
            &conn,
            &ConversationRecord {
                id: conversation_id.clone(),
                title: title_from(&input.content),
                messages: Value::Array(history_messages.clone()),
                created_at: created.clone(),
                updated_at: created.clone(),
            },
        )?;
    }

    // Rebuild history messages. Older user turns that carry an image become
    // vision parts here; the newest message is left plain — build_request
    // attaches input.image_base64 to it exactly once (no double image).
    // Tool messages (assistant tool_calls + role:"tool" results) must
    // round-trip so later turns still see them (specs/phase-2.3 §9.3).
    let last_idx = history_messages.len().saturating_sub(1);
    let chat_messages: Vec<ChatMessage> = history_messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            let role = m.get("role")?.as_str()?.to_string();
            match role.as_str() {
                "user" => {
                    let text = m
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let image = if idx == last_idx {
                        None
                    } else {
                        m.get("image_base64")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                    };
                    let content = match image {
                        Some(b64) => serde_json::json!([
                            { "type": "text", "text": text },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{b64}") } },
                        ]),
                        None => Value::String(text),
                    };
                    Some(ChatMessage {
                        role,
                        content,
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    })
                }
                "assistant" => {
                    // May carry text, tool_calls, or both.
                    let content = m.get("content").cloned().unwrap_or(Value::Null);
                    let tool_calls = m.get("tool_calls").cloned();
                    Some(ChatMessage {
                        role,
                        content,
                        tool_calls,
                        tool_call_id: None,
                        name: None,
                    })
                }
                "tool" => {
                    let content = Value::String(
                        m.get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    );
                    Some(ChatMessage {
                        role,
                        content,
                        tool_calls: None,
                        tool_call_id: m
                            .get("tool_call_id")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        name: m.get("name").and_then(|v| v.as_str()).map(str::to_string),
                    })
                }
                _ => None,
            }
        })
        .collect();
    // Attach clipboard + image to the last user message ONCE; every
    // agent-loop round reuses these messages verbatim (build_request with
    // None/None leaves them untouched).
    let request = build_request(
        &model,
        chat_messages,
        input.clipboard_context.as_deref(),
        input.image_base64.as_deref(),
        None,
        None,
    );
    let chat_messages = request.messages;
    let url = join_chat_url(&base_url);
    let (tx, mut rx) = watch::channel(false);
    *state.cancel.lock().map_err(|e| e.to_string())? = Some(tx);

    // Action schemas only join the request when BOTH switches are on
    // (actions default off — opt-in; specs/phase-2.6 §1d).
    let tools_json = if settings.tools_enabled {
        let mut schemas = crate::tools::all_schemas();
        if settings.actions_enabled {
            schemas.extend(crate::tools::actions::all_action_schemas());
        }
        Some(schemas)
    } else {
        None
    };
    let tools_on = settings.tools_enabled;

    let app_clone = app.clone();
    let conv_id = conversation_id.clone();
    let http = state.http.clone();
    tauri::async_runtime::spawn(async move {
        let result = agent_loop(
            &http, &url, &api_key, &model, chat_messages, &app_clone, &conv_id, &mut rx,
            tools_json, tools_on,
        )
        .await;
        match result {
            Ok(()) => {
                eprintln!("[agent] loop ok -> emit done");
                let _ = app_clone.emit(
                    "chat://done",
                    json!({ "conversation_id": conv_id, "cancelled": false }),
                );
            }
            Err(err) => {
                eprintln!("[agent] loop err -> emit error: {err}");
                let _ = app_clone.emit(
                    "chat://error",
                    json!({ "conversation_id": conv_id, "message": err }),
                );
            }
        }
    });

    Ok(SendMessageOutput { conversation_id })
}

/// The agent loop (specs/phase-2.3 §8.5): POST → stream → if the model
/// asked for tools, execute them, append the results, re-POST — until a
/// final text answer, cancellation, or the round/timeout guards trip.
#[allow(clippy::too_many_arguments)]
async fn agent_loop(
    http: &reqwest::Client,
    url: &str,
    api_key: &str,
    model: &str,
    mut messages: Vec<ChatMessage>,
    app: &AppHandle,
    conversation_id: &str,
    cancel: &mut watch::Receiver<bool>,
    tools: Option<Vec<Value>>,
    tools_enabled: bool,
) -> Result<(), String> {
    const MAX_TOOL_ROUNDS: usize = 10;
    const LOOP_TIMEOUT_SECS: u64 = 120;

    let deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(LOOP_TIMEOUT_SECS);
    let mut round = 0usize;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err("Agent loop timed out after 120 seconds.".into());
        }
        if *cancel.borrow() {
            let _ = app.emit(
                "chat://done",
                json!({ "conversation_id": conversation_id, "cancelled": true }),
            );
            return Ok(());
        }

        // Round 0..MAX-1: offer tools (when enabled), auto choice. Past the
        // cap: force a text answer with tool_choice "none".
        let (req_tools, req_tool_choice) = if round >= MAX_TOOL_ROUNDS {
            (None, Some(serde_json::json!("none")))
        } else if tools_enabled {
            (tools.clone(), Some(serde_json::json!("auto")))
        } else {
            (None, None)
        };

        let request = crate::api::client::build_request(
            model,
            messages.clone(),
            None,
            None,
            req_tools,
            req_tool_choice,
        );

        let stream_result = tokio::time::timeout_at(
            deadline,
            stream_chat(http, url, api_key, &request, app, conversation_id, cancel),
        )
        .await
        .map_err(|_| "Agent loop timed out.".to_string())??;

        match stream_result {
            StreamResult::Text { text, cancelled } => {
                if cancelled {
                    let _ = app.emit(
                        "chat://done",
                        json!({ "conversation_id": conversation_id, "cancelled": true }),
                    );
                    return Ok(());
                }
                if !text.is_empty() {
                    persist_assistant(app, conversation_id, &text);
                }
                return Ok(()); // normal exit — caller emits chat://done
            }
            StreamResult::ToolCalls(calls) => {
                if round >= MAX_TOOL_ROUNDS {
                    // Defensive: tool_choice "none" should make this impossible.
                    return Err("Model refused to stop calling tools after 10 rounds.".into());
                }
                round += 1;

                // Record the assistant's tool_calls request in history + DB.
                let tool_calls_json: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id":   c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments }
                        })
                    })
                    .collect();
                let assistant_msg = ChatMessage {
                    role: "assistant".into(),
                    content: Value::Null,
                    tool_calls: Some(Value::Array(tool_calls_json)),
                    tool_call_id: None,
                    name: None,
                };
                messages.push(assistant_msg.clone());
                persist_tool_assistant(app, conversation_id, &assistant_msg);

                // Sequential execution — read-only tools are all fast;
                // parallel waits for Phase 2.6.
                for call in &calls {
                    let _ = app.emit(
                        "chat://tool-start",
                        json!({
                            "conversation_id": conversation_id,
                            "call_id":   call.id,
                            "tool_name": call.name,
                            "arguments": call.arguments,
                        }),
                    );

                    // Action tools gate behind YES/NO; read-only run free.
                    let result_str = if crate::tools::actions::is_action(&call.name) {
                        let state = app.state::<AppState>();
                        crate::tools::actions::dispatch_with_confirm(
                            &call.name,
                            &call.arguments,
                            &call.id,
                            app,
                            &state,
                            cancel,
                        )
                        .await
                        .unwrap_or_else(|e| format!("Action error: {e}"))
                    } else {
                        crate::tools::dispatch(&call.name, &call.arguments)
                            .await
                            .unwrap_or_else(|e| format!("Tool error: {e}"))
                    };

                    let tool_msg = ChatMessage {
                        role: "tool".into(),
                        content: Value::String(result_str.clone()),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        name: Some(call.name.clone()),
                    };
                    messages.push(tool_msg.clone());
                    persist_tool_result(app, conversation_id, &tool_msg);

                    let _ = app.emit(
                        "chat://tool-result",
                        json!({
                            "conversation_id": conversation_id,
                            "call_id":   call.id,
                            "tool_name": call.name,
                            "result":    result_str.chars().take(200).collect::<String>(),
                        }),
                    );
                }
                // Loop continues: next POST carries the tool results.
            }
        }
    }
}

fn title_from(content: &str) -> String {
    let trimmed = content.trim().replace('\n', " ");
    if trimmed.chars().count() <= 48 {
        trimmed
    } else {
        format!("{}…", trimmed.chars().take(48).collect::<String>())
    }
}

/// Provider errors in plain language; keys never appear in any message.
fn provider_error(status: u16, body: &str) -> String {
    let snippet: String = body.chars().take(240).collect();
    match status {
        401 | 403 => "The provider rejected the key. Check the key and the endpoint in Settings."
            .into(),
        404 => "Endpoint or model not found. Check the URL and model name in Settings.".into(),
        429 => "Rate limited. Wait a moment and send again.".into(),
        _ => format!("Provider error ({status}): {snippet}"),
    }
}

/// One fully reassembled tool call (all argument fragments joined).
#[derive(Debug, Clone)]
struct AssembledCall {
    id: String,
    name: String,
    arguments: String,
}

/// What one streamed request produced (specs/phase-2.3 §8.2).
enum StreamResult {
    /// Model produced text; bool = cancelled.
    Text { text: String, cancelled: bool },
    /// Model requested tool calls — execute and re-POST.
    ToolCalls(Vec<AssembledCall>),
}

async fn stream_chat(
    http: &reqwest::Client,
    url: &str,
    api_key: &str,
    request: &crate::api::client::ChatRequest,
    app: &AppHandle,
    conversation_id: &str,
    cancel: &mut watch::Receiver<bool>,
) -> Result<StreamResult, String> {
    use crate::api::client::{parse_delta, Delta, FinishReason};

    let response = http
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .await
        .map_err(|e| format!("Could not reach the provider: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status.as_u16(), &body));
    }
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut assembled = String::new();
    // Tool-call fragments accumulate per parallel-call index until the
    // finish_reason line arrives.
    let mut tool_bufs: std::collections::BTreeMap<usize, (String, String, String)> =
        std::collections::BTreeMap::new(); // index → (id, name, arguments)
    loop {
        if *cancel.borrow() {
            return Ok(StreamResult::Text {
                text: assembled,
                cancelled: true,
            });
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_ok() && *cancel.borrow() {
                    return Ok(StreamResult::Text { text: assembled, cancelled: true });
                }
            }
            next = stream.next() => {
                match next {
                    Some(Ok(chunk)) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));
                        while let Some(idx) = buffer.find('\n') {
                            let line = buffer[..idx].trim_end_matches('\r').to_string();
                            buffer.drain(..=idx);
                            let Some(data) = line.strip_prefix("data:") else { continue };
                            let data = data.trim();
                            if data == "[DONE]" {
                                // [DONE] without a prior finish_reason: Stop.
                                return Ok(StreamResult::Text { text: assembled, cancelled: false });
                            }
                            match parse_delta(data) {
                                Some(Delta::Content(text)) => {
                                    assembled.push_str(&text);
                                    let _ = app.emit(
                                        "chat://chunk",
                                        json!({ "conversation_id": conversation_id, "text": text }),
                                    );
                                }
                                Some(Delta::ToolCall(tc)) => {
                                    eprintln!(
                                        "[agent] toolcall frag idx={} id={:?} name={:?} args+{}",
                                        tc.index,
                                        tc.id,
                                        tc.name,
                                        tc.arguments.len()
                                    );
                                    let entry = tool_bufs.entry(tc.index)
                                        .or_insert_with(|| (String::new(), String::new(), String::new()));
                                    if let Some(id) = tc.id { entry.0 = id; }
                                    if let Some(name) = tc.name { entry.1 = name; }
                                    entry.2.push_str(&tc.arguments);
                                }
                                Some(Delta::Done(FinishReason::ToolCalls)) => {
                                    eprintln!("[agent] done tool_calls");
                                    let calls = tool_bufs.into_values()
                                        .map(|(id, name, arguments)| AssembledCall { id, name, arguments })
                                        .collect();
                                    return Ok(StreamResult::ToolCalls(calls));
                                }
                                Some(Delta::Done(other)) => {
                                    eprintln!("[agent] done other: {other:?}");
                                    return Ok(StreamResult::Text { text: assembled, cancelled: false });
                                }
                                None => {}
                            }
                        }
                    }
                    Some(Err(err)) => return Err(err.to_string()),
                    None => return Ok(StreamResult::Text { text: assembled, cancelled: false }),
                }
            }
        }
    }
}

fn persist_assistant(app: &AppHandle, conversation_id: &str, text: &str) {
    let state = app.state::<AppState>();
    let Ok(conn) = state.db.lock() else { return };
    let Ok(mut rec) = db::get_conversation(&conn, conversation_id) else { return };
    let mut messages = rec.messages.as_array().cloned().unwrap_or_default();
    messages.push(json!({
        "id": Uuid::new_v4().to_string(),
        "role": "assistant",
        "content": text,
        "created_at": db::stamp(),
    }));
    rec.messages = Value::Array(messages);
    rec.updated_at = db::stamp();
    let _ = db::upsert_conversation(&conn, &rec);
}

/// Persist an assistant message that requested tool_calls (no text content).
fn persist_tool_assistant(app: &AppHandle, conversation_id: &str, msg: &ChatMessage) {
    let state = app.state::<AppState>();
    let Ok(conn) = state.db.lock() else { return };
    let Ok(mut rec) = db::get_conversation(&conn, conversation_id) else { return };
    let mut messages = rec.messages.as_array().cloned().unwrap_or_default();
    messages.push(json!({
        "id":         Uuid::new_v4().to_string(),
        "role":       "assistant",
        "content":    null,
        "tool_calls": msg.tool_calls,
        "created_at": db::stamp(),
    }));
    rec.messages = Value::Array(messages);
    rec.updated_at = db::stamp();
    let _ = db::upsert_conversation(&conn, &rec);
}

/// Persist a role:"tool" result message.
fn persist_tool_result(app: &AppHandle, conversation_id: &str, msg: &ChatMessage) {
    let state = app.state::<AppState>();
    let Ok(conn) = state.db.lock() else { return };
    let Ok(mut rec) = db::get_conversation(&conn, conversation_id) else { return };
    let mut messages = rec.messages.as_array().cloned().unwrap_or_default();
    messages.push(json!({
        "id":           Uuid::new_v4().to_string(),
        "role":         "tool",
        "tool_call_id": msg.tool_call_id,
        "name":         msg.name,
        "content":      msg.content,
        "created_at":   db::stamp(),
    }));
    rec.messages = Value::Array(messages);
    rec.updated_at = db::stamp();
    let _ = db::upsert_conversation(&conn, &rec);
}

/* ---------- action confirmations (Phase 2.6) ---------- */

/// Answer a pending action confirmation card (YES/NO).
#[tauri::command]
pub fn confirm_tool(
    state: State<AppState>,
    call_id: String,
    approved: bool,
) -> Result<(), String> {
    let mut map = state.pending_confirmations.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = map.remove(&call_id) {
        let _ = tx.send(approved);
        Ok(())
    } else {
        Err(format!("No pending confirmation for call_id: {call_id}"))
    }
}

/* ---------- voice (Phase 2.4: spoken replies) ---------- */

/// Speak text aloud via SAPI (fire-and-forget; specs/phase-2.4 §3c).
#[tauri::command]
pub fn speak_text(text: String) -> Result<(), String> {
    crate::speech::sapi_speak(&text)
}

/// Stop any queued SAPI speech immediately.
#[tauri::command]
pub fn stop_speech() -> Result<(), String> {
    crate::speech::stop_sapi_speech()
}

/* ---------- screenshot (Phase 2.1: direct AI analysis) ---------- */

#[derive(Debug, Deserialize)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// One still frame, optionally cropped to a region (physical px). Saves
/// quietly to Pictures\Thuki + copies to clipboard, and returns the base64
/// PNG so the caller can send it straight to the AI. No Explorer popup.
/// Async: PNG encode + disk + clipboard are blocking work.
#[tauri::command]
pub async fn capture_screen(region: Option<Region>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let full = crate::capture::capture_display()?;
        let capture = match region {
            Some(r) if r.w > 0 && r.h > 0 => crate::capture::crop(
                &full,
                r.x.max(0) as u32,
                r.y.max(0) as u32,
                r.w,
                r.h,
            ),
            _ => full,
        };
        let base64 = crate::capture::base64_png(&capture)?;
        let path = crate::capture::save_png(&capture)?;
        let _ = crate::capture::copy_to_clipboard(&capture);
        Ok(json!({ "path": path.to_string_lossy(), "base64_png": base64 }))
    })
    .await
    .map_err(|e| format!("Capture task failed: {e}"))?
}

/* ---------- snip overlay window control (Phase 2.1, Snipping-Tool mode) ---------- */

/// Expand the main window over the FULL primary monitor for region
/// selection. `resizable: false` + the 56px min size make Windows silently
/// clamp programmatic resizes, so unlock both, resize to the exact monitor
/// rect (Win32-authoritative physical px), and report the disc's old spot.
#[tauri::command]
pub fn begin_snip(app: AppHandle) -> Result<serde_json::Value, String> {
    let run = || -> Result<serde_json::Value, String> {
        let win = app
            .get_webview_window("main")
            .ok_or_else(|| {
                eprintln!("[snip] FAIL: window not found");
                "window not found".to_string()
            })?;
        let pos = win.outer_position().map_err(|e| {
            eprintln!("[snip] FAIL: outer_position: {e}");
            e.to_string()
        })?;
        let monitor = win
            .primary_monitor()
            .map_err(|e| {
                eprintln!("[snip] FAIL: primary_monitor: {e}");
                e.to_string()
            })?
            .ok_or_else(|| {
                eprintln!("[snip] FAIL: primary_monitor returned None");
                "no primary monitor".to_string()
            })?;
        let mpos = monitor.position();
        let msize = monitor.size();
        let (mw, mh) = (msize.width, msize.height);
        eprintln!("[snip] monitor {mw}x{mh} at {}/{}, disc at {}/{}", mpos.x, mpos.y, pos.x, pos.y);
        win.set_resizable(true).map_err(|e| {
            eprintln!("[snip] FAIL: set_resizable: {e}");
            e.to_string()
        })?;
        let cleared = win.set_min_size(None::<tauri::LogicalSize<f64>>).is_ok();
        if !cleared {
            let _ = win.set_min_size(Some(tauri::PhysicalSize::new(1u32, 1u32)));
        }
        eprintln!("[snip] resizable unlocked, min cleared={cleared}");
        win.set_size(tauri::PhysicalSize::new(msize.width, msize.height))
            .map_err(|e| {
                eprintln!("[snip] FAIL: set_size: {e}");
                e.to_string()
            })?;
        win.set_position(tauri::PhysicalPosition::new(mpos.x, mpos.y))
            .map_err(|e| {
                eprintln!("[snip] FAIL: set_position: {e}");
                e.to_string()
            })?;
        eprintln!("[snip] window resized+positioned OK");
        Ok(json!({
            "restore_x": pos.x,
            "restore_y": pos.y,
            "mon_x": mpos.x,
            "mon_y": mpos.y,
            "mon_w": msize.width,
            "mon_h": msize.height,
        }))
    };
    let out = run();
    if out.is_err() {
        eprintln!("[snip] begin_snip returned error: {:?}", out.as_ref().err());
    }
    out
}

/// Shrink back to the 56×56 disc at its saved spot and re-lock the size.
#[tauri::command]
pub fn end_snip(app: AppHandle, restore_x: i32, restore_y: i32) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("window not found")?;
    win.set_size(tauri::PhysicalSize::new(56u32, 56u32))
        .map_err(|e| e.to_string())?;
    win.set_position(tauri::PhysicalPosition::new(restore_x, restore_y))
        .map_err(|e| e.to_string())?;
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(56.0f64, 56.0f64)));
    let _ = win.set_resizable(false);
    Ok(())
}

/* ---------- window (kept from Phase 0) ---------- */

#[tauri::command]
pub fn toggle_window(app: AppHandle) {
    hotkey::toggle_window(&app);
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    hotkey::hide_window(&app);
}
