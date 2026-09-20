// Jini: floating Windows AI assistant overlay (renamed from Thuki-Win,
// 2026-09-20 — data migrates automatically on first run).
// Phase 1 (specs/phase-1/spec.md): AI base over the Phase 0 skeleton.

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;
use tokio::sync::oneshot;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tokio::sync::watch;

pub mod api;
pub mod capture;
pub mod clipboard;
pub mod commands;
pub mod db;
pub mod hotkey;
pub mod secrets;
pub mod speech;
pub mod speech_recognition;
pub mod tools;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub cancel: Mutex<Option<watch::Sender<bool>>>,
    pub hotkey: Mutex<String>,
    pub http: reqwest::Client,
    /// Cached tools toggle; send_message re-reads fresh from settings so a
    /// Settings change applies to the next message without a restart.
    pub tools_enabled: Mutex<bool>,
    /// Pending action confirmations: call_id → oneshot sender. The agent
    /// loop suspends the action until the user answers YES/NO (2.6 §1a).
    pub pending_confirmations: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    /// Action tools master switch — DEFAULT OFF (opt-in, safety-first).
    pub actions_enabled: Mutex<bool>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

            // Rename migration (thuki → jini): if we have no database yet
            // but the old install does, copy it over. Old data is never
            // touched or deleted.
            let new_db = dir.join("jini.sqlite");
            if !new_db.exists() {
                if let Some(old_dir) = dirs_data_dir("com.thuki.win") {
                    let old_db = old_dir.join("thuki.sqlite");
                    if old_db.exists() {
                        if let Err(err) = std::fs::copy(&old_db, &new_db) {
                            eprintln!("[jini] data migration copy failed: {err}");
                        } else {
                            eprintln!("[jini] migrated history/settings from the Thuki install");
                        }
                    }
                }
            }

            let conn = db::open(&new_db)?;
            let combo = db::get_value_public(&conn, "hotkey")?.unwrap_or_else(|| "Alt+Space".into());
            let tools_on = db::get_value_public(&conn, "tools_enabled")?
                .map(|v| !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true);
            let actions_on = db::get_value_public(&conn, "actions_enabled")?
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            app.manage(AppState {
                db: Mutex::new(conn),
                cancel: Mutex::new(None),
                hotkey: Mutex::new(combo.clone()),
                http: reqwest::Client::new(),
                tools_enabled: Mutex::new(tools_on),
                pending_confirmations: Mutex::new(HashMap::new()),
                actions_enabled: Mutex::new(actions_on),
            });

            if let Some(window) = app.get_webview_window("main") {
                // Tiny window: an explicit min size overrides the Windows
                // default track floor (~133px at this DPI/font setup).
                let _ = window.set_min_size(Some(tauri::LogicalSize::new(56.0f64, 56.0f64)));
                let _ = window.set_size(tauri::LogicalSize::new(56.0f64, 56.0f64));
                let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
            }

            let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &settings, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Jini — AI assistant")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" | "settings" => hotkey::show_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            secrets::migrate_old_entries();

            if let Err(err) = hotkey::register(app.handle(), &combo) {
                eprintln!("hotkey register failed: {err}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::set_api_key,
            commands::has_api_key,
            commands::delete_api_key,
            commands::set_hotkey,
            commands::get_clipboard,
            commands::test_provider,
            commands::get_conversations,
            commands::get_conversation,
            commands::delete_conversation,
            commands::save_conversation,
            commands::send_message,
            commands::cancel_message,
            commands::capture_screen,
            commands::confirm_tool,
            commands::speak_text,
            commands::stop_speech,
            speech_recognition::start_listening,
            speech_recognition::stop_listening,
            commands::begin_snip,
            commands::end_snip,
            commands::toggle_window,
            commands::hide_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// The per-user roaming data dir for an app identifier, the same way
/// Tauri resolves it (`%APPDATA%\<identifier>`).
fn dirs_data_dir(identifier: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|base| {
        std::path::PathBuf::from(base).join(identifier)
    })
}
