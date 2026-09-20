# Phase 1 Spec — The AI base

Written: 2026-09-18, before building. Goal: the overlay becomes a working
AI assistant — Ask AI streams answers through one OpenAI-compatible client,
keys live in Windows Credential Manager, history saves locally.

## Step 1.1 — One chat client (Rust)

- `src-tauri/src/api/client.rs`: `join_chat_url(base)` → `{base}/chat/completions`;
  `build_request(model, messages, clipboard_context)` → body with `stream: true`;
  `extract_delta(payload)` → delta content, else reasoning_content, skipping
  `[DONE]`/empty.
- `src-tauri/src/api/presets.rs`: `PresetId` = glm | minimax | openai |
  anymodel | custom with default URLs/models (glm coding
  `https://api.z.ai/api/coding/paas/v4` + `glm-5.3`; minimax
  `https://api.minimax.io/v1` + `MiniMax-M3`; openai
  `https://api.openai.com/v1` + `gpt-4o-mini`; anymodel
  `https://anymodel.org/v1` + `gpt-5.6-sol`).
- Streaming send: POST with Bearer key, honest SSE (emit `chat://chunk` per
  delta, `chat://done` on finish/cancel, `chat://error` with a plain-language
  message). Cancel via a watch channel (`cancel_message`).
- **Unit tests:** URL joins (all presets + trailing slash), request shaping,
  clipboard append/skip, delta extraction. Errors → readable strings
  (401/403/404/429 wording), never raw dumps of keys.

## Step 1.2 — Storage

- SQLite `%APPDATA%\com.thuki.win\thuki.sqlite`: `settings(key,value)` and
  `conversations(id,title,messages,created_at,updated_at)`. Non-secret only.
- Keys → Credential Manager via `keyring` 3 with `windows-native`, service
  `thuki-win`, user `thuki-win/{preset}` (same names as the old app so an
  existing AnyModel key carries over). `get_settings` returns only
  `api_key_set` booleans — never key material.

## Step 1.3 — Settings UI

- Panel (workspace ~720×540 CSS) from the Settings tool: provider dropdown
  (shows "· key saved"), endpoint, model, key field (write-only, password),
  hotkey, Test connection + Save.
- Test connection sends "Reply with the single word: ok" and shows
  `<model> replied: <snippet>` or a readable error.
- Done when: key saved → "key saved" appears; Test connection through
  AnyModel answers ok; key still saved after app restart.

## Step 1.4 — Ask AI + History

- Chat panel: AskBar (Enter sends, Shift+Enter newline, Esc back), streamed
  response panel with Cancel while streaming, ModelSelector chip, History
  list (open/delete conversations).
- `send_message` persists the user message before streaming and the
  assistant reply after `chat://done` (unless cancelled/empty).
- Done when: a real question streams token-by-token through the active
  preset; cancel stops mid-stream; the conversation appears in History and
  reloads.

## Step 1.5 — Smart Clipboard

- `show_window` (tray/hotkey) emits `overlay://shown` with the clipboard
  text; the chat shows a "Context from clipboard" pill (280-char preview,
  Clear button); sending quotes it as `Clipboard context:` per 1.1.
- Done when: copy text → summon → ask "what does my clipboard say" → the
  answer references it; Clear removes it.

## Step 1.6 — Hardening

- Esc in the chat returns home; Dismiss on home hides the overlay
  (`hide_window`); every `overlay://shown` re-fits the window to disc size
  (self-heal); workspace opens call `setFocus()` after resize (WebView2
  blank-paint guard); layout ops stay timeout-guarded.
- Done when: full loop — summon → tray → ask → stream → Esc → hide →
  summon again — works twice in a row.

## Gates (after every step)

`npx tsc --noEmit` · `npm run build` · `cargo test` — all green before the
next step.
