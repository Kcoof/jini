// The nine tray tools (specs/phase-0/spec.md 0.5). In Phase 0 only Dismiss
// is live; every other tool is an honest stub ("not in this version").

import type { LucideIcon } from "lucide-react";
import {
  AppWindow,
  Camera,
  Clipboard,
  Crosshair,
  Eye,
  Mic,
  SlidersHorizontal,
  Sparkles,
  X,
} from "lucide-react";

export type ToolId =
  | "screenshot"
  | "eyes"
  | "voice"
  | "inspect"
  | "dismiss"
  | "ask"
  | "clipboard"
  | "windows"
  | "settings";

export type Tool = {
  id: ToolId;
  name: string;
  icon: LucideIcon;
  glow: string;
  live: boolean;
  ring?: boolean;
};

export const TOOLS: Tool[] = [
  { id: "screenshot", name: "Screenshot", icon: Camera, glow: "#f5a524", live: true },
  { id: "eyes", name: "Eyes Check All", icon: Eye, glow: "#22d3ee", live: true },
  { id: "voice", name: "Voice", icon: Mic, glow: "#a78bfa", live: true },
  { id: "inspect", name: "Inspect", icon: Crosshair, glow: "#fb7185", live: false },
  { id: "dismiss", name: "Dismiss", icon: X, glow: "#aab0bc", live: true, ring: true },
  { id: "ask", name: "Ask AI", icon: Sparkles, glow: "#818cf8", live: true },
  { id: "clipboard", name: "Smart Clipboard", icon: Clipboard, glow: "#34d399", live: true },
  { id: "windows", name: "Window Switcher", icon: AppWindow, glow: "#60a5fa", live: false },
  { id: "settings", name: "Settings", icon: SlidersHorizontal, glow: "#94a3b8", live: true },
];
