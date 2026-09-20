//! find_elements action (specs/phase-2.6 §5e) — raw Win32 UIA via the
//! windows crate (the uiautomation crate needs windows 0.62 and is
//! excluded). Read-only, but confirmed like all actions for uniformity.

use serde::Serialize;
use serde_json::{json, Value};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants,
};

#[derive(Serialize)]
struct UiElement {
    name: String,
    role: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

pub fn schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "find_elements",
            "description": "Find UI elements in the focused window matching an optional text query. Returns elements with name, role, and center coordinates (x, y). Use the returned x/y with click_at.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Optional substring to filter elements by name (case-insensitive). Omit to list all (up to 100)."
                    }
                },
                "required": []
            }
        }
    })
}

pub async fn run(args: &Value) -> super::ToolResult {
    let query = args["query"].as_str().map(str::to_lowercase);
    tokio::task::spawn_blocking(move || find_elements_sync(query.as_deref()))
        .await
        .map_err(|e| format!("find_elements task: {e}"))?
}

fn find_elements_sync(query: Option<&str>) -> super::ToolResult {
    const MAX_ELEMENTS: usize = 100;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)
            .map_err(|e| format!("UIA init: {e}"))?;
        let focused: IUIAutomationElement = uia
            .GetFocusedElement()
            .map_err(|e| format!("GetFocusedElement: {e}"))?;

        let condition = uia
            .CreateTrueCondition()
            .map_err(|e| format!("CreateTrueCondition: {e}"))?;
        let element_array = focused
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| format!("FindAll: {e}"))?;

        let count = element_array.Length().map_err(|e| format!("Length: {e}"))? as usize;
        let mut results = Vec::new();

        for i in 0..count.min(MAX_ELEMENTS) {
            let Ok(el) = element_array.GetElement(i as i32) else {
                continue;
            };

            let name = el.CurrentName().unwrap_or_default().to_string();
            if name.is_empty() {
                continue;
            }
            if let Some(q) = query {
                if !name.to_lowercase().contains(q) {
                    continue;
                }
            }

            let role = ctrl_type_name(el.CurrentControlType().map(|c| c.0).unwrap_or(0));
            let rect = el.CurrentBoundingRectangle().unwrap_or_default();
            results.push(UiElement {
                name,
                role,
                x: rect.left + (rect.right - rect.left) / 2,
                y: rect.top + (rect.bottom - rect.top) / 2,
                w: rect.right - rect.left,
                h: rect.bottom - rect.top,
            });
        }

        serde_json::to_string(&results).map_err(|e| e.to_string())
    }
}

fn ctrl_type_name(id: i32) -> String {
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        _ => "Unknown",
    }
    .to_string()
}
