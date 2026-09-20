//! list_windows + get_active_window tools (specs/phase-2.3 §7.2) — Win32
//! EnumWindows / GetForegroundWindow, visible non-minimized windows with
//! title + process name + pid. Thuki itself is excluded.

use serde::Serialize;
use serde_json::json;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible,
};

#[derive(Serialize)]
struct WindowInfo {
    title: String,
    process: String,
    pid: u32,
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
        else {
            return pid.to_string();
        };
        let mut buf = [0u16; 256];
        let len = GetModuleBaseNameW(handle, None, &mut buf);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if len == 0 {
            return pid.to_string();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

fn info_for(hwnd: HWND) -> WindowInfo {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    WindowInfo {
        title: window_title(hwnd),
        process: if pid > 0 { process_name(pid) } else { String::new() },
        pid,
    }
}

fn enumerate_visible_windows() -> Vec<WindowInfo> {
    let mut out: Vec<WindowInfo> = Vec::new();
    let ptr = LPARAM(&mut out as *mut Vec<WindowInfo> as isize);
    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            ptr,
        );
    }
    out
}

unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // The whole body is unsafe context (unsafe fn); Win32 calls are fine.
    let out = unsafe { &mut *(lparam.0 as *mut Vec<WindowInfo>) };
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return BOOL(1);
    }
    let info = info_for(hwnd);
    if info.title.is_empty() || info.process.eq_ignore_ascii_case("thuki-win.exe") {
        return BOOL(1);
    }
    out.push(info);
    BOOL(1)
}

pub fn list_windows_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "list_windows",
            "description": "List all visible, non-minimized top-level windows. Returns title, process name, and PID for each.",
            "parameters": { "type": "object", "properties": {}, "required": [] }
        }
    })
}

pub fn get_active_window_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_active_window",
            "description": "Return the title and process name of the currently focused window.",
            "parameters": { "type": "object", "properties": {}, "required": [] }
        }
    })
}

pub async fn run_list(_args: &serde_json::Value) -> super::ToolResult {
    let windows = tokio::task::spawn_blocking(enumerate_visible_windows)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&windows).map_err(|e| e.to_string())
}

pub async fn run_active(_args: &serde_json::Value) -> super::ToolResult {
    let info = tokio::task::spawn_blocking(|| {
        let hwnd = unsafe { GetForegroundWindow() };
        info_for(hwnd)
    })
    .await
    .map_err(|e| e.to_string())?;
    serde_json::to_string(&info).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn list_windows_returns_at_least_one() {
        // Any live Windows session has at least one visible window.
        let result = super::run_list(&serde_json::Value::Null).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(!v.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn active_window_has_fields() {
        let result = super::run_active(&serde_json::Value::Null).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["title"].is_string());
        assert!(v["pid"].is_number());
    }
}
