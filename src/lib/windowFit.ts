// Window geometry helpers for the disc overlay (specs/phase-0/spec.md 0.4).
//
// Rules learned the hard way and now baked in:
// - Dragging uses pointer screenX/Y, never clientX (clientX feeds back on
//   window movement and shakes).
// - Position ops are physical px = CSS px × devicePixelRatio.
// - All window size/position changes go through one serialized queue with a
//   timeout, so a hung op can never wedge the disc forever.

import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TOOLS } from "./tools";
import { beginSnip, endSnip } from "./api";

export const DISC = 56;

/* Tray metrics, CSS px — keep in sync with .tools-tray / .tray-scroll /
   .tray-name in index.css. */
export const TRAY_TOOL = 52;
const TRAY_TOOL_GAP = 10;
const TRAY_ROW_H = 72;
const TRAY_NAME_H = 24;
const TRAY_PAD_TOP = 12;
const TRAY_PAD_BOTTOM = 10;
const TRAY_ROW_GAP = 8;
const TRAY_PAD_X = 12;
/** Full tray height, border-box. */
export const TRAY_H =
  TRAY_PAD_TOP + TRAY_ROW_H + TRAY_ROW_GAP + TRAY_NAME_H + TRAY_PAD_BOTTOM;
/** Gap between the disc and the tray. */
export const TRAY_GAP = 12;
/** Transparent margin around disc + tray inside the menu window. */
export const TRAY_MARGIN = 12;
/** Rounding slack so flip math never trips by a fraction of a pixel. */
const TRAY_SLACK = 6;

/** Tray width grows with tools then caps (and scrolls past the cap). */
export function trayWidthFor(viewportW: number): number {
  const n = TOOLS.length;
  const natural = TRAY_PAD_X * 2 + n * TRAY_TOOL + (n - 1) * TRAY_TOOL_GAP;
  const cap = Math.min(420, Math.floor(viewportW * 0.7));
  return Math.min(natural, Math.max(cap, 220));
}

function trayWindowFor(viewportW: number): { w: number; h: number } {
  return {
    w: trayWidthFor(viewportW) + TRAY_MARGIN * 2,
    h: TRAY_MARGIN + DISC + TRAY_GAP + TRAY_H + TRAY_MARGIN + TRAY_SLACK,
  };
}

export type MenuAnchor = { x: number; y: number; flipX?: boolean; flipY?: boolean };

/** Position to restore the disc to when the menu closes. */
let menuRestore: PhysicalPosition | null = null;

export type DragBounds = {
  scale: number;
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
};

/** A hung window op (e.g. resize while hidden) must not wedge the queue. */
const OP_TIMEOUT_MS = 1500;

function withTimeout<T>(op: Promise<T>, fallback: T): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<T>((resolve) => {
    timer = setTimeout(() => resolve(fallback), OP_TIMEOUT_MS);
  });
  return Promise.race([op, timeout]).finally(() => clearTimeout(timer));
}

let layoutTail: Promise<unknown> = Promise.resolve();

function runLayout<T>(fn: () => Promise<T>, timeoutFallback: T): Promise<T> {
  const run = () => withTimeout(fn(), timeoutFallback);
  const next = layoutTail.then(run, run);
  layoutTail = next.then(
    () => undefined,
    () => undefined,
  );
  return next;
}

/** Resize to the disc size (used on summon and when closing things). */
export async function fitDisc(): Promise<void> {
  return runLayout(async () => {
    try {
      await getCurrentWindow().setSize(new LogicalSize(DISC, DISC));
    } catch (err) {
      console.error("[jini] fitDisc failed", err);
    }
  }, undefined);
}

/**
 * Resize to the workspace sheet (settings/chat/history). Taking focus after
 * the resize forces a fresh composition pass — WebView2 stops painting when
 * resized in the background (window on screen, zero pixels).
 */
export async function fitWorkspace(): Promise<void> {
  return runLayout(async () => {
    try {
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(720, 540));
      await win.center();
      try {
        await win.setFocus();
      } catch {
        // Focus can be denied by the OS; Alt+Space twice always recovers.
      }
    } catch (err) {
      console.error("[jini] fitWorkspace failed", err);
    }
  }, undefined);
}

/**
 * Grow the window to disc + tray around the current disc position. The tray
 * unfolds away from the nearest edges (right half grows leftward, no room
 * below opens upward) so the disc never moves. Returns the disc's offset
 * inside the window for tray anchoring.
 */
export async function openMenuOverlay(): Promise<MenuAnchor> {
  const restAnchor: MenuAnchor = { x: TRAY_MARGIN, y: TRAY_MARGIN };
  return runLayout(async () => {
    try {
      const win = getCurrentWindow();
      const scale = window.devicePixelRatio || 1;
      const screen = window.screen as Screen & { availLeft?: number; availTop?: number };
      const availLeft = screen.availLeft || 0;
      const availTop = screen.availTop || 0;
      const availW = window.screen.availWidth;
      const availH = window.screen.availHeight;
      // The disc fills the 56×56 window in this mode, so screenX/Y give its
      // position synchronously — no IPC read needed before layout.
      const discX = window.screenX;
      const discY = window.screenY;
      menuRestore = new PhysicalPosition(
        Math.round(discX * scale),
        Math.round(discY * scale),
      );
      const size = trayWindowFor(availW);
      await win.setSize(new LogicalSize(size.w, size.h));

      const flipX = discX + DISC / 2 > availLeft + availW / 2;
      const roomBelow = discY + DISC + TRAY_GAP + TRAY_H + TRAY_MARGIN <= availTop + availH;
      const roomAbove = discY >= TRAY_MARGIN + TRAY_GAP + TRAY_H + TRAY_SLACK + availTop;
      const flipY = !roomBelow && roomAbove;
      const anchor: MenuAnchor = {
        x: flipX ? size.w - TRAY_MARGIN - DISC : TRAY_MARGIN,
        y: flipY ? size.h - TRAY_MARGIN - DISC : TRAY_MARGIN,
        flipX,
        flipY,
      };
      const cssX = flipX ? discX + DISC - size.w + TRAY_MARGIN : discX - TRAY_MARGIN;
      const cssY = flipY ? discY + DISC - size.h + TRAY_MARGIN : discY - TRAY_MARGIN;
      const clampedX = Math.min(
        Math.max(cssX, availLeft),
        Math.max(availLeft + availW - size.w, availLeft),
      );
      const clampedY = Math.min(
        Math.max(cssY, availTop),
        Math.max(availTop + availH - size.h, availTop),
      );
      await win.setPosition(
        new PhysicalPosition(Math.round(clampedX * scale), Math.round(clampedY * scale)),
      );
      return anchor;
    } catch (err) {
      console.error("[jini] openMenuOverlay failed", err);
      return restAnchor;
    }
  }, restAnchor);
}

/** Shrink back to the disc; the disc returns to where it was. */
export async function closeMenuOverlay(): Promise<void> {
  return runLayout(async () => {
    const restore = menuRestore;
    menuRestore = null;
    try {
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(DISC, DISC));
      if (restore) await win.setPosition(restore);
    } catch (err) {
      console.error("[jini] closeMenuOverlay failed", err);
    }
  }, undefined);
}

/**
 * Snipping-Tool mode: the Rust side unlocks the window (resizable:false +
 * the 56px min make Windows clamp programmatic resizes), expands it over
 * the exact primary-monitor rect, and reports the disc's old spot. The
 * returned closure restores everything.
 */
export async function fitSnip(): Promise<() => Promise<void>> {
  const result = await runLayout(async () => {
    try {
      return await beginSnip();
    } catch (err) {
      console.error("[jini] fitSnip failed", err);
      return null;
    }
  }, null);
  return async () => {
    await runLayout(async () => {
      if (!result) return;
      try {
        await endSnip(result.restore_x, result.restore_y);
      } catch (err) {
        console.error("[jini] snip restore failed", err);
      }
    }, undefined);
  };
}

/**
 * Work-area drag bounds, physical px. `window.screen` excludes the taskbar,
 * so the disc can never rest under it.
 */
export function readDragBounds(): DragBounds {
  const scale = window.devicePixelRatio || 1;
  const screen = window.screen as Screen & { availLeft?: number; availTop?: number };
  const size = DISC * scale;
  const minX = (screen.availLeft || 0) * scale;
  const minY = (screen.availTop || 0) * scale;
  return {
    scale,
    minX,
    minY,
    maxX: minX + window.screen.availWidth * scale - size,
    maxY: minY + window.screen.availHeight * scale - size,
  };
}

export function physicalFromCss(cssX: number, cssY: number, scale: number) {
  return { x: cssX * scale, y: cssY * scale };
}

/** Coalesce window moves to at most one setPosition per animation frame. */
let positionRaf = 0;
let pending: PhysicalPosition | null = null;

export function queueWindowPosition(x: number, y: number, bounds: DragBounds): void {
  const nx = Math.round(Math.min(Math.max(x, bounds.minX), bounds.maxX));
  const ny = Math.round(Math.min(Math.max(y, bounds.minY), bounds.maxY));
  pending = new PhysicalPosition(nx, ny);
  if (positionRaf) return;
  positionRaf = requestAnimationFrame(() => {
    positionRaf = 0;
    const next = pending;
    pending = null;
    if (!next) return;
    void getCurrentWindow().setPosition(next);
  });
}
