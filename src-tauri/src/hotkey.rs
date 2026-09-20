//! Overlay show/hide used by the tray menu, the Alt+Space hotkey, and the
//! frontend commands. Showing also emits `overlay://shown` with the current
//! clipboard text (Smart Clipboard context, specs/phase-1/spec.md 1.5).

use tauri::{AppHandle, Emitter, Manager};

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let clipboard = crate::clipboard::read_text();
        let _ = app.emit(
            "overlay://shown",
            serde_json::json!({ "clipboard": clipboard }),
        );
    }
}

pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

pub fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            hide_window(app);
        } else {
            show_window(app);
        }
    }
}

/// (Re)register the global hotkey; the combo lives in AppState + SQLite.
pub fn register(app: &AppHandle, combo: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

    let shortcut: Shortcut = combo.parse().map_err(|e| format!("Invalid hotkey: {e}"))?;
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;
    manager
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_window(app);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
