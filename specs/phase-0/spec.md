# Phase 0 Spec — Structure (the skeleton, no AI)

Written: 2026-09-11, before building. Decision: **build new from zero**
(no restore from the backup).

## Goal

An empty Thuki-Win skeleton that launches, hides, shows on Alt+Space or the
tray, and shows a working disc + tools tray. No AI, no settings, no chat in
this phase.

## Step 0.1 — Project scaffold

**Files created:** standard Tauri v2 + React + TypeScript + Vite layout —
`package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`,
`src/main.tsx`, `src/App.tsx`, `src/index.css`, `src-tauri/Cargo.toml`,
`src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`,
`src-tauri/icons/`, `src-tauri/capabilities/default.json`.

**Config values:**
- identifier: `com.thuki.win`
- productName: `Thuki-Win`
- devUrl: `http://localhost:1420` (port fixed, strictPort)

**Done when:** `npm run tauri dev` launches the app; `npx tsc --noEmit`,
`npm run build`, `cargo test` all pass.

## Step 0.2 — Overlay window

`tauri.conf.json` window `main`: width/height 56, `visible: false`,
`transparent: true`, `decorations: false`, `alwaysOnTop: true`,
`skipTaskbar: true`, `shadow: false`, `center: true`,
`backgroundColor: #00000000`. Main window background set transparent from
Rust on setup. No `backdrop-filter` on any full-window element (WebView2
white-box bug).

**Done when:** app launches with NO visible window (checked: window exists,
`IsWindowVisible` = false, no taskbar entry).

## Step 0.3 — Tray + hotkey

- Tray icon with menu: **Show / Settings / Quit**. Show and Settings both
  call show_window (unminimize, show, set_focus). Quit exits.
- Global hotkey **Alt+Space** (config value in code for now): toggles
  window visibility. Plugin: `tauri-plugin-global-shortcut`.
- Commands this phase: `toggle_window`, `hide_window`.

**Done when:** real Alt+Space press hides and shows the window (verified
twice); tray menu Show also works.

## Step 0.4 — The disc

`src/components/Disc.tsx` — a 56×56 round button filling the window.
- **Click** (no drag): emits a toggle event to open/close the tools tray.
- **Drag**: moves the OS window using `event.screenX/Y` + `window.screenX/Y`
  (never `clientX` — feedback loop). Jitter under 14px still counts as a
  click. On release, the disc docks to the nearest left/right work-area edge
  (taskbar-aware, `screen.avail*`).
- Physical px = CSS px × `devicePixelRatio` for `setPosition`.
- No `backdrop-filter` on the disc.

**Done when:** click opens/closes the tray (0.5); drag moves the window
smoothly; release docks to an edge.

## Step 0.5 — Tools tray

`src/components/ToolsTray.tsx`, `src/lib/tools.ts`, `src/lib/windowFit.ts`.
- One horizontal row of nine ~52px icon buttons: Screenshot, Eyes Check All,
  Voice, Inspect, Dismiss (ringed), Ask AI, Smart Clipboard, Window Switcher,
  Settings. Icons: `lucide-react`.
- **Visible horizontal scrollbar** lane under the row; name bar under that.
- Width grows with tools then caps at ~min(420, 70% screen) and scrolls.
- `openMenuOverlay` grows the window around the disc (flip X/Y near edges so
  the disc never jumps). `closeMenuOverlay` shrinks back and restores the
  disc position. All window size/position ops go through one serialized
  queue (`runLayout`) with a timeout so a hung op can never wedge the disc.
- In this phase ALL tools except **Dismiss** show the honest stub notice
  "… — not in this version". Dismiss closes the tray.

**Done when:** tray opens/closes cleanly 3× in a row; scrollbar visible;
stub notices show; disc stays put when the tray opens near screen edges.

## Test checklist for the whole phase (run at the end)

1. App starts hidden. 2. Alt+Space shows the disc (no white box).
3. Click disc → tray. 4. Drag disc → docks. 5. Every stub tool shows its
notice. 6. Dismiss closes. 7. Alt+Space hides. 8. Gates green.
