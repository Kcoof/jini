// Region selection overlay (specs/phase-2.1). Two stages:
// 1. Draw: drag anywhere to draw the rectangle.
// 2. Adjust: the box stays on screen — drag inside to MOVE it anywhere,
//    drag an edge/corner to RESIZE it, Enter or the check button captures,
//    Esc or the cross cancels. Nothing outside the box is selectable.

import { useEffect, useRef, useState } from "react";
import type { PointerEvent } from "react";

export type Region = { x: number; y: number; w: number; h: number };

type Props = {
  onRegion: (region: Region) => void;
  onCancel: () => void;
};

type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
type DragMode =
  | { kind: "draw"; ox: number; oy: number }
  | { kind: "move"; ox: number; oy: number; start: Region }
  | { kind: "resize"; handle: Handle; start: Region };

const HANDLES: { id: Handle; x: number; y: number; cursor: string }[] = [
  { id: "nw", x: 0, y: 0, cursor: "nwse-resize" },
  { id: "n", x: 0.5, y: 0, cursor: "ns-resize" },
  { id: "ne", x: 1, y: 0, cursor: "nesw-resize" },
  { id: "e", x: 1, y: 0.5, cursor: "ew-resize" },
  { id: "se", x: 1, y: 1, cursor: "nwse-resize" },
  { id: "s", x: 0.5, y: 1, cursor: "ns-resize" },
  { id: "sw", x: 0, y: 1, cursor: "nesw-resize" },
  { id: "w", x: 0, y: 0.5, cursor: "ew-resize" },
];

function resize(start: Region, handle: Handle, dx: number, dy: number, vw: number, vh: number): Region {
  let { x, y, w, h } = start;
  if (handle.includes("e")) w = Math.max(8, Math.min(start.x + start.w + dx, vw) - x);
  if (handle.includes("s")) h = Math.max(8, Math.min(start.y + start.h + dy, vh) - y);
  if (handle.includes("w")) {
    const nx = Math.max(0, Math.min(start.x + dx, start.x + start.w - 8));
    x = nx;
    w = start.x + start.w - nx;
  }
  if (handle.includes("n")) {
    const ny = Math.max(0, Math.min(start.y + dy, start.y + start.h - 8));
    y = ny;
    h = start.y + start.h - ny;
  }
  return { x, y, w, h };
}

export function SnipOverlay({ onRegion, onCancel }: Props) {
  const [stage, setStage] = useState<"draw" | "adjust">("draw");
  const [rect, setRect] = useState<Region | null>(null);
  const drag = useRef<DragMode | null>(null);
  const [hoverHandle, setHoverHandle] = useState<Handle | null>(null);

  useEffect(() => {
    function onKey(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
        return;
      }
      if (event.key === "Enter" && stage === "adjust" && rect && rect.w >= 8 && rect.h >= 8) {
        event.preventDefault();
        onRegion(rect);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, onRegion, stage, rect]);

  function localPoint(event: PointerEvent<HTMLElement>) {
    return { x: event.clientX, y: event.clientY };
  }

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    // The confirm/cancel buttons need their natural click: capturing the
    // pointer here would retarget the pointerup at the overlay and swallow
    // the button's click entirely (dead ✓ with a real mouse).
    const target = event.target as HTMLElement;
    if (target.closest(".snip-confirm")) return;
    const p = localPoint(event);
    const handle = target.dataset.handle as Handle | undefined;
    if (stage === "adjust" && rect) {
      if (handle) {
        drag.current = { kind: "resize", handle, start: rect };
      } else {
        drag.current = { kind: "move", ox: p.x, oy: p.y, start: rect };
      }
    } else {
      drag.current = { kind: "draw", ox: p.x, oy: p.y };
      setRect({ x: p.x, y: p.y, w: 0, h: 0 });
    }
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const p = localPoint(event);
    const mode = drag.current;
    if (!mode) return;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    if (mode.kind === "draw") {
      const clampedOx = Math.max(0, Math.min(mode.ox, vw));
      const clampedOy = Math.max(0, Math.min(mode.oy, vh));
      const clampedPx = Math.max(0, Math.min(p.x, vw));
      const clampedPy = Math.max(0, Math.min(p.y, vh));
      setRect({
        x: Math.min(clampedOx, clampedPx),
        y: Math.min(clampedOy, clampedPy),
        w: Math.abs(clampedPx - clampedOx),
        h: Math.abs(clampedPy - clampedOy),
      });
      return;
    }
    if (mode.kind === "move") {
      const dx = p.x - mode.ox;
      const dy = p.y - mode.oy;
      setRect({
        x: Math.max(0, Math.min(mode.start.x + dx, vw - mode.start.w)),
        y: Math.max(0, Math.min(mode.start.y + dy, vh - mode.start.h)),
        w: mode.start.w,
        h: mode.start.h,
      });
      return;
    }
    setRect(resize(mode.start, mode.handle, p.x - mode.start.x, p.y - mode.start.y, vw, vh));
  }

  function onPointerUp(event: PointerEvent<HTMLDivElement>) {
    const mode = drag.current;
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (!mode || !rect) return;
    if (mode.kind === "draw") {
      if (rect.w < 8 || rect.h < 8) {
        // Treat a tiny drag as a cancel of the drawing, not a capture.
        setRect(null);
        return;
      }
      setStage("adjust");
    }
  }

  const cursor =
    stage === "adjust" && rect
      ? hoverHandle
        ? (HANDLES.find((h) => h.id === hoverHandle)?.cursor ?? "default")
        : "move"
      : "crosshair";

  return (
    <div
      className="snip-overlay"
      style={{ cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => {
        if (stage === "adjust" && rect && rect.w >= 8 && rect.h >= 8) onRegion(rect);
      }}
    >
      {rect && rect.w > 0 && rect.h > 0 ? (
        <div className="snip-rect" style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}>
          {stage === "adjust"
            ? HANDLES.map((h) => (
                <span
                  key={h.id}
                  data-handle={h.id}
                  className={`snip-handle snip-${h.id}`}
                  style={{
                    left: `calc(${h.x * 100}% - 4px)`,
                    top: `calc(${h.y * 100}% - 4px)`,
                  }}
                  onPointerEnter={() => setHoverHandle(h.id)}
                  onPointerLeave={() => setHoverHandle(null)}
                />
              ))
            : null}
          <span className="snip-size">
            {Math.round(rect.w)} × {Math.round(rect.h)}
          </span>
          {stage === "adjust" ? (
            <span className="snip-confirm">
              <button type="button" className="ok" onClick={() => onRegion(rect)}>
                ✓ Capture
              </button>
              <button type="button" className="ghost" onClick={onCancel}>
                ✕
              </button>
            </span>
          ) : null}
        </div>
      ) : (
        <div className="snip-hint">Drag to select an area · Esc to cancel</div>
      )}
    </div>
  );
}
