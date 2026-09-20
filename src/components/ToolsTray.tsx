// The square frosted tools tray (specs/phase-0/spec.md 0.5): one horizontal
// row of icon buttons over a visible scrollbar lane, name bar underneath.
// Phase 0: only Dismiss is live; the rest are honest stubs.

import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { TOOLS } from "../lib/tools";
import type { Tool, ToolId } from "../lib/tools";
import {
  DISC,
  TRAY_GAP,
  TRAY_H,
  TRAY_MARGIN,
  trayWidthFor,
  type MenuAnchor,
} from "../lib/windowFit";

const NOTICE_MS = 2800;

type Props = {
  anchor: MenuAnchor | null;
  onAskAi: () => void;
  onClipboard: () => void;
  onSettings: () => void;
  onClose: () => void;
  onScreenshot: (mode: "full" | "select") => void;
  onEyesCheck: () => void;
  onVoiceActivate: () => void;
};

export function ToolsTray({
  anchor,
  onAskAi,
  onClipboard,
  onSettings,
  onClose,
  onScreenshot,
  onEyesCheck,
  onVoiceActivate,
}: Props) {
  const [hovered, setHovered] = useState<ToolId | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [overflowing, setOverflowing] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const noticeTimer = useRef<number | undefined>(undefined);
  const scrollRef = useRef<HTMLDivElement>(null);

  const trayW = trayWidthFor(window.screen.availWidth);
  const spot = anchor ?? { x: TRAY_MARGIN, y: TRAY_MARGIN };
  const flipX = spot.flipX ?? false;
  const flipY = spot.flipY ?? false;
  const left = flipX ? spot.x + DISC - trayW : spot.x;
  const top = flipY ? spot.y - TRAY_GAP - TRAY_H : spot.y + DISC + TRAY_GAP;

  useEffect(() => {
    const el = scrollRef.current;
    setOverflowing(!!el && el.scrollWidth > el.clientWidth);
  }, [trayW]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    // Vertical wheel scrolls the row — trackpads with deltaX keep native.
    const onWheel = (event: WheelEvent) => {
      if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
      el.scrollLeft += event.deltaY;
      event.preventDefault();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  useEffect(() => () => window.clearTimeout(noticeTimer.current), []);

  function activate(tool: Tool) {
    switch (tool.id) {
      case "dismiss":
        onClose();
        return;
      case "ask":
        if (tool.live) onAskAi();
        else showNotice(tool);
        return;
      case "clipboard":
        if (tool.live) onClipboard();
        else showNotice(tool);
        return;
      case "settings":
        if (tool.live) onSettings();
        else showNotice(tool);
        return;
      case "screenshot":
        if (tool.live) setChoosing(true);
        else showNotice(tool);
        return;
      case "eyes":
        // One click, no chooser: straight to a full-screen check.
        if (tool.live) onEyesCheck();
        else showNotice(tool);
        return;
      case "voice":
        // Push-to-talk: chat opens and the mic starts listening.
        if (tool.live) onVoiceActivate();
        else showNotice(tool);
        return;
      default:
        showNotice(tool);
    }
  }

  function showNotice(tool: Tool) {
    // Reserved tools stay honest: report, never fake a capture or scan.
    setHovered(null);
    setNotice(`${tool.name} — not in this version`);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), NOTICE_MS);
  }

  const hoveredTool = TOOLS.find((t) => t.id === hovered);
  const idleLabel = overflowing ? "Scroll for more tools →" : "Hover a tool for its name";
  const label = hoveredTool ? hoveredTool.name : (notice ?? idleLabel);

  return (
    <div
      className="tools-tray"
      style={
        {
          left,
          top,
          width: trayW,
          height: TRAY_H,
          transformOrigin: `${flipX ? "right" : "left"} ${flipY ? "bottom" : "top"}`,
        } as CSSProperties
      }
      role="group"
      aria-label="Jini tools"
    >
      <div className="tray-scroll" ref={scrollRef}>
        {TOOLS.map((tool) => {
          const Icon = tool.icon;
          return (
            <button
              key={tool.id}
              type="button"
              className={`tray-tool${tool.ring ? " ringed" : ""}`}
              style={{ "--glow": tool.glow } as CSSProperties}
              aria-label={tool.name}
              onMouseEnter={() => setHovered(tool.id)}
              onMouseLeave={() => setHovered(null)}
              onFocus={() => setHovered(tool.id)}
              onBlur={() => setHovered(null)}
              onClick={() => activate(tool)}
            >
              {tool.ring ? (
                <span className="tool-ring">
                  <Icon size={15} strokeWidth={2.4} />
                </span>
              ) : (
                <Icon size={21} strokeWidth={1.9} />
              )}
            </button>
          );
        })}
      </div>
      <div
        className={`tray-name${hoveredTool || notice ? "" : " idle"}`}
        style={hoveredTool ? { color: hoveredTool.glow } : undefined}
        aria-live="polite"
      >
        {choosing ? (
          <span className="tray-chooser">
            <button type="button" onClick={() => onScreenshot("full")}>
              Full screen
            </button>
            <button type="button" onClick={() => onScreenshot("select")}>
              Select area
            </button>
            <button type="button" className="ghost" onClick={() => setChoosing(false)}>
              ✕
            </button>
          </span>
        ) : (
          label
        )}
      </div>
    </div>
  );
}
