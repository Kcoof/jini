//! API keys in Windows Credential Manager (specs/phase-1/spec.md 1.2).
//! Renamed thuki → jini (2026-09-20): on first run the old "thuki-win"
//! entries are copied to the new "jini" names so existing keys (for
//! example the AnyModel key) carry over without re-entry. Old entries
//! are left in place.

use std::collections::BTreeMap;

use keyring::Entry;

use crate::api::presets::PresetId;

fn entry(preset: PresetId) -> Result<Entry, String> {
    Entry::new("jini", &format!("jini/{}", preset.as_str())).map_err(|e| e.to_string())
}

fn old_entry(preset: PresetId) -> Result<Entry, String> {
    Entry::new("thuki-win", &format!("thuki-win/{}", preset.as_str())).map_err(|e| e.to_string())
}

/// One-time rename migration: copy any old-service key into the new one.
/// Runs at startup; no-ops once the new entry exists.
pub fn migrate_old_entries() {
    for id in PresetId::all() {
        let (Ok(new), Ok(old)) = (entry(id), old_entry(id)) else {
            continue;
        };
        if new.get_password().is_ok() {
            continue; // already migrated or newly set
        }
        if let Ok(key) = old.get_password() {
            let _ = new.set_password(&key);
        }
    }
}

pub fn set_api_key(preset: PresetId, key: &str) -> Result<(), String> {
    entry(preset)?
        .set_password(key)
        .map_err(|e| format!("Could not save the key: {e}"))
}

pub fn get_api_key(preset: PresetId) -> Result<Option<String>, String> {
    match entry(preset)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(keyring::Error::Ambiguous(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn has_api_key(preset: PresetId) -> Result<bool, String> {
    Ok(get_api_key(preset)?.is_some())
}

pub fn delete_api_key(preset: PresetId) -> Result<(), String> {
    match entry(preset)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Per-preset "a key exists" map for `get_settings` — booleans only.
pub fn key_map() -> Result<BTreeMap<String, bool>, String> {
    let mut map = BTreeMap::new();
    for id in PresetId::all() {
        map.insert(id.as_str().to_string(), has_api_key(id)?);
    }
    Ok(map)
}
