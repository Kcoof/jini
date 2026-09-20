//! Input synthesis actions (specs/phase-2.6 §5b–d): click_at, type_text,
//! press_keys — all through SendInput on the blocking pool.

use serde_json::{json, Value};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11,
    VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_LEFT, VK_LWIN,
    VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, INPUT, INPUT_0,
    INPUT_KEYBOARD, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSE_EVENT_FLAGS, MOUSEINPUT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use super::guard;

pub fn click_at_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "click_at",
            "description": "Move the mouse to physical screen coordinates (x, y) and left-click. Use find_elements first to locate UI elements reliably.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": { "type": "integer", "description": "Physical pixel X from the left of the primary monitor." },
                    "y": { "type": "integer", "description": "Physical pixel Y from the top of the primary monitor." }
                },
                "required": ["x", "y"]
            }
        }
    })
}

pub fn type_text_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "type_text",
            "description": "Type text into the currently focused window/control.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "The text to type. Maximum 500 characters." }
                },
                "required": ["text"]
            }
        }
    })
}

pub fn press_keys_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "press_keys",
            "description": "Press a key combination, e.g. \"Ctrl+S\", \"Enter\", \"Alt+F4\", \"Win+E\".",
            "parameters": {
                "type": "object",
                "properties": {
                    "keys": { "type": "string", "description": "Keys joined by +. Modifiers: Ctrl, Alt, Shift, Win." }
                },
                "required": ["keys"]
            }
        }
    })
}

/// Physical px → SendInput's 0..65535 primary-monitor space (specs §5b).
fn to_normalized(x_phys: i32, y_phys: i32) -> (i32, i32) {
    unsafe {
        let screen_w = GetSystemMetrics(SM_CXSCREEN).max(1);
        let screen_h = GetSystemMetrics(SM_CYSCREEN).max(1);
        let nx = (x_phys * 65535 + screen_w / 2) / screen_w;
        let ny = (y_phys * 65535 + screen_h / 2) / screen_h;
        (nx, ny)
    }
}

pub async fn click_at(args: &Value) -> super::ToolResult {
    let x = args["x"].as_i64().ok_or("click_at: missing x")? as i32;
    let y = args["y"].as_i64().ok_or("click_at: missing y")? as i32;
    guard::check_click_bounds(x, y)?;

    tokio::task::spawn_blocking(move || {
        unsafe {
            let (nx, ny) = to_normalized(x, y);
            let mk = |dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx,
                        dy,
                        mouseData: 0,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            let move_input = mk(
                nx,
                ny,
                MOUSE_EVENT_FLAGS((MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE).0),
            );
            let down = mk(0, 0, MOUSEEVENTF_LEFTDOWN);
            let up = mk(0, 0, MOUSEEVENTF_LEFTUP);
            let inputs = [move_input, down, up];
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent == 3 {
                Ok(format!("Clicked at ({x}, {y})"))
            } else {
                Err(format!("SendInput: only {sent}/3 events sent"))
            }
        }
    })
    .await
    .map_err(|e| format!("click_at task: {e}"))?
}

pub async fn type_text(args: &Value) -> super::ToolResult {
    let text = args["text"].as_str().ok_or("type_text: missing text")?;
    if text.chars().count() > guard::MAX_TYPE_TEXT_CHARS {
        return Err(format!(
            "type_text: text too long ({} chars; max {})",
            text.chars().count(),
            guard::MAX_TYPE_TEXT_CHARS
        ));
    }
    let text = text.to_string();

    tokio::task::spawn_blocking(move || {
        unsafe {
            for ch in text.encode_utf16() {
                let mk = |flags: KEYBD_EVENT_FLAGS| INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: ch,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let down = mk(KEYEVENTF_UNICODE);
                let up = mk(KEYBD_EVENT_FLAGS(KEYEVENTF_UNICODE.0 | KEYEVENTF_KEYUP.0));
                SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
            }
            Ok(format!("Typed {} characters", text.chars().count()))
        }
    })
    .await
    .map_err(|e| format!("type_text task: {e}"))?
}

pub async fn press_keys(args: &Value) -> super::ToolResult {
    let keys_str = args["keys"].as_str().ok_or("press_keys: missing keys")?.to_string();
    if keys_str.eq_ignore_ascii_case("ctrl+alt+del") {
        return Err("press_keys: Ctrl+Alt+Del is a system-reserved combination".into());
    }
    let vkeys = parse_keys(&keys_str)?;

    tokio::task::spawn_blocking(move || {
        unsafe {
            let mk = |vk: u16, up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            let mut inputs = Vec::new();
            for &vk in &vkeys {
                inputs.push(mk(vk, false));
            }
            for &vk in vkeys.iter().rev() {
                inputs.push(mk(vk, true));
            }
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent as usize == inputs.len() {
                Ok(format!("Pressed: {keys_str}"))
            } else {
                Err(format!("SendInput: {sent}/{} events sent", inputs.len()))
            }
        }
    })
    .await
    .map_err(|e| format!("press_keys task: {e}"))?
}

fn parse_keys(s: &str) -> Result<Vec<u16>, String> {
    s.split('+').map(|t| t.trim()).map(token_to_vk).collect()
}

fn token_to_vk(token: &str) -> Result<u16, String> {
    Ok(match token.to_lowercase().as_str() {
        "ctrl" | "control" => VK_CONTROL.0,
        "alt" => VK_MENU.0,
        "shift" => VK_SHIFT.0,
        "win" | "windows" => VK_LWIN.0,
        "enter" | "return" => VK_RETURN.0,
        "esc" | "escape" => VK_ESCAPE.0,
        "tab" => VK_TAB.0,
        "space" => VK_SPACE.0,
        "backspace" => VK_BACK.0,
        "delete" | "del" => VK_DELETE.0,
        "up" => VK_UP.0,
        "down" => VK_DOWN.0,
        "left" => VK_LEFT.0,
        "right" => VK_RIGHT.0,
        "home" => VK_HOME.0,
        "end" => VK_END.0,
        "pgup" | "pageup" => VK_PRIOR.0,
        "pgdn" | "pagedown" => VK_NEXT.0,
        "f1" => VK_F1.0,
        "f2" => VK_F2.0,
        "f3" => VK_F3.0,
        "f4" => VK_F4.0,
        "f5" => VK_F5.0,
        "f6" => VK_F6.0,
        "f7" => VK_F7.0,
        "f8" => VK_F8.0,
        "f9" => VK_F9.0,
        "f10" => VK_F10.0,
        "f11" => VK_F11.0,
        "f12" => VK_F12.0,
        single if single.len() == 1 => {
            let c = single.chars().next().unwrap().to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as u16
            } else {
                return Err(format!("press_keys: unknown key token '{single}'"));
            }
        }
        other => return Err(format!("press_keys: unknown key token '{other}'")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_modifiers() {
        assert_eq!(parse_keys("Ctrl+S").unwrap(), vec![VK_CONTROL.0, 'S' as u16]);
        assert_eq!(parse_keys("Enter").unwrap(), vec![VK_RETURN.0]);
        assert_eq!(parse_keys("alt+f4").unwrap(), vec![VK_MENU.0, VK_F4.0]);
        assert_eq!(
            parse_keys("Ctrl+Shift+T").unwrap(),
            vec![VK_CONTROL.0, VK_SHIFT.0, 'T' as u16]
        );
        assert_eq!(parse_keys(" win + e ").unwrap(), vec![VK_LWIN.0, 'E' as u16]);
    }

    #[test]
    fn rejects_unknown_tokens() {
        assert!(parse_keys("Ctrl+Boom").is_err());
        assert!(parse_keys("").is_err()); // empty string → empty token
        assert!(parse_keys("Ctrl+!").is_err()); // non-alphanumeric single char
    }

    #[tokio::test]
    async fn blocked_combo_rejected() {
        let result = press_keys(&json!({ "keys": "Ctrl+Alt+Del" })).await;
        assert!(result.unwrap_err().contains("system-reserved"));
    }

    #[tokio::test]
    async fn unknown_key_rejected() {
        let result = press_keys(&json!({ "keys": "Ctrl+Boom" })).await;
        assert!(result.unwrap_err().contains("unknown key token"));
    }
}
