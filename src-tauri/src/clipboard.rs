//! Clipboard text read (Smart Clipboard context, specs/phase-1/spec.md 1.5).

pub fn read_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok().and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(text)
        }
    })
}
