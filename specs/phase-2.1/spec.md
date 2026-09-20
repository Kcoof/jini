# Phase 2.1 Spec — Screenshot with direct AI analysis

Updated: 2026-09-18 after owner review. The screenshot no longer ends in a
folder — it goes straight into the AI chat. (Eyes Check All is a separate,
later phase: one click, no chooser.)

## User flow

1. Click the **camera** tool → the tray shows a chooser:
   - **Full screen** — capture everything now.
   - **Select area** — the screen dims; drag a rectangle; release captures;
     **Esc** cancels.
2. The moment the capture exists it is sent to the AI with the fixed
   question: *"What is in this picture? Tell me anything important, wrong,
   or noteworthy."*
3. The chat panel opens showing a thumbnail of the capture and the answer
   streaming in.
4. Follow-up questions can be typed in the same conversation — the image
   stays in the conversation context (it is stored with the message).
5. The file silently auto-saves to `Pictures\Thuki` + clipboard copy.
   **No Explorer popup.**

## Backend

- `capture.rs`: `capture_display()` (unchanged) + `crop()` (pure,
  unit-tested) + base64 PNG encoding.
- `client.rs`: `ChatMessage.content` becomes text-or-parts; a base64 image
  wraps the user message into OpenAI-compatible vision parts. Unit tests:
  parts shape, blank-image ignored, all previous tests keep passing.
- `commands.rs`: `capture_screen(region: Option<{x,y,w,h}>)` →
  `{ path, base64_png }` (region in physical px, cropped server-side);
  `send_message` gains `image_base64`; the image is persisted with the user
  message so later turns (and history) still see it.

## Frontend

- `ToolsTray`: camera opens a chooser (two buttons in the name-bar area).
- New `SnipOverlay.tsx`: fullscreen dim overlay reusing the main window;
  pointer drag rectangle (CSS px = screen px, window at 0,0), Esc cancels.
- `windowFit.ts`: `fitSnip()` (fullscreen the window) and restore.
- `App.tsx`: chooser → capture (full or region) → open chat → auto-send
  with image; `useChat` renders the thumbnail for messages that carry an
  image.

## Done when (tested alone)

1. Camera → Full screen → chat opens → AI describes the real screen.
2. Camera → Select area → drag a small box → the answer is about that part.
3. Esc during select → nothing captured, no file, back to disc.
4. File appears silently in `Pictures\Thuki`; clipboard holds the image.
5. A follow-up typed question gets an answer that still refers to the
   picture; the conversation is in History (SQLite verified).
6. Gates green: `tsc`, `npm run build`, `cargo test`.
