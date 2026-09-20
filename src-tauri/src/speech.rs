//! Shared SAPI text-to-speech (specs/phase-2.4). One implementation used
//! by both the `speak` agent tool and the `speak_text` command for spoken
//! replies. Fire-and-forget via SPF_ASYNC; SAPI queues on its own thread.

use windows::core::PCWSTR;
use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPF_ASYNC, SPF_PURGEBEFORESPEAK};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

/// Safety cap shared by both callers; long answers are filtered by the
/// frontend before reaching here.
const MAX_SPEAK_CHARS: usize = 500;

/// Speak text aloud via SAPI (fire-and-forget).
pub fn sapi_speak(text: &str) -> Result<(), String> {
    let truncated: String = text.chars().take(MAX_SPEAK_CHARS).collect();
    if truncated.is_empty() {
        return Err("speak: text must not be empty".into());
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let voice: ISpVoice =
            CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| format!("SAPI init: {e}"))?;
        let wide: Vec<u16> = truncated.encode_utf16().chain(std::iter::once(0u16)).collect();
        voice
            .Speak(PCWSTR(wide.as_ptr()), SPF_ASYNC.0 as u32, None)
            .map_err(|e| format!("SAPI speak: {e}"))?;
    }
    Ok(())
}

/// Stop all queued SAPI speech immediately. Speaking an empty string with
/// SPF_PURGEBEFORESPEAK clears the queue on a fresh voice instance.
pub fn stop_sapi_speech() -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let voice: ISpVoice =
            CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| format!("SAPI init: {e}"))?;
        voice
            .Speak(PCWSTR::null(), SPF_PURGEBEFORESPEAK.0 as u32, None)
            .map_err(|e| format!("SAPI stop: {e}"))?;
    }
    Ok(())
}
