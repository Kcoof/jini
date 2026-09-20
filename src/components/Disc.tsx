// The round floating disc (specs/phase-0/spec.md 0.4).
//
// Click (no real drag) → onToggle. Drag → move the OS window via pointer
// screenX/Y; on release, dock to the nearest left/right work-area edge.
// Jitter under DRAG_THRESHOLD still counts as a click.

import { useRef, useState } from "react";
import type { PointerEvent } from "react";
import {
  DISC,
  physicalFromCss,
  queueWindowPosition,
  readDragBounds,
  type DragBounds,
} from "../lib/windowFit";

const DRAG_THRESHOLD = 14;

type DragState = {
  pointerId: number;
  startScreenX: number;
  startScreenY: number;
  originCssX: number;
  originCssY: number;
  bounds: DragBounds;
  moved: boolean;
};

type Props = {
  onToggle: () => void;
  /** Disc offset inside the menu window while the tray is open (drag off). */
  discWindow?: { x: number; y: number } | null;
};

export function Disc({ onToggle, discWindow }: Props) {
  const [dragging, setDragging] = useState(false);
  const drag = useRef<DragState | null>(null);
  const skipClick = useRef(false);
  const inMenu = discWindow != null;

  function onPointerDown(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0 || inMenu) return;
    drag.current = {
      pointerId: event.pointerId,
      startScreenX: event.screenX,
      startScreenY: event.screenY,
      originCssX: window.screenX,
      originCssY: window.screenY,
      bounds: readDragBounds(),
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent<HTMLButtonElement>) {
    const state = drag.current;
    if (!state || state.pointerId !== event.pointerId) return;
    const dx = event.screenX - state.startScreenX;
    const dy = event.screenY - state.startScreenY;
    if (!state.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    state.moved = true;
    setDragging(true);
    const cssX = state.originCssX + dx;
    const cssY = state.originCssY + dy;
    const phys = physicalFromCss(cssX, cssY, state.bounds.scale);
    queueWindowPosition(phys.x, phys.y, state.bounds);
  }

  function endDrag(event: PointerEvent<HTMLButtonElement>) {
    const state = drag.current;
    if (!state || state.pointerId !== event.pointerId) return;
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (state.moved) {
      setDragging(false);
      // Drop-anywhere (owner 2026-09-17): the disc stays exactly where the
      // pointer left it — no edge docking, no settle animation. The last
      // queueWindowPosition already clamped to the work area, so a release
      // near the bottom settles just above the taskbar.
      skipClick.current = true;
      return;
    }
    // No real movement → this is a click; onClick fires after pointerup.
  }

  return (
    <button
      type="button"
      className={`disc${dragging ? " dragging" : ""}${inMenu ? " open" : ""}`}
      style={{ width: DISC, height: DISC, left: discWindow?.x ?? 0, top: discWindow?.y ?? 0 }}
      aria-label={inMenu ? "Close Jini tools" : "Jini disc"}
      aria-expanded={inMenu}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onClick={() => {
        if (skipClick.current) {
          skipClick.current = false;
          return;
        }
        onToggle();
      }}
    >
      <span className="disc-ring">
        <span className="disc-core" />
      </span>
    </button>
  );
}
