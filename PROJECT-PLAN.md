# Thuki-Win — Full Project Plan

Created: 2026-09-11. This is the master plan for building the project from
zero, one step at a time.

**How to use this plan:** before we start any phase, we write a **deep spec
for that phase** in `specs/<phase-name>/spec.md` (details: exact files, exact
behavior, exact test steps). Then we build it, one step at a time. Only after
that phase is finished and tested do we open the next one.

## The idea

A small Windows overlay — a floating AI helper. Press **Alt+Space** → a round
**disc** appears on top of any app. Click it → a **tools tray** with 9 tools.
Tools talk to an AI provider (AnyModel by default). Press **Esc** or
**Alt+Space** to hide. The value: answer without leaving your work.

## The rules for every step (no exceptions)

1. Build **one step at a time**. Never two things at once.
2. Before building a phase: write its deep spec (`specs/<phase>/spec.md`).
3. After every step, all three checks must pass:
   - `npx tsc --noEmit` (TypeScript)
   - `npm run build` (frontend build)
   - `cargo test --manifest-path src-tauri/Cargo.toml` (Rust tests)
4. **Test each feature alone** in the running app (`npm run tauri -- dev`)
   before starting the next one.
5. If a test fails → fix it **before** moving on.
6. Commit only when the owner says "commit".
7. Tools that are not built yet must stay honest stubs
   ("not in this version").

## Phase 0 — Structure (the skeleton, no AI yet)

| Step | What | Test |
|------|------|------|
| 0.1 | Empty Tauri v2 + React + TypeScript + Vite project | app launches |
| 0.2 | Window: 56×56, transparent, always on top, hidden at start | no white box, no taskbar icon |
| 0.3 | Tray icon (Show / Settings / Quit) + Alt+Space global hotkey | show/hide works |
| 0.4 | The disc: click, drag, dock to screen edge | click vs drag feels right |
| 0.5 | Tools tray: 9 icon buttons, horizontal scroll, name bar | opens/closes cleanly |

## Phase 1 — The AI base

| Step | What | Test |
|------|------|------|
| 1.1 | One chat client (OpenAI-compatible, streaming SSE, cancel) | answer streams token by token |
| 1.2 | Presets: glm, minimax, openai, anymodel, custom | switch presets in Settings |
| 1.3 | Settings page; API keys in Windows Credential Manager; SQLite for the rest | key survives app restart |
| 1.4 | Ask AI tool + chat panel + History | ask → stream → saved in history |
| 1.5 | Smart Clipboard (context pill on summon) | AI quotes the clipboard text |
| 1.6 | Dismiss + Esc + self-heal hardening | Alt+Space twice always fixes the window |

## Phase 2 — Extra features, one by one

| # | Feature | What it does |
|---|---------|--------------|
| 1 | **Screenshot** | click camera → PNG saved to `Pictures\Thuki` + copied to clipboard |
| 2 | **Eyes Check All** | captures the screen → sends the picture to the AI → the answer streams in chat |
| 3 | **Window Switcher** | lists open windows → click one → it comes to the front, overlay hides |
| 4 | **Voice** *(needs rule change)* | microphone → speech-to-text |
| 5 | **Inspect** *(needs rule change)* | reads the UI element under the mouse (name, role) |

## Definition of done (every phase and feature)

- The three checks pass.
- The feature tested alone in the real app, twice.
- Every earlier tool still works after it.
- This file updated with a ✓ and the date.

## Progress

- [x] Phase 0 — Structure (2026-09-11: scaffold from zero, 56×56 hidden
  transparent window — needed an explicit `set_min_size` in Rust to beat the
  Windows width floor, tray + Alt+Space verified with real key presses, disc
  drag docks to edge, tray opens/closes 3×, Dismiss works. Note: this build
  runs at CSS px = physical px, DPI scale reads 1.0.)
- [x] Phase 1 — AI base (2026-09-18: one streaming client + presets with 12
  unit tests, SQLite settings, Credential Manager keys — the old AnyModel
  key and old conversations carried over automatically; Ask AI verified
  end-to-end via DB: question sent → streamed reply "42" persisted with
  clipboard context attached; Alt+Space×2 self-heal verified; Settings and
  chat panels open. Cancel/Esc live-click not re-verified this round —
  same code paths proven earlier.)
- [x] Phase 2.1 — Screenshot (2026-09-18: redesigned per owner — camera →
  chooser Full screen / Select area; box can be moved and resized, Esc
  cancels; capture goes straight into an AI chat with the image attached
  and the answer streaming; silent save to Pictures\Thuki. Reviewed by
  Claude (Sonnet 4.6 via opencode/AnyModel): round 1 requested 7 fixes
  (crop clamp, async capture, vision-array guard, no double image, DPI
  scaling, pointercancel, stale conversation id) — all applied, round 2
  VERDICT: OK. Gates: tsc, build, cargo 20/20.)
  - 2026-09-17 Select-area bug hunt: window re-shrank to 56×56 because the
    [panel, menuOpen] effect's closeMenuOverlay landed AFTER begin_snip's
    full-screen expand (Claude diagnosis). Fix: `await closeMenuOverlay()`
    in takeScreenshot before fitSnip. Objectively verified via CDP DOM
    driving: full-screen expand sticks, draw/move/resize box, ✓ Capture →
    region cropped correctly (DPR 1.25 scaled) → chat streams a real
    answer about the selected area, image persisted in SQLite
    (image_base64 355 KB PNG), Esc cancels cleanly, whole flow repeatable.
    scripts/cdp-eval.mjs = deterministic test hook for the WebView.)
  - 2026-09-17 Owner's real-mouse round: ✓ Capture was dead (overlay's
    setPointerCapture stole the button's click), chat couldn't scroll
    (auto grid row made .sheet resolve height:100% against content — fixed
    with minmax(0,100%) tracks + min-height:0), and typing was blocked
    (textarea disabled during streaming — now always enabled, Send waits
    for the stream, cursor auto-returns to the ask box when done). All
    verified with REAL input: mouse click on ✓, OS keystrokes during a
    stream, real Enter follow-up (answer described the picture's colors),
    real wheel scroll. Claude review: VERDICT OK. Lesson: verify buttons
    through the real pointer pipeline — element.click() hides
    pointer-capture bugs.
  - 2026-09-17 Owner round 2 — disc smoothness + style (Claude specced,
    implemented verbatim, VERDICT OK): drop-anywhere — the disc stays
    exactly where released (nearestEdgeX edge-docking deleted; work-area
    clamp keeps it above the taskbar; no settle animation). Disc style:
    NO outer box-shadow in any state (transparent-window artifact), depth
    via inset highlight only, .disc.open gets an indigo inset ring,
    hover/dragging brighten the gradient. Verified with REAL drags:
    mid-screen drop stayed at the drop point; taskbar-zone drop clamped
    to the work-area bottom; click still opens the tray.
- [x] Phase 2.2 — Eyes Check All (2026-09-17: built per Claude's roadmap;
  one click, no chooser, full-screen check + "Check again" in the same
  conversation. Claude plan → implemented → Claude review VERDICT: OK.
  Verified with REAL input: eye click → streamed check; Check again →
  second image in the same conversation (DB verified); button disabled
  while streaming. Note: owner's parallel session had switched the model
  selector to the unconfigured custom preset — sends fail honestly with
  "Set an endpoint in Settings"; switched back to anymodel.)
- [~] Phase 2.3 — Agent loop + 6 read-only tools (2026-09-19: built per
  Claude's spec specs/phase-2.3/spec.md — tools array in chat requests,
  SSE tool_calls reassembly, agent loop (max 10 rounds, 120s timeout,
  cancel-aware), tools module (capture_screen, list_windows,
  get_active_window, read_clipboard, get_datetime, speak via SAPI),
  tools_enabled setting + Settings toggle, gray tool cards, history
  round-trip of tool messages. LIVE provider quirks found and fixed with
  regression tests: finish_reason:"" on unfinished chunks; id/name
  repeated as "" in later fragments. Stale-turn event guard. AT-1/7/10/
  12/13 PASSED live (real windows listed by the model, tool card
  persists, history reopen + follow-up works); Claude review dispatched.
  Old "Window Switcher" phase is absorbed: switching becomes tools.)
- [x] Phase 2.3b — MCP stdio server (2026-09-19: built per Claude's spec
  specs/phase-2.3b/spec.md — thuki-mcp.exe (separate bin, no Tauri app)
  exposes the same 6 read-only tools over MCP stdio via rmcp 3.4; tools
  migrated to tokio::task::spawn_blocking (works in both doors);
  default-run=thuki-win added (second bin made cargo run ambiguous);
  serverInfo thuki-mcp 0.1.0. PROBE PASSED over stdin JSON-RPC:
  6 tools listed with schemas, real datetime, real window list. GUI
  regression (AT-B8) passed. cargo 43/43, tsc, build green. Claude
  review VERDICT OK. Owner steps pending: Claude Desktop + ZCode config
  (AT-B5/B6/B7).)
- [ ] Phase 2.3c — MCP server (HTTP + tunnel: ChatGPT) — planned
- [x] Phase 2.4 — Voice (2026-09-19: Claude spec + fallback spec; TTS
  spoken replies via SAPI (speak_text/stop_speech, auto/always/never
  mode, code-block strip); STT pivoted twice by evidence —
  webkitSpeechRecognition dead in WebView2, SAPI ISpRecoContext blocked
  by a windows-rs 0.58 vtable bug (probed; SetInterest always
  E_INVALIDARG) — final: Windows 11 WinRT SpeechRecognizer (Voice
  Access engine), MTA thread, dictation topic, voice:// events.
  Speech engine installed via UAC (Language.Speech en-US). Mic UX:
  listening bar, Esc/cancel, 5s silence, auto-send, input locked.
  PENDING OWNER: flip Settings → Privacy & security → Speech → Online
  speech recognition ON (0x80045509 until then). Claude VERDICT OK.)
- [x] Phase 2.5 — Capture-on-speak (2026-09-19: Claude spec; mic click
  captures the screen at that moment, transcript sends with the image;
  graceful degradation if capture fails; Check-again appears. Verified:
  fresh PNG saved at click time. Claude VERDICT OK. Full voice round
  trip pending the same privacy switch.)
- [ ] Phase 2.4 — Voice (needs rule change)
- [ ] Phase 2.5 — Inspect (needs rule change)

> 2026-09-17: the owner merged Eyes+Voice into one live assistant idea.
> Claude sorted it into a phased roadmap (2.2 eyes → 2.3 windows → 2.4
> push-to-talk voice → 2.5 capture-on-speak watcher → 2.6 wake word →
> Phase 3 agent-action ladder with confirmations). Full plan:
> specs/assistant-roadmap.md — awaiting owner approval. The list above
> will be renumbered to match once approved.
>
> 2026-09-17 (later): owner pushed back — wants voice+eyes+control
> CONNECTED, plus direct provider tool connection (MCP-style). Live probes
> PROVED gpt-5.6-sol supports tool calling AND tools+images in one
> request. Claude revised the plan into Roadmap v2:
> specs/assistant-roadmap-v2.md — Phase 2.3 agent loop + 6 read-only
> tools, 2.4 voice push-to-talk, 2.5 capture-on-speak, 2.6 actions with
> YES/NO confirm cards, wake word deferred. Awaiting owner approval to
> start 2.3.
>
> 2026-09-17 (latest): owner clarified the MCP idea — Thuki itself becomes
> an MCP SERVER so outside AIs (GPT subscription via connectors, Claude
> Desktop, ZCode) can connect and drive Thuki's tools on the laptop.
> Claude planned it as "Door 2": phases 2.3b (stdio transport for Claude
> Desktop/ZCode) and 2.3c (HTTP + free Cloudflare tunnel for ChatGPT),
> localhost-only + bearer token, YES/NO cards stay in Thuki for every
> action. Plan saved: specs/mcp-door-plan.md. Awaiting owner approval.

- [~] Phase 2.6 — Action tools (2026-09-19: built per Claude's spec
  specs/phase-2.6/spec.md, ALL sub-steps: confirmation gate (yellow
  YES/NO card, 60s auto-decline, cancel-aware, Esc=NO), six tools
  (open_path, click_at, type_text, press_keys, find_elements UIA,
  launch_app), safety guard (system dirs + dangerous exes blocked,
  length caps), actions_enabled setting DEFAULT OFF, MCP door stays
  read-only. LIVE-PROVEN: "open https://example.com" -> YES -> Edge
  opened it; "open openai.com" -> NO -> declined + acknowledged. cargo
  54/54, tsc, build green. ALSO FIXED real bug: useSettings.update
  dropped every post-Phase-1 setting on save (voice/tools/actions
  toggles never persisted) — now sends full merged settings. Claude
  review PENDING: reviewer endpoint erroring after 4 attempts; re-run
  when back.)

## Rename: Thuki -> Jini (2026-09-20)

The owner picked the name Jini. Identifier com.jini.app, exes
jini.exe / jini-mcp.exe, lib jini_lib, MCP serverInfo "jini", tray
tooltip added, all UI strings/logs switched, screenshots now save to
Pictures/Jini. Data migrations run once at startup: the Thuki sqlite is
COPIED to %APPDATA%/com.jini.app/jini.sqlite and old thuki-win
Credential Manager entries are copied to the new "jini" service names
(old data never deleted). Verified live: history + GLM settings carried,
migrated key answered a screenshot, PNG saved to Pictures/Jini, MCP
probe on release jini-mcp (6 read-only tools, serverInfo jini). ZCode
config updated to jini-mcp. Old specs/docs keep the historical name.
Claude review pending with the 2.6 review.

## Note

The old app exists in `Desktop\eyes-backup-2026-09-11.tar.gz` (source + git
history, no node_modules). Phase 0+1 can be restored from it in minutes
instead of hours — decide before starting Phase 0: **restore or rebuild**.
