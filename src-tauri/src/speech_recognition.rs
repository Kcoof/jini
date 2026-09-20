//! Speech input via the Windows 11 WinRT recognizer
//! (Windows.Media.SpeechRecognition — the Voice Access engine).
//! Replaces the SAPI ISpRecoContext plan: the windows-rs 0.58 bindings
//! scramble that legacy vtable (SetInterest lands in the wrong slot and
//! returns E_INVALIDARG), while the WinRT projection is clean.
//! Fully local, no keys. Language follows the installed recognizer
//! (en-US after the Language.Speech capability install).

use serde_json::json;
use std::thread;
use tauri::{AppHandle, Emitter};
use windows::Media::SpeechRecognition::{
    SpeechRecognitionScenario, SpeechRecognitionTopicConstraint, SpeechRecognizer,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

/// The active recognizer, held so stop_listening can cancel mid-utterance.
static ACTIVE: std::sync::Mutex<Option<SpeechRecognizer>> = std::sync::Mutex::new(None);

#[tauri::command]
pub fn start_listening(app: AppHandle) -> Result<(), String> {
    {
        let guard = ACTIVE.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("Already listening".into());
        }
    }
    thread::spawn(move || listen_thread(app));
    Ok(())
}

#[tauri::command]
pub fn stop_listening() -> Result<(), String> {
    if let Ok(mut guard) = ACTIVE.lock() {
        if let Some(recognizer) = guard.take() {
            let _ = recognizer.StopRecognitionAsync();
            let _ = recognizer.Close();
        }
    }
    Ok(())
}

fn listen_thread(app: AppHandle) {
    unsafe {
        // WinRT prefers MTA.
        let com_inited = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();

        let session = run_recognizer(&app);

        if let Ok(mut guard) = ACTIVE.lock() {
            *guard = None;
        }
        let _ = app.emit("voice://end", json!({}));
        if let Err(err) = session {
            eprintln!("[SR] session ended: {err}");
            let _ = app.emit("voice://error", json!({ "message": voice_error_text(&err) }));
        }
        if com_inited {
            CoUninitialize();
        }
    }
}

/// Plain-English wrapper for WinRT recognizer errors.
fn voice_error_text(err: &str) -> String {
    if err.contains("0x80045509") {
        "Windows needs the speech privacy setting ON once: Settings - Privacy & security - Speech - Online speech recognition.".into()
    } else if err.contains("0x8004550A") {
        "No speech detected. Try again.".into()
    } else if err.contains("0x80070490") {
        "Speech recognition is not available. Install the Speech language in Windows Settings.".into()
    } else {
        format!("Voice input failed: {err}")
    }
}

unsafe fn run_recognizer(app: &AppHandle) -> Result<(), String> {
    unsafe {
        let recognizer = SpeechRecognizer::new().map_err(|e| format!("init: {e:?}"))?;
        if let Ok(mut guard) = ACTIVE.lock() {
            *guard = Some(recognizer.clone());
        }

        // Dictation topic = free-form speech.
        let constraints = recognizer
            .Constraints()
            .map_err(|e| format!("constraints: {e:?}"))?;
        constraints.Clear().map_err(|e| format!("clear: {e:?}"))?;
        let dictation = SpeechRecognitionTopicConstraint::Create(
            SpeechRecognitionScenario::Dictation,
            &windows::core::HSTRING::from("dictation"),
        )
        .map_err(|e| format!("constraint: {e:?}"))?;
        constraints
            .Append(&dictation)
            .map_err(|e| format!("append: {e:?}"))?;
        recognizer
            .CompileConstraintsAsync()
            .map_err(|e| format!("compile: {e:?}"))?
            .get()
            .map_err(|e| format!("compile wait: {e:?}"))?;

        recognizer
            .RecognizeAsync()
            .map_err(|e| format!("recognize: {e:?}"))?
            .get()
            .map_err(|e| format!("recognize wait: {e:?}"))?
            .Text()
            .map_err(|e| format!("text: {e:?}"))
            .map(|text| {
                let trimmed = text.to_string().trim().to_string();
                if !trimmed.is_empty() {
                    let _ = app.emit("voice://final", json!({ "text": trimmed }));
                }
            })
    }
}
