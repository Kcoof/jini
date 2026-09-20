// Capture the Jini WebView UI as a PNG with a transparent background via CDP.
// Usage: node scripts/cdp-shot.mjs <out.png>
// Needs the app started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
import { writeFileSync } from "node:fs";

const out = process.argv[2] || "shot.png";

const list = await (await fetch("http://127.0.0.1:9222/json")).json();
const page = list.find((t) => t.type === "page" && t.url.includes("localhost:1420"));
if (!page) {
  console.error("NO PAGE TARGET. Targets:", list.map((t) => `${t.type}:${t.url}`).join(", "));
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 1;
const pending = new Map();

function send(method, params) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(JSON.stringify(msg.error)));
    else resolve(msg.result);
  }
});

await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve);
  ws.addEventListener("error", reject);
});

try {
  await send("Emulation.setDefaultBackgroundColorOverride", {
    color: { r: 0, g: 0, b: 0, a: 0 },
  });
  const shot = await send("Page.captureScreenshot", { format: "png" });
  try {
    await send("Emulation.setDefaultBackgroundColorOverride", {});
  } catch {
    // not all Chromium builds support clearing; the override dies with the WS anyway
  }
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log(`SAVED ${out}`);
} finally {
  ws.close();
}
