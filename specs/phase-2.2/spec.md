# Phase 2.2 — Eyes Check All (spec, written before building)

> STATUS 2026-09-17: BUILT per the approved roadmap (specs/assistant-
> roadmap.md Phase 2.2) and verified with real input; Claude review round
> dispatched. The draft below is the original owner-facing design.

Owner's words: "Eyes Check All in this i want everything to be live not as
screenshot… one click, no chooser, full screen, instant analysis… when I will
take the screenshot direct it should give the analysis and what in this
screen." The screenshot tool (2.1) already captures → analyzes → streams.
Eyes Check All is the SAME pipeline with zero decisions in between.

## Goal

One click on the eye tool → the app immediately captures the whole screen →
sends it to the AI → the chat opens with the answer streaming in. The owner
never picks anything, never draws anything, never waits on a chooser.

## UX flow

1. Disc → tools tray → click **Eyes Check All** (the eye icon, cyan glow).
   - The tool goes LIVE in this phase (was "not in this version" stub).
   - No chooser UI appears. Nothing to choose.
2. Tray closes (same awaited close as the camera fix — no layout race).
3. The whole screen is captured instantly (full monitor, exactly like
   camera → Full screen).
4. The chat panel opens with the picture attached and the answer streaming.
5. At the bottom of the chat, a **Check again** button (near the ask bar):
   one click → captures the screen NOW → sends as a new turn in the SAME
   conversation. This is the "live" feel: check, read, check again.
6. The user can still type follow-up questions about the latest picture in
   the same chat (already works from 2.1).

## The prompt

Different from the camera's neutral "What is in this picture?" — Eyes is a
CHECK, so the prompt asks for a review:

> "Look at this screen. Describe what is on it, then point out anything
> important, wrong, unusual, or that needs attention. Be brief and clear."

## Window lifecycle

- Tray open (444×224) → click eye → `await closeMenuOverlay()` (56×56) →
  capture → `setPanel("chat")` → fitWorkspace (720×540). Identical chaining
  to takeScreenshot("full") — reuse that path, do not fork it.
- If a stream is already running when the eye is clicked: ignore the click
  (same guard as 2.1, never stack streams).

## Check again button

- Rendered in the chat panel only when the last user message carries an
  image (i.e. this conversation is a picture-check conversation).
- Label: **Check again**. On click: guard streaming → capture full screen →
  send with the same Eyes prompt as a NEW turn in the SAME conversation
  (conversationIdRef keeps the thread).
- Disabled (grey, no action) while a stream is running; enabled the moment
  it ends (cursor returns to the ask box as in 2.1).

## Edge cases

- No API key / provider error: chat opens, error line shows in plain
  language (existing error path from 2.1).
- Eye clicked while streaming: nothing happens (guard).
- Check again clicked while streaming: button disabled.
- Everything silent-saves to Pictures\Thuki as before (no popups).

## Build steps (one at a time, gates after each)

1. tools.ts: `eyes` → live: true (its activate path wires to the new
   handler; stub notice stays for voice/inspect/windows).
2. App.tsx: `eyesCheckAll()` — awaited tray close → capture → chat with the
   Eyes prompt (reuse analyzeCapture with a prompt parameter).
3. Check again button in the chat panel (visible when the last user message
   has an image; disabled while streaming).
4. Gates after every step: `npx tsc --noEmit`, `npm run build`, `cargo test`.

## Live tests (each alone, objective)

- Click eye (REAL mouse via tray) → chat opens, picture attached, answer
  streams about the real screen. No chooser ever appears.
- Check again → NEW capture (DB shows a second image message in the SAME
  conversation), answer about the NEW screen state.
- Follow-up typing + Enter during/after streams (2.1 behavior intact).
- Eye click during an active stream → ignored, no double stream.
- SQLite: conversation title from the Eyes prompt, both images persisted.
- Esc/Back → home; disc still drop-anywhere (no regression).

## Out of scope

- Voice, Inspect, Window Switcher (their own phases; stubs stay).
- Periodic/automatic checking (owner said one click each time — "Check
  again" is manual).
- Region selection for Eyes (full screen only by design).
- Nothing committed unless the owner says commit.
