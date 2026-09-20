//! Safety guards for action tools (specs/phase-2.6 §4).

use std::path::Path;

/// Blocked path prefixes for launch_app and open_path (case-insensitive).
const BLOCKED_PREFIXES: &[&str] = &[
    r"C:\Windows\System32",
    r"C:\Windows\SysWOW64",
    r"C:\Windows\System",
    r"C:\Windows\Boot",
];

/// Blocked executable names (always blocked regardless of path).
const BLOCKED_EXES: &[&str] = &[
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "regedit.exe",
    "taskkill.exe",
    "format.exe",
    "diskpart.exe",
    "wmic.exe",
    "mshta.exe",
    "cscript.exe",
    "wscript.exe",
];

pub fn check_path(path: &str) -> Result<(), String> {
    // URLs are fine (no file-system risk).
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(());
    }
    let lower = path.to_lowercase();
    let p = Path::new(path);

    for prefix in BLOCKED_PREFIXES {
        if lower.starts_with(&prefix.to_lowercase()) {
            return Err(format!(
                "Action blocked: path is in a protected system directory ({path})"
            ));
        }
    }

    if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
        let lower_name = file_name.to_lowercase();
        for exe in BLOCKED_EXES {
            if lower_name == *exe {
                return Err(format!("Action blocked: {file_name} is a restricted executable"));
            }
        }
    }

    Ok(())
}

/// Screen bounds check for click_at (plausible on the virtual desktop).
pub fn check_click_bounds(x: i32, y: i32) -> Result<(), String> {
    if x < 0 || y < 0 || x > 32000 || y > 32000 {
        return Err(format!("click_at: coordinates ({x}, {y}) are out of screen bounds"));
    }
    Ok(())
}

/// type_text length cap.
pub const MAX_TYPE_TEXT_CHARS: usize = 500;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_directories_blocked() {
        assert!(check_path(r"C:\Windows\System32\cmd.exe").is_err());
        assert!(check_path(r"c:\windows\system32\whatever.dll").is_err());
        assert!(check_path(r"C:\Windows\System32").is_err());
    }

    #[test]
    fn dangerous_exes_blocked_anywhere() {
        assert!(check_path(r"C:\Tools\cmd.exe").is_err());
        assert!(check_path(r"D:\downloads\regedit.exe").is_err());
    }

    #[test]
    fn normal_paths_pass() {
        assert!(check_path(r"C:\Program Files\Notepad++\notepad++.exe").is_ok());
        assert!(check_path(r"C:\Users\Sam\Desktop\notes.txt").is_ok());
        assert!(check_path(r"https://example.com").is_ok());
        assert!(check_path("notepad.exe").is_ok());
    }

    #[test]
    fn click_bounds_checked() {
        assert!(check_click_bounds(100, 100).is_ok());
        assert!(check_click_bounds(-1, 0).is_err());
        assert!(check_click_bounds(0, 40000).is_err());
    }
}
