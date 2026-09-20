//! SQLite store for non-secret settings and conversations
//! (specs/phase-1/spec.md 1.2). Keys never live here.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::presets::{preset, PresetId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub active_preset: PresetId,
    pub model: String,
    pub hotkey: String,
    pub theme: String,
    pub models: BTreeMap<String, String>,
    pub base_urls: BTreeMap<String, String>,
    /// Per-preset "a key exists" flags only — never key material.
    pub api_key_set: BTreeMap<String, bool>,
    /// Agent tools master switch (specs/phase-2.3 §10). Default true.
    pub tools_enabled: bool,
    /// Spoken replies mode: "always" | "auto" | "never" (specs/phase-2.4).
    pub voice_replies_mode: String,
    /// BCP-47 tag for speech recognition (default en-US).
    pub stt_language: String,
    /// Action tools master switch — default OFF (specs/phase-2.6 §6a).
    pub actions_enabled: bool,
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            messages TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
         );",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn get_value(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other.to_string()),
    })
}

pub fn set_value(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Public read for setup code (lib.rs) outside the db module's privates.
pub fn get_value_public(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    get_value(conn, key)
}

/// Unknown/absent values fall back to preset defaults; the default active
/// preset for fresh installs is GLM.
pub fn load_settings(conn: &Connection, key_set: BTreeMap<String, bool>) -> Result<Settings, String> {
    let active_preset = get_value(conn, "active_preset")?
        .and_then(|v| serde_json::from_str::<PresetId>(&format!("\"{v}\"")).ok())
        .unwrap_or(PresetId::Glm);

    let mut models = BTreeMap::new();
    let mut base_urls = BTreeMap::new();
    for id in PresetId::all() {
        let p = preset(id);
        let model = get_value(conn, &format!("model:{}", id.as_str()))?.unwrap_or_default();
        let base = get_value(conn, &format!("base_url:{}", id.as_str()))?.unwrap_or_default();
        if !model.is_empty() {
            models.insert(id.as_str().to_string(), model);
        }
        if !base.is_empty() {
            base_urls.insert(id.as_str().to_string(), base);
        }
        if base_urls.get(id.as_str()).is_none() && !p.base_url.is_empty() {
            base_urls.insert(id.as_str().to_string(), p.base_url.to_string());
        }
    }

    let model = models
        .get(active_preset.as_str())
        .cloned()
        .unwrap_or_else(|| preset(active_preset).default_model.to_string());
    let hotkey = get_value(conn, "hotkey")?.unwrap_or_else(|| "Alt+Space".into());
    let theme = get_value(conn, "theme")?.unwrap_or_else(|| "dark".into());
    let tools_enabled = get_value(conn, "tools_enabled")?
        .map(|v| !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    let voice_replies_mode = get_value(conn, "voice_replies_mode")?.unwrap_or_else(|| "auto".into());
    let stt_language = get_value(conn, "stt_language")?.unwrap_or_else(|| "en-US".into());
    let actions_enabled = get_value(conn, "actions_enabled")?
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    Ok(Settings {
        active_preset,
        model,
        hotkey,
        theme,
        models,
        base_urls,
        api_key_set: key_set,
        tools_enabled,
        voice_replies_mode,
        stt_language,
        actions_enabled,
    })
}

/// Save the core (non-secret) settings; `base_url` applies to `active_preset`.
pub fn save_core_settings(
    conn: &Connection,
    active: PresetId,
    model: &str,
    hotkey: &str,
    theme: &str,
    base_url: Option<&str>,
    tools_enabled: bool,
    voice_replies_mode: Option<&str>,
    stt_language: Option<&str>,
    actions_enabled: Option<bool>,
) -> Result<(), String> {
    if let Some(on) = actions_enabled {
        set_value(conn, "actions_enabled", if on { "true" } else { "false" })?;
    }
    set_value(conn, "tools_enabled", if tools_enabled { "true" } else { "false" })?;
    if let Some(mode) = voice_replies_mode {
        set_value(conn, "voice_replies_mode", mode)?;
    }
    if let Some(lang) = stt_language {
        set_value(conn, "stt_language", lang)?;
    }
    set_value(conn, "active_preset", active.as_str())?;
    if !model.trim().is_empty() {
        set_value(conn, &format!("model:{}", active.as_str()), model.trim())?;
    }
    set_value(conn, "hotkey", hotkey)?;
    set_value(conn, "theme", theme)?;
    if let Some(url) = base_url.map(str::trim).filter(|s| !s.is_empty()) {
        set_value(conn, &format!("base_url:{}", active.as_str()), url)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: String,
    pub title: String,
    pub messages: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

fn now_iso() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

pub fn upsert_conversation(conn: &Connection, rec: &ConversationRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO conversations (id, title, messages, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            messages = excluded.messages,
            updated_at = excluded.updated_at",
        rusqlite::params![
            rec.id,
            rec.title,
            rec.messages.to_string(),
            rec.created_at,
            rec.updated_at,
        ],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

pub fn get_conversation(conn: &Connection, id: &str) -> Result<ConversationRecord, String> {
    conn.query_row(
        "SELECT id, title, messages, created_at, updated_at FROM conversations WHERE id = ?1",
        [id],
        |row| {
            let messages: String = row.get(2)?;
            Ok(ConversationRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                messages: serde_json::from_str(&messages)
                    .unwrap_or(Value::Array(Vec::new())),
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

pub fn list_conversations(conn: &Connection) -> Result<Vec<ConversationSummary>, String> {
    let mut stmt = conn
        .prepare("SELECT id, title, updated_at FROM conversations ORDER BY updated_at DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn delete_conversation(conn: &Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM conversations WHERE id = ?1", [id])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn stamp() -> String {
    now_iso()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        open(Path::new(":memory:")).expect("schema")
    }

    #[test]
    fn fresh_defaults_to_glm() {
        let conn = memory();
        let s = load_settings(&conn, BTreeMap::new()).expect("load");
        assert_eq!(s.active_preset, PresetId::Glm);
        assert_eq!(s.hotkey, "Alt+Space");
        assert_eq!(
            s.base_urls.get("anymodel").map(String::as_str),
            Some("https://anymodel.org/v1")
        );
    }

    #[test]
    fn unknown_preset_falls_back_to_glm() {
        let conn = memory();
        set_value(&conn, "active_preset", "does-not-exist").expect("set");
        let s = load_settings(&conn, BTreeMap::new()).expect("load");
        assert_eq!(s.active_preset, PresetId::Glm);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let conn = memory();
        save_core_settings(
            &conn,
            PresetId::Anymodel,
            "gpt-5.6-sol",
            "Ctrl+J",
            "dark",
            Some("https://anymodel.org/v1"),
            true,
            None,
            None,
            None,
        )
        .expect("save");
        let s = load_settings(&conn, BTreeMap::new()).expect("load");
        assert_eq!(s.active_preset, PresetId::Anymodel);
        assert_eq!(s.model, "gpt-5.6-sol");
        assert_eq!(s.hotkey, "Ctrl+J");
    }

    #[test]
    fn conversation_upsert_get_list_delete() {
        let conn = memory();
        let rec = ConversationRecord {
            id: "c1".into(),
            title: "Hello".into(),
            messages: serde_json::json!([{ "role": "user", "content": "hi" }]),
            created_at: "1".into(),
            updated_at: "1".into(),
        };
        upsert_conversation(&conn, &rec).expect("upsert");
        let got = get_conversation(&conn, "c1").expect("get");
        assert_eq!(got.title, "Hello");
        assert!(list_conversations(&conn).expect("list").len() == 1);
        delete_conversation(&conn, "c1").expect("delete");
        assert!(get_conversation(&conn, "c1").is_err());
    }

    #[test]
    fn tool_messages_survive_roundtrip() {
        // specs/phase-2.3 §14.5 — assistant tool_calls + role:"tool"
        // messages must persist and reload intact (null content included).
        let conn = memory();
        let messages = serde_json::json!([
            { "id": "1", "role": "user", "content": "what windows are open?", "created_at": "1" },
            { "id": "2", "role": "assistant", "content": null,
              "tool_calls": [{"id":"call_x","type":"function",
                  "function":{"name":"list_windows","arguments":"{}"}}],
              "created_at": "2" },
            { "id": "3", "role": "tool", "tool_call_id": "call_x",
              "name": "list_windows", "content": "[{\"title\":\"Notepad\"}]", "created_at": "3" },
            { "id": "4", "role": "assistant", "content": "You have Notepad open.", "created_at": "4" }
        ]);
        let rec = ConversationRecord {
            id: "c1".into(),
            title: "t".into(),
            messages: messages.clone(),
            created_at: "1".into(),
            updated_at: "4".into(),
        };
        upsert_conversation(&conn, &rec).unwrap();
        let got = get_conversation(&conn, "c1").unwrap();
        let arr = got.messages.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[1]["role"], "assistant");
        assert!(arr[1]["tool_calls"].is_array());
        assert_eq!(arr[2]["role"], "tool");
        assert_eq!(arr[2]["tool_call_id"], "call_x");
        assert_eq!(arr[3]["content"], "You have Notepad open.");
    }
}
